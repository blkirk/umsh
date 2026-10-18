//! The node behind a communicator's interface.
//!
//! The settings half is here: the device's settings through its own ULCP
//! session ([`Settings`]). The chats half arrives with the chat session,
//! and until then the node has no chats, peers, or messages, and takes
//! none.

use core::cell::RefCell;

use umsh_ulcp::profiles::VETTED;
use umsh_ulcp_admin::LocalPort;
use umsh_ux_communicator::avatar::AvatarStyle;
use umsh_ux_communicator::heapless::{String, Vec};
use umsh_ux_communicator::model::{
    ChannelEntry, ChatId, ChatSummary, Chats, Details, EventQueue, Heard, Invite, KeyboardSettings,
    MAX_HEARD, Member, Message, MessageId, Messages, Node, NodeEvent, PeerDetail, PeerEntry,
    PeerId, PeerSummary, Peers, RadioSettings, Reaction, Setting, ShareUri, Shareable,
};

use crate::settings::Settings;

/// How many events wait for the interface before they collapse into a
/// resync.
const EVENTS: usize = 16;

/// A communicator's node, over the device's own ULCP session reached
/// through `P`.
///
/// Every method borrows the node for the length of the call and no
/// longer, so the interface and the board's task can share it on one
/// executor.
pub struct CommunicatorNode<P> {
    settings: RefCell<Settings<P>>,
    events: RefCell<EventQueue<EVENTS>>,
}

impl<P: LocalPort> CommunicatorNode<P> {
    pub fn new(port: P) -> Self {
        Self {
            settings: RefCell::new(Settings::new(port)),
            events: RefCell::new(EventQueue::new()),
        }
    }

    /// Hands the device what is waiting and takes in what it sent. The
    /// board calls this after the interface has taken an input, whenever
    /// the port has something, and at [`CommunicatorNode::next_deadline`].
    pub fn pump(&self, now_ms: u64) {
        let mut events = self.events.borrow_mut();
        self.settings
            .borrow_mut()
            .pump(now_ms, &mut |event| events.push(event));
    }

    /// When [`CommunicatorNode::pump`] next has work of its own.
    pub fn next_deadline(&self) -> Option<u64> {
        self.settings.borrow().next_deadline()
    }

    /// Whether every setting has been read from the device once.
    pub fn primed(&self) -> bool {
        self.settings.borrow().primed()
    }

    /// Runs `f` with the port, such as to wait on it.
    pub fn with_port<R>(&self, f: impl FnOnce(&P) -> R) -> R {
        f(self.settings.borrow().port())
    }

    fn change(&self, setting: Setting, change: impl FnOnce(&mut Settings<P>) -> bool) {
        if !change(&mut self.settings.borrow_mut()) {
            self.events
                .borrow_mut()
                .push(NodeEvent::SettingRefused(setting));
        }
    }
}

impl<P: LocalPort> Node for CommunicatorNode<P> {
    fn next_event(&self) -> Option<NodeEvent> {
        self.events.borrow_mut().pop()
    }

    // ─── Chats, peers, and channels: none yet ────────────────────────

    fn chats(&self, _chats: &mut Chats) {}

    fn chat(&self, _chat: ChatId) -> Option<ChatSummary> {
        None
    }

    fn peers(&self, _peers: &mut Peers) {}

    fn load(&self, _chat: ChatId, _messages: &mut Messages) {}

    fn message(&self, _chat: ChatId, _message: MessageId) -> Option<Message> {
        None
    }

    fn opened(&self, _chat: ChatId) {}

    fn save_draft(&self, _chat: ChatId, _text: &str) {}

    fn send(&self, _chat: ChatId, _text: &str, _reply_to: Option<MessageId>) -> Option<Message> {
        None
    }

    fn react(&self, _chat: ChatId, _message: MessageId, _reaction: Option<Reaction>) {}

    fn details(&self, _chat: ChatId, _message: MessageId) -> Option<Details> {
        None
    }

    fn member(&self, _chat: ChatId, _message: MessageId) -> Option<Member> {
        None
    }

    fn request_identity(&self, _chat: ChatId, _message: MessageId) {}

    fn direct_chat(
        &self,
        _address: &str,
        _name: &str,
        _avatar: &AvatarStyle,
    ) -> Option<ChatSummary> {
        None
    }

    fn inspect_peer(&self, text: &str) -> PeerEntry {
        if text.trim().is_empty() {
            PeerEntry::Empty
        } else {
            PeerEntry::Invalid
        }
    }

    fn peer_detail(&self, _peer: PeerId) -> Option<PeerDetail> {
        None
    }

    fn set_forward_secrecy(&self, _peer: PeerId, _on: bool) {}

    fn ping(&self, _peer: PeerId) {}

    fn forget_peer(&self, _peer: PeerId) {}

    fn save_peer(&self, _member: &Member) -> Option<PeerSummary> {
        None
    }

    fn join(&self, _invite: &Invite, _name: &str) -> Option<ChatSummary> {
        None
    }

    fn retry(&self, _chat: ChatId, _message: MessageId) -> bool {
        false
    }

    fn inspect_channel(&self, text: &str) -> ChannelEntry {
        if text.trim().is_empty() {
            ChannelEntry::Empty
        } else {
            ChannelEntry::Invalid
        }
    }

    fn add_channel(&self, _text: &str) -> Option<ChatSummary> {
        None
    }

    fn find_heard(&self, _query: &str) -> Vec<Heard, MAX_HEARD> {
        Vec::new()
    }

    // ─── The device ──────────────────────────────────────────────────

    fn address(&self) -> String<44> {
        self.settings.borrow().address()
    }

    fn avatar(&self) -> AvatarStyle {
        self.settings.borrow().avatar()
    }

    fn device_name(&self) -> String<24> {
        self.settings.borrow().device_name()
    }

    fn set_name(&self, name: &str) {
        self.change(Setting::Name, |settings| settings.set_name(name));
    }

    fn share_uri(&self, what: Shareable) -> Option<ShareUri> {
        match what {
            Shareable::Identity => self.settings.borrow().share_uri(),
            Shareable::Channel(_) => None,
        }
    }

    fn profile_count(&self) -> usize {
        VETTED.len()
    }

    fn profile(&self, index: usize) -> Option<&'static str> {
        VETTED.get(index).map(|profile| profile.name)
    }

    fn set_profile(&self, index: usize) {
        self.change(Setting::Profile, |settings| settings.set_profile(index));
    }

    fn radio(&self) -> RadioSettings {
        self.settings.borrow().radio()
    }

    fn set_forwarding(&self, on: bool) {
        self.change(Setting::Forwarding, |settings| settings.set_forwarding(on));
    }

    fn set_share_location(&self, on: bool) {
        self.change(Setting::ShareLocation, |settings| {
            settings.set_share_location(on)
        });
    }

    fn set_transmit_power(&self, dbm: i8) {
        self.change(Setting::TransmitPower, |settings| {
            settings.set_transmit_power(dbm)
        });
    }

    fn keyboard(&self) -> KeyboardSettings {
        self.settings.borrow().keyboard()
    }

    fn set_keyboard(&self, keyboard: KeyboardSettings) {
        self.change(Setting::Keyboard, |settings| {
            settings.set_keyboard(keyboard)
        });
    }

    fn brightness(&self) -> u8 {
        self.settings.borrow().brightness()
    }

    fn set_brightness(&self, brightness: u8) {
        self.change(Setting::Brightness, |settings| {
            settings.set_brightness(brightness)
        });
    }
}
