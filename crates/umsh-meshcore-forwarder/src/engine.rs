//! The forwarding decision: what a baseline repeater does with each
//! packet it hears, and the frame it sends in reply.
//!
//! The order of the checks, and which of them record a packet as seen,
//! follow the baseline's receive path, because both decide what a
//! neighbor receives: a copy recorded here is one this device will never
//! forward again, whichever route it arrives by.

use heapless::Vec;
use umsh_hal::Snr;

use crate::packet::{
    self, Frame, MAX_PATH_LEN, Packet, PacketHash, ParseError, RouteType, payload_type,
};
use crate::queue::DelayQueue;
use crate::region::{RegionPolicy, TransportKey};
use crate::seen::SeenTable;

/// Regions the forwarder can hold keys for: as many as
/// `PROP_MESHCORE_REGIONS` holds names.
pub const MAX_REGIONS: usize = 8;

/// The baseline repeater's flood limits. A flood is not forwarded once
/// its received path count reaches any limit that applies to it.
pub const FLOOD_MAX: usize = 64;
/// The limit for unscoped (`FLOOD`) packets.
pub const FLOOD_MAX_UNSCOPED: usize = 64;
/// The limit for advertisements.
pub const FLOOD_MAX_ADVERT: usize = 8;

/// Retransmit-delay factor for floods, in thousandths of the frame's
/// airtime.
pub const FLOOD_DELAY_FACTOR_MILLI: u32 = 500;
/// Retransmit-delay factor for direct traffic, in thousandths.
pub const DIRECT_DELAY_FACTOR_MILLI: u32 = 300;

/// Spacing the baseline expects between the parts of a multipart
/// acknowledgment.
const MULTIPART_ACK_SPACING_MS: u32 = 300;

/// The most entries a path-length octet can count.
const MAX_PATH_COUNT: usize = 0x3F;

/// Queue priority of a forwarded direct TRACE.
const TRACE_PRIORITY: u8 = 5;

/// The SNR a TRACE records for a hop that was not a radio link: the
/// backhaul between two bridged devices. The best value the field can
/// hold, so a trace shows the bridge as the excellent link it is.
pub const UNMEASURED_TRACE_SNR: i8 = i8::MAX;

/// How a frame reached the forwarder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Heard {
    /// Off the air, with the radio's measurements.
    Air { rssi: i16, snr: Snr },
    /// Transmitted by another client of this radio. It is already on the
    /// air from here, so it is remembered and never forwarded.
    LocalTx,
    /// Delivered across a backhaul, with no measurement to judge.
    Backhaul,
}

/// What the forwarder may forward, mirrored from the device domain.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    /// `PROP_MESHCORE_REGION_POLICY`.
    pub region_policy: RegionPolicy,
    /// Keys of the public regions in `PROP_MESHCORE_REGIONS`.
    pub regions: Vec<TransportKey, MAX_REGIONS>,
    /// `PROP_MAC_REPEATER_MIN_RSSI`, in dBm.
    pub min_rssi: Option<i16>,
    /// `PROP_MAC_REPEATER_MIN_SNR`, in whole dB.
    pub min_snr: Option<i8>,
}

impl Settings {
    /// Derive the region keys from the stored region strings. Private
    /// (`$`) regions contribute none.
    pub fn with_region_names<'a>(mut self, names: impl IntoIterator<Item = &'a [u8]>) -> Self {
        self.regions = names
            .into_iter()
            .filter_map(TransportKey::for_region_name)
            .take(MAX_REGIONS)
            .collect();
        self
    }

    /// Whether a measured reception clears the link-quality thresholds.
    fn link_passes(&self, rssi: i16, snr: Snr) -> bool {
        self.min_rssi.is_none_or(|min| rssi >= min)
            && self
                .min_snr
                .is_none_or(|min| snr >= Snr::from_decibels(min))
    }
}

/// What the forwarder needs from the device it runs on.
pub trait Environment {
    /// Airtime in milliseconds of a `len`-octet frame at the active
    /// modulation.
    fn airtime_ms(&self, len: usize) -> u32;
    /// A uniform 32-bit draw from a cryptographic generator.
    fn random_u32(&mut self) -> u32;
}

/// Why a well-formed packet was not forwarded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Declined {
    /// A direct packet routed through some other node.
    NotNextHop,
    /// A direct TRACE that has reached the end of its path.
    TraceComplete,
    /// A zero-hop direct packet, which goes no further than its sender's
    /// neighbors.
    ZeroHop,
    /// A direct CONTROL packet in the subset that never travels.
    LocalControl,
    /// A payload type that is not flooded.
    PayloadType,
    /// Shorter than its payload type requires.
    Incomplete,
    /// An advertisement whose signature does not verify.
    ForgedAdvert,
    /// No room left in the path for this device's entry.
    PathFull,
    /// The path count has reached a flood limit.
    HopLimit,
    /// The region policy refused the flood.
    Region,
}

/// What became of one received frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Not a MeshCore v1 frame: another protocol on the same channel.
    Foreign,
    /// A MeshCore v1 header over a frame the baseline would reject.
    Malformed,
    /// A copy of another local client's transmission, remembered.
    Remembered,
    /// A measured flood below the link-quality thresholds. Not
    /// remembered, so a stronger copy can still be forwarded.
    LinkGated,
    /// Already seen.
    Duplicate,
    Declined(Declined),
    /// Queued to go no earlier than `due_ms`.
    Scheduled {
        due_ms: u64,
    },
    /// The delay queue was full, so the forward was dropped.
    QueueFull,
}

/// Tallies of every verdict, for tests and diagnostics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counters {
    pub foreign: u32,
    pub malformed: u32,
    pub remembered: u32,
    pub link_gated: u32,
    pub duplicates: u32,
    pub declined: u32,
    pub scheduled: u32,
    pub queue_full: u32,
}

impl Counters {
    fn note(&mut self, verdict: Verdict) {
        let counter = match verdict {
            Verdict::Foreign => &mut self.foreign,
            Verdict::Malformed => &mut self.malformed,
            Verdict::Remembered => &mut self.remembered,
            Verdict::LinkGated => &mut self.link_gated,
            Verdict::Duplicate => &mut self.duplicates,
            Verdict::Declined(_) => &mut self.declined,
            Verdict::Scheduled { .. } => &mut self.scheduled,
            Verdict::QueueFull => &mut self.queue_full,
        };
        *counter = counter.wrapping_add(1);
    }
}

/// When a forward may go.
enum Delay {
    /// A fixed number of milliseconds.
    Fixed(u32),
    /// The baseline's randomized window: up to five times `factor`
    /// thousandths of the forwarded frame's airtime.
    Window { factor_milli: u32 },
}

/// A forward the decision produced.
struct Forward {
    frame: Frame,
    priority: u8,
    delay: Delay,
}

/// A MeshCore v1 forwarder for one device.
///
/// Its only identity input is the device's public key, whose prefixes
/// are this node's path entries. Nothing here signs, decrypts, or
/// originates a packet: every frame it emits is a received packet with
/// its path advanced.
pub struct Forwarder<const QUEUE: usize> {
    public_key: [u8; 32],
    settings: Settings,
    seen: SeenTable,
    queue: DelayQueue<QUEUE>,
    counters: Counters,
}

impl<const QUEUE: usize> Forwarder<QUEUE> {
    pub fn new(public_key: [u8; 32], settings: Settings) -> Self {
        Self {
            public_key,
            settings,
            seen: SeenTable::new(),
            queue: DelayQueue::new(),
            counters: Counters::default(),
        }
    }

    /// Replace the settings. What has been seen and what is queued stay.
    pub fn set_settings(&mut self, settings: Settings) {
        self.settings = settings;
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Forget everything seen and everything queued.
    pub fn reset(&mut self) {
        self.seen.clear();
        self.queue.clear();
    }

    pub fn counters(&self) -> Counters {
        self.counters
    }

    /// When the next queued forward falls due, or `None` with nothing
    /// queued.
    pub fn next_due_ms(&self) -> Option<u64> {
        self.queue.next_due_ms()
    }

    /// The next forward to transmit at `now_ms`, if one is due.
    pub fn pop_due(&mut self, now_ms: u64) -> Option<Frame> {
        self.queue.pop_due(now_ms)
    }

    /// The priority `pop_due` would send at `now_ms`.
    #[cfg(test)]
    pub fn due_priority(&self, now_ms: u64) -> Option<u8> {
        self.queue.due_priority(now_ms)
    }

    /// Forwards waiting in the queue.
    pub fn queued(&self) -> usize {
        self.queue.len()
    }

    /// Take in one frame.
    pub fn receive(
        &mut self,
        frame: &[u8],
        heard: Heard,
        now_ms: u64,
        env: &mut impl Environment,
    ) -> Verdict {
        let verdict = self.process(frame, heard, now_ms, env);
        self.counters.note(verdict);
        verdict
    }

    fn process(
        &mut self,
        frame: &[u8],
        heard: Heard,
        now_ms: u64,
        env: &mut impl Environment,
    ) -> Verdict {
        let packet = match Packet::parse(frame) {
            Ok(packet) => packet,
            Err(ParseError::Empty | ParseError::Version) => return Verdict::Foreign,
            Err(_) => return Verdict::Malformed,
        };
        if heard == Heard::LocalTx {
            // The antenna beside this client already sent it; a repeater
            // that hears a frame it has sent ignores the repeats. A host
            // sending the same frame again takes no second slot.
            let hash = packet.hash();
            if !self.seen.contains(&hash) {
                self.seen.insert(hash);
            }
            return Verdict::Remembered;
        }
        let forward = match self.decide(&packet, heard) {
            Ok(forward) => forward,
            Err(verdict) => return verdict,
        };
        let delay_ms = match forward.delay {
            Delay::Fixed(ms) => ms,
            Delay::Window { factor_milli } => {
                retransmit_delay_ms(&forward.frame, factor_milli, env)
            }
        };
        let due_ms = now_ms.saturating_add(u64::from(delay_ms));
        match self.queue.push(due_ms, forward.priority, forward.frame) {
            Ok(()) => Verdict::Scheduled { due_ms },
            Err(_) => Verdict::QueueFull,
        }
    }

    /// The baseline's receive path, for a packet heard from a neighbor.
    fn decide(&mut self, packet: &Packet<'_>, heard: Heard) -> Result<Forward, Verdict> {
        let route = packet.route_type();
        let kind = packet.payload_type();
        if !route.is_flood() {
            if kind == payload_type::TRACE {
                return self.trace(packet, heard);
            }
            if kind == payload_type::CONTROL
                && packet
                    .payload()
                    .first()
                    .is_some_and(|first| first & 0x80 != 0)
            {
                return Err(Verdict::Declined(Declined::LocalControl));
            }
            if packet.path_hash_count() > 0 {
                return self.direct(packet);
            }
        } else if let Heard::Air { rssi, snr } = heard
            && !self.settings.link_passes(rssi, snr)
        {
            return Err(Verdict::LinkGated);
        }
        self.by_payload(packet)
    }

    /// A direct TRACE: its path holds the SNR of each hop so far, and the
    /// route is a list of hashes at the end of the payload.
    fn trace(&mut self, packet: &Packet<'_>, heard: Heard) -> Result<Forward, Verdict> {
        // A path-length octet below 63 is one-octet entries with room for
        // another. The baseline also takes 63 and increments it into the
        // hash-size bits, sending a trace that claims two-octet entries
        // and an empty path; that is a defect, not something to match.
        if usize::from(packet.path_len()) >= MAX_PATH_COUNT {
            return Err(Verdict::Declined(Declined::PathFull));
        }
        // Tag, authentication code, flags.
        let Some((head, route)) = packet.payload().split_at_checked(9) else {
            return Err(Verdict::Declined(Declined::Incomplete));
        };
        let hash_shift = head[8] & 0x03;
        let hash_size = 1usize << hash_shift;
        let offset = usize::from(packet.path_len()) << hash_shift;
        if offset >= route.len() {
            return Err(Verdict::Declined(Declined::TraceComplete));
        }
        // An entry cut short by the end of the payload names nobody.
        let next_hop = route.get(offset..offset + hash_size);
        if next_hop != Some(&self.public_key[..hash_size]) {
            return Err(Verdict::Declined(Declined::NotNextHop));
        }
        self.first_sighting(packet.hash())?;
        let snr = match heard {
            Heard::Air { snr, .. } => {
                snr.as_quarter_db_steps()
                    .clamp(i16::from(i8::MIN), i16::from(i8::MAX)) as i8
            }
            _ => UNMEASURED_TRACE_SNR,
        };
        let mut path: Vec<u8, MAX_PATH_LEN> = Vec::from_slice(packet.path()).expect("under 64");
        path.push(snr as u8).expect("room checked");
        let frame = packet::encode(
            packet.header(),
            packet.transport_codes(),
            packet.path_len() + 1,
            &path,
            packet.payload(),
        )
        .expect("a trace only grows into room it had");
        Ok(Forward {
            frame,
            priority: TRACE_PRIORITY,
            delay: Delay::Window {
                factor_milli: DIRECT_DELAY_FACTOR_MILLI,
            },
        })
    }

    /// A direct packet with a route ahead of it.
    fn direct(&mut self, packet: &Packet<'_>) -> Result<Forward, Verdict> {
        let size = packet.path_hash_size();
        let (next_hop, onward) = packet.path().split_at(size);
        if next_hop != &self.public_key[..size] {
            return Err(Verdict::Declined(Declined::NotNextHop));
        }
        let onward_len = packet.path_len() - 1;
        let payload = packet.payload();
        match packet.payload_type() {
            // A multipart acknowledgment goes on as the plain one it
            // carries, spaced by how many parts are still to come.
            payload_type::MULTIPART => {
                let Some((&first, ack)) = payload.split_first() else {
                    return Err(Verdict::Declined(Declined::Incomplete));
                };
                if first & 0x0F != payload_type::ACK || payload.len() < 5 {
                    return Err(Verdict::Declined(Declined::PayloadType));
                }
                self.first_sighting(packet::packet_hash(
                    payload_type::MULTIPART,
                    packet.path_len(),
                    ack,
                ))?;
                let remaining = u32::from(first >> 4);
                Ok(Forward {
                    frame: acknowledgment(onward_len, onward, ack),
                    priority: 0,
                    delay: Delay::Fixed((remaining + 1) * MULTIPART_ACK_SPACING_MS),
                })
            }
            // Rebuilt rather than relayed, which drops any transport
            // codes, and sent at once.
            payload_type::ACK => {
                self.first_sighting(packet.hash())?;
                Ok(Forward {
                    frame: acknowledgment(onward_len, onward, payload),
                    priority: 0,
                    delay: Delay::Fixed(0),
                })
            }
            _ => {
                self.first_sighting(packet.hash())?;
                let frame = packet::encode(
                    packet.header(),
                    packet.transport_codes(),
                    onward_len,
                    onward,
                    payload,
                )
                .expect("a shorter path still fits");
                Ok(Forward {
                    frame,
                    priority: 0,
                    delay: Delay::Window {
                        factor_milli: DIRECT_DELAY_FACTOR_MILLI,
                    },
                })
            }
        }
    }

    /// Floods and zero-hop direct packets: what each payload type
    /// requires, then the flood rules.
    fn by_payload(&mut self, packet: &Packet<'_>) -> Result<Forward, Verdict> {
        let payload = packet.payload();
        let len = payload.len();
        let complete = match packet.payload_type() {
            payload_type::ACK => len >= 4,
            // Destination and source hashes, then a two-octet MAC, then
            // at least one octet of ciphertext.
            payload_type::PATH
            | payload_type::REQ
            | payload_type::RESPONSE
            | payload_type::TXT_MSG => len >= 5,
            // Destination hash, the sender's key, the MAC, ciphertext.
            payload_type::ANON_REQ => len >= 36,
            payload_type::GRP_DATA | payload_type::GRP_TXT => len >= 4,
            // Key, timestamp, signature.
            payload_type::ADVERT => len >= 100,
            // Remembered when direct, never flooded.
            payload_type::RAW_CUSTOM => {
                if packet.route_type().is_flood() {
                    return Err(Verdict::Declined(Declined::PayloadType));
                }
                self.first_sighting(packet.hash())?;
                return Err(Verdict::Declined(Declined::ZeroHop));
            }
            // A multipart acknowledgment is remembered under the identity
            // of the acknowledgment it carries, and never flooded.
            payload_type::MULTIPART => {
                if len < 5 || payload[0] & 0x0F != payload_type::ACK {
                    return Err(Verdict::Declined(Declined::PayloadType));
                }
                self.first_sighting(packet::packet_hash(
                    payload_type::MULTIPART,
                    packet.path_len(),
                    &payload[1..],
                ))?;
                return Err(Verdict::Declined(Declined::PayloadType));
            }
            // TRACE and CONTROL floods, and every unassigned type.
            _ => return Err(Verdict::Declined(Declined::PayloadType)),
        };
        if !complete {
            return Err(Verdict::Declined(Declined::Incomplete));
        }
        self.first_sighting(packet.hash())?;
        if packet.payload_type() == payload_type::ADVERT && !advert_signature_valid(payload) {
            return Err(Verdict::Declined(Declined::ForgedAdvert));
        }
        if !packet.route_type().is_flood() {
            return Err(Verdict::Declined(Declined::ZeroHop));
        }
        self.flood(packet)
    }

    /// A flood that has passed its payload checks: limits, scope, and
    /// this device's path entry.
    fn flood(&self, packet: &Packet<'_>) -> Result<Forward, Verdict> {
        let size = packet.path_hash_size();
        let count = packet.path_hash_count();
        // The count is six bits wide. The baseline checks only the octet
        // capacity, so at 63 one-octet entries it carries the count into
        // the hash-size bits and sends a flood that claims an empty path
        // of two-octet entries; that is a defect, not something to match.
        if (count + 1) * size > MAX_PATH_LEN || count + 1 > MAX_PATH_COUNT {
            return Err(Verdict::Declined(Declined::PathFull));
        }
        if count >= FLOOD_MAX
            || (packet.route_type() == RouteType::Flood && count >= FLOOD_MAX_UNSCOPED)
            || (packet.payload_type() == payload_type::ADVERT && count >= FLOOD_MAX_ADVERT)
        {
            return Err(Verdict::Declined(Declined::HopLimit));
        }
        let kind = packet.payload_type();
        let payload = packet.payload();
        let admitted = self
            .settings
            .region_policy
            .admits(packet.first_transport_code(), |code| {
                self.settings
                    .regions
                    .iter()
                    .any(|key| key.code(kind, payload) == code)
            });
        if !admitted {
            return Err(Verdict::Declined(Declined::Region));
        }
        let mut path: Vec<u8, MAX_PATH_LEN> = Vec::from_slice(packet.path()).expect("parsed");
        path.extend_from_slice(&self.public_key[..size])
            .expect("room checked");
        let frame = packet::encode(
            packet.header(),
            packet.transport_codes(),
            packet.path_len() + 1,
            &path,
            payload,
        )
        .expect("a path within its limit fits");
        Ok(Forward {
            frame,
            // Nearer sources first.
            priority: (count + 1) as u8,
            delay: Delay::Window {
                factor_milli: FLOOD_DELAY_FACTOR_MILLI,
            },
        })
    }

    /// Record `hash`, or report the packet as a duplicate.
    fn first_sighting(&mut self, hash: PacketHash) -> Result<(), Verdict> {
        if self.seen.contains(&hash) {
            return Err(Verdict::Duplicate);
        }
        self.seen.insert(hash);
        Ok(())
    }
}

/// The plain direct acknowledgment the baseline builds to relay one.
fn acknowledgment(path_len: u8, path: &[u8], payload: &[u8]) -> Frame {
    packet::encode(
        packet::header(payload_type::ACK, RouteType::Direct),
        None,
        path_len,
        path,
        payload,
    )
    .expect("no larger than the packet it relays")
}

/// The baseline's retransmit window for a forwarded frame: a uniform
/// draw from zero to five times `factor_milli` thousandths of the
/// frame's airtime. The airtime counts the header, path length, path,
/// and payload, and leaves out transport codes, as the baseline does.
fn retransmit_delay_ms(frame: &[u8], factor_milli: u32, env: &mut impl Environment) -> u32 {
    let packet = Packet::parse(frame).expect("built by this engine");
    let len = packet.path().len() + packet.payload().len() + 2;
    let t = u64::from(env.airtime_ms(len)) * u64::from(factor_milli) / 1_000;
    let span = 5 * t + 1;
    // Fixed-point scaling rather than a modulus: uniform over the whole
    // span with no bias at the top.
    ((u64::from(env.random_u32()) * span) >> 32) as u32
}

/// Whether an advertisement's signature verifies.
///
/// The signature covers the key, the timestamp, and at most the first 32
/// octets of the application data; anything after those is unsigned, as
/// the baseline treats it. Verification is strict: a small-order key or
/// point the baseline's verifier would accept is declined here, which
/// only a crafted advertisement can tell apart.
fn advert_signature_valid(payload: &[u8]) -> bool {
    const MAX_SIGNED_APP_DATA: usize = 32;
    let (key, rest) = payload.split_at(32);
    let (timestamp, rest) = rest.split_at(4);
    let (signature, app_data) = rest.split_at(64);
    let app_data = &app_data[..app_data.len().min(MAX_SIGNED_APP_DATA)];
    let mut message: Vec<u8, { 32 + 4 + MAX_SIGNED_APP_DATA }> = Vec::new();
    message.extend_from_slice(key).expect("fits");
    message.extend_from_slice(timestamp).expect("fits");
    message.extend_from_slice(app_data).expect("fits");
    umsh_crypto::software::verify_ed25519_signature(
        &umsh_core::PublicKey(key.try_into().expect("32 octets")),
        &message,
        signature.try_into().expect("64 octets"),
    )
}
