//! The device's settings, as the communicator shows and changes them.
//!
//! Every setting is a property of the device's own ULCP session, reached
//! through a [`LocalExchange`]. A change is a request the session serves as
//! it serves any host's, and what the session publishes keeps the cache
//! current, whoever made the change: this interface, an attached phone, or
//! an administrator across the mesh.
//!
//! A setter changes what its read reports at once. The outcome follows as
//! an event: [`NodeEvent::SettingsChanged`] once the device has applied
//! it, or [`NodeEvent::SettingRefused`], after which the read reports what
//! the device kept.
//!
//! **Saving.** The device keeps an existing snapshot in step with every
//! change made here (`ulcp-saved-state.md`, Changes Made at the Device),
//! but never makes one. So the first boot saves one, and a change whose
//! loss would matter—the name, the radio—is followed by a save when the
//! device has none in effect. Brightness and the keyboard never ask for
//! one.
//!
//! **Peers and channels.** The device's store keeps `PROP_DEV_PEERS` and
//! `PROP_DEV_CHANNEL_KEYS` outside the snapshot, so changing them asks for
//! no save. A communicator starts out on `public` and `EMERGENCY`: they are
//! what its channel keys hold after a reset (`ulcp-device.md`, Lists Held
//! by the Device's Store; see
//! [`ChannelCrypto::starting_keys`](crate::ChannelCrypto::starting_keys)).

use umsh_core::ChannelTag;
use umsh_ulcp::ids::{keyboard_aids, prop, saved};
use umsh_ulcp::profiles::VETTED;
use umsh_ulcp_admin::descriptor::Value;
use umsh_ulcp_admin::{
    Event, Freshness, LocalExchange, LocalPort, Model, Refusal, WriteId, WriteOutcome,
};
use umsh_ux_communicator::avatar::{AvatarStyle, PeerShape};
use umsh_ux_communicator::heapless::{Deque, String};
use umsh_ux_communicator::model::{
    KeyboardSettings, NodeEvent, Profile, RadioSettings, Setting, ShareUri, brightness_at,
};

use crate::chat::address;

/// How long after a change worth saving the device is asked to save it, so
/// that a burst of changes is saved once.
pub const SAVE_DELAY_MS: u64 = 2_000;

/// How often the airtime spent is read again. The device counts it in
/// four-minute steps.
pub const AIRTIME_READ_MS: u64 = 60_000;

/// The screen's brightness until the device reports it.
pub const DEFAULT_BRIGHTNESS: u8 = brightness_at(5);

/// How many recent writes are remembered by the setting they changed. The
/// local link answers in milliseconds, far sooner than this many changes
/// can be made.
const WRITES_KEPT: usize = 16;

/// How many saves and forgets of peers are remembered until the device
/// answers them.
const PEER_WRITES_KEPT: usize = 8;

/// The settings half of the communicator's node.
pub struct Settings<P> {
    model: Model,
    exchange: LocalExchange<P>,
    /// The setting each recent write changed, oldest first.
    writes: Deque<(WriteId, Setting), WRITES_KEPT>,
    /// Peers saved, or with `false` forgotten, that the device has yet to
    /// answer for, oldest first.
    peer_writes: Deque<(WriteId, [u8; 32], bool), PEER_WRITES_KEPT>,
    /// When to ask the device to save.
    save_at: Option<u64>,
    /// Whether the device's saved state at boot has been looked at.
    boot_checked: bool,
    /// When the airtime spent is next read.
    airtime_at: u64,
    now_ms: u64,
}

impl<P: LocalPort> Settings<P> {
    /// Settings read over `port`. Nothing is known until the first
    /// [`Settings::pump`] has asked and the device has answered.
    pub fn new(port: P) -> Self {
        let mut model = Model::new();
        model.refresh();
        Self {
            model,
            exchange: LocalExchange::new(port),
            writes: Deque::new(),
            peer_writes: Deque::new(),
            save_at: None,
            boot_checked: false,
            airtime_at: AIRTIME_READ_MS,
            now_ms: 0,
        }
    }

    pub fn port(&self) -> &P {
        self.exchange.port()
    }

    /// Whether every setting has been read once.
    pub fn primed(&self) -> bool {
        self.boot_checked
    }

    /// Hands what is waiting to the device and takes in what has come back,
    /// passing on what the interface should hear to `events`. The board
    /// calls it after the interface has changed something, and whenever
    /// the device has something for it.
    pub fn pump(&mut self, now_ms: u64, events: &mut impl FnMut(NodeEvent)) {
        self.now_ms = now_ms;
        if now_ms >= self.airtime_at {
            self.airtime_at = now_ms + AIRTIME_READ_MS;
            self.model.read(prop::PHY_DUTY_NOW);
        }
        if self.save_at.is_some_and(|at| now_ms >= at) {
            self.save_at = None;
            // The device refuses a save only when it has no saved state to
            // keep, which no change made here can fix.
            let _ = self.model.save();
        }
        loop {
            self.model.pump(&mut self.exchange, now_ms);
            let mut took = false;
            while let Some(event) = self.model.next_event() {
                took = true;
                self.take(event, events);
            }
            // Until nothing comes back at once: a save asked for here, or
            // an answer a port gave while being asked.
            if !self.check_boot() && !took {
                break;
            }
        }
    }

    /// When [`Settings::pump`] next has something to do without being
    /// woken by the device.
    pub fn next_deadline(&self) -> Option<u64> {
        Some(
            self.save_at
                .map_or(self.airtime_at, |at| at.min(self.airtime_at)),
        )
    }

    fn take(&mut self, event: Event, events: &mut impl FnMut(NodeEvent)) {
        match event {
            Event::Changed(_) | Event::Resync => events(NodeEvent::SettingsChanged),
            Event::Written { id, outcome, .. } => {
                if self.peer_writes.iter().any(|(write, ..)| *write == id) {
                    self.peer_writes.retain(|(write, ..)| *write != id);
                    events(NodeEvent::SettingsChanged);
                    return;
                }
                let Some(setting) = self
                    .writes
                    .iter()
                    .find(|(write, _)| *write == id)
                    .map(|&(_, setting)| setting)
                else {
                    return;
                };
                match outcome {
                    WriteOutcome::Applied | WriteOutcome::Adjusted => {
                        if matters(setting) {
                            self.save_if_unsaved();
                        }
                        events(NodeEvent::SettingsChanged);
                    }
                    WriteOutcome::Refused(_)
                    | WriteOutcome::Skipped
                    | WriteOutcome::Unknown
                    | WriteOutcome::Unsent => events(NodeEvent::SettingRefused(setting)),
                }
            }
            Event::Saved { .. } | Event::Unavailable { .. } => {}
        }
    }

    /// At first boot nothing is saved: save, so that a snapshot is in
    /// effect from then on. Returns whether a save was asked for.
    fn check_boot(&mut self) -> bool {
        if self.boot_checked
            || !matches!(self.model.freshness(prop::SAVED), Freshness::Fresh { .. })
        {
            return false;
        }
        self.boot_checked = true;
        self.model.saved() == Some(saved::NONE) && self.model.save().is_ok()
    }

    /// A change that would matter if it were lost is saved, unless the
    /// device kept a snapshot in step with it already.
    fn save_if_unsaved(&mut self) {
        if self.model.saved() != Some(saved::CURRENT) {
            self.save_at = Some(self.now_ms + SAVE_DELAY_MS);
        }
    }

    // ─── Changes ─────────────────────────────────────────────────────
    //
    // Each returns whether the change was taken: false when the device
    // cannot have it, such as a property it does not have. The change goes
    // out at the next pump.

    pub fn set_name(&mut self, name: &str) -> bool {
        let write = self.model.set_device_name(name);
        self.written(Setting::Name, write)
    }

    /// Adopts the vetted profile at `index`.
    pub fn set_profile(&mut self, index: usize) -> bool {
        let Some(profile) = VETTED.get(index) else {
            return false;
        };
        let write = self.model.set_profile(profile);
        self.written(Setting::Profile, write)
    }

    pub fn set_transmit_power(&mut self, dbm: i8) -> bool {
        let write = self.model.set_tx_power_dbm(dbm);
        self.written(Setting::TransmitPower, write)
    }

    pub fn set_forwarding(&mut self, on: bool) -> bool {
        let write = self.model.set_forwarding(on);
        self.written(Setting::Forwarding, write)
    }

    pub fn set_share_location(&mut self, on: bool) -> bool {
        let write = self.model.set_share_location(on);
        self.written(Setting::ShareLocation, write)
    }

    pub fn set_keyboard(&mut self, settings: KeyboardSettings) -> bool {
        let mut aids = 0;
        if settings.capitalize {
            aids |= keyboard_aids::CAPITALIZE;
        }
        if settings.period {
            aids |= keyboard_aids::PERIOD;
        }
        let write = self.model.set_many(&[
            (prop::KEYBOARD_AIDS, &[aids]),
            (prop::KEYBOARD_LIGHT, &[u8::from(settings.light)]),
        ]);
        self.written(Setting::Keyboard, write)
    }

    pub fn set_brightness(&mut self, brightness: u8) -> bool {
        let write = self.model.set_brightness(brightness);
        self.written(Setting::Brightness, write)
    }

    /// Keeps `key` among the device's peers.
    pub fn save_peer(&mut self, key: &[u8; 32]) -> bool {
        let write = self.model.save_peer(key);
        self.peer_written(key, true, write)
    }

    /// No longer keeps `key` among the device's peers.
    pub fn forget_peer(&mut self, key: &[u8; 32]) -> bool {
        let write = self.model.forget_peer(key);
        self.peer_written(key, false, write)
    }

    fn peer_written(
        &mut self,
        key: &[u8; 32],
        saved: bool,
        write: Result<WriteId, Refusal>,
    ) -> bool {
        let Ok(id) = write else {
            return false;
        };
        if self.peer_writes.is_full() {
            self.peer_writes.pop_front();
        }
        let _ = self.peer_writes.push_back((id, *key, saved));
        true
    }

    fn written(&mut self, setting: Setting, write: Result<WriteId, Refusal>) -> bool {
        let Ok(id) = write else {
            return false;
        };
        if self.writes.is_full() {
            self.writes.pop_front();
        }
        let _ = self.writes.push_back((id, setting));
        true
    }

    // ─── Reads ───────────────────────────────────────────────────────

    /// `PROP_DEV_KEY`: the device identity, once read.
    pub fn key(&self) -> Option<&[u8; 32]> {
        self.model.device_key()
    }

    /// The device identity's address.
    pub fn address(&self) -> String<44> {
        self.key().map(address).unwrap_or_default()
    }

    /// The device identity's peers: what `PROP_DEV_PEERS` holds once the
    /// device has answered every save and forget asked of it.
    pub fn peers(&self) -> alloc::vec::Vec<[u8; 32]> {
        let mut peers: alloc::vec::Vec<[u8; 32]> = self.model.peers().copied().collect();
        for (_, key, saved) in &self.peer_writes {
            peers.retain(|peer| peer != key);
            if *saved {
                peers.push(*key);
            }
        }
        peers
    }

    /// The channels the device identity holds, by tag.
    pub fn channels(&self) -> impl Iterator<Item = ChannelTag> + '_ {
        self.model.channels().map(|tag| ChannelTag(*tag))
    }

    /// How the device identity is drawn: its key's leading octets as the
    /// fill, and its address's first characters as the lettering.
    pub fn avatar(&self) -> AvatarStyle {
        let key = self.key().copied().unwrap_or_default();
        let address = self.address();
        AvatarStyle::peer(
            [key[0], key[1], key[2]],
            address.get(..4).unwrap_or_default(),
            PeerShape::Circle,
        )
    }

    /// The link that shares the device identity.
    pub fn share_uri(&self) -> Option<ShareUri> {
        self.key()?;
        let mut uri = ShareUri::new();
        uri.push_str("umsh:n:").ok()?;
        uri.push_str(&self.address()).ok()?;
        Some(uri)
    }

    pub fn device_name(&self) -> String<24> {
        let mut name = String::new();
        if let Some(Value::Text(text)) = self.model.intended(prop::DEV_NAME) {
            for c in text.chars() {
                if name.push(c).is_err() {
                    break;
                }
            }
        }
        name
    }

    pub fn radio(&self) -> RadioSettings {
        let profile = match (
            self.u32(prop::PHY_FREQ),
            self.u32(prop::PHY_LORA_BW),
            self.u8(prop::PHY_LORA_SF),
            self.u8(prop::PHY_LORA_CR),
        ) {
            (Some(freq), Some(bw), Some(sf), Some(cr)) => VETTED
                .iter()
                .position(|profile| profile.interoperates_with(freq, bw, sf, cr))
                .map_or(Profile::Custom, Profile::Vetted),
            _ => Profile::Custom,
        };
        let airtime = self.model.duty_now().map_or(0, |duty| {
            (u32::from(duty) * 100 / u32::from(u16::MAX)) as u8
        });
        RadioSettings {
            profile,
            forwarding: self.bool(prop::MAC_REPEATER_ENABLED).unwrap_or(false),
            share_location: self.bool(prop::GNSS_IDENT_UPDATE).unwrap_or(false),
            transmit_power: match self.model.intended(prop::PHY_TX_POWER) {
                Some(Value::I8(dbm)) => dbm,
                _ => 0,
            },
            airtime,
        }
    }

    pub fn keyboard(&self) -> KeyboardSettings {
        let aids = self.u8(prop::KEYBOARD_AIDS).unwrap_or(keyboard_aids::ALL);
        KeyboardSettings {
            capitalize: aids & keyboard_aids::CAPITALIZE != 0,
            period: aids & keyboard_aids::PERIOD != 0,
            light: self.bool(prop::KEYBOARD_LIGHT).unwrap_or(true),
        }
    }

    pub fn brightness(&self) -> u8 {
        self.u8(prop::DISPLAY_BRIGHTNESS)
            .unwrap_or(DEFAULT_BRIGHTNESS)
    }

    fn bool(&self, key: u32) -> Option<bool> {
        match self.model.intended(key)? {
            Value::Bool(value) => Some(value),
            _ => None,
        }
    }

    fn u8(&self, key: u32) -> Option<u8> {
        match self.model.intended(key)? {
            Value::U8(value) => Some(value),
            _ => None,
        }
    }

    fn u32(&self, key: u32) -> Option<u32> {
        match self.model.intended(key)? {
            Value::U32(value) => Some(value),
            _ => None,
        }
    }
}

/// Whether losing a change to `setting` would matter enough to save one.
const fn matters(setting: Setting) -> bool {
    !matches!(setting, Setting::Keyboard | Setting::Brightness)
}
