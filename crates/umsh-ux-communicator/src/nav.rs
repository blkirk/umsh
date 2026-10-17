//! Moving between screens, and where focus goes when you do.
//!
//! Each screen remembers its own focus, so Back returns to what you left.
//! Opening and leaving screens happen here and nowhere else, whether a
//! press or a key asked for it.

use buoyant::focus::{FocusKey, FocusState};

use buoyant::view::EditBuffer;
use heapless::String;

use crate::model::{
    ChatId, ChatSummary, Details, Invite, Member, MessageId, PeerDetail, PeerEntry, PeerId, Preset,
    RadioSettings, Reaction, Section, State,
};

/// What a joined channel is to be called here, as it is edited.
pub type JoinName = EditBuffer<String<32>>;
/// An address or a peer invitation, as it is typed into Add peer.
pub type PeerText = EditBuffer<String<64>>;

/// The names of focusable elements. The high byte says what kind of element
/// a key names, so keys of different kinds never collide.
pub mod keys {
    use super::{ChatId, FocusKey, MessageId, PeerId, Preset, Reaction, Section};

    const KIND: u32 = 0xFF00_0000;
    const RAIL: u32 = 0x0100_0000;
    const CHAT: u32 = 0x0200_0000;
    const MESSAGE: u32 = 0x0300_0000;
    const ACTION: u32 = 0x0500_0000;
    const PEER: u32 = 0x0900_0000;
    const DEVICE: u32 = 0x0D00_0000;
    const PRESET: u32 = 0x0E00_0010;

    /// A conversation's composer. Only one conversation is shown at a time.
    pub const COMPOSER: FocusKey = FocusKey(0x0400_0000);
    /// The actions menu's Reply and Details.
    pub const REPLY: FocusKey = FocusKey(ACTION | 0x100);
    pub const DETAILS: FocusKey = FocusKey(ACTION | 0x101);
    /// The actions menu's View member, on a member's message.
    pub const VIEW_MEMBER: FocusKey = FocusKey(ACTION | 0x102);
    /// The actions menu's Join, on a message carrying an invitation.
    pub const JOIN: FocusKey = FocusKey(ACTION | 0x103);
    /// What the details screen shows, which the wheel scrolls through.
    pub const DETAIL_LIST: FocusKey = FocusKey(0x0600_0000);
    /// The member screen's Request identity, Message, and Save peer.
    pub const REQUEST_IDENTITY: FocusKey = FocusKey(0x0700_0000);
    pub const MESSAGE_MEMBER: FocusKey = FocusKey(0x0700_0001);
    pub const SAVE_PEER: FocusKey = FocusKey(0x0700_0002);
    /// The Peers list's Add peer, which heads it.
    pub const ADD_PEER: FocusKey = FocusKey(0x0A00_0000);
    /// A saved peer's screen: its actions and readouts, in order, and
    /// Forget's confirmation.
    pub const PEER_MESSAGE: FocusKey = FocusKey(0x0C00_0000);
    pub const PEER_ROUTE: FocusKey = FocusKey(0x0C00_0001);
    pub const PEER_HEARD: FocusKey = FocusKey(0x0C00_0002);
    pub const PEER_SECRECY: FocusKey = FocusKey(0x0C00_0003);
    pub const PEER_PING: FocusKey = FocusKey(0x0C00_0004);
    pub const PEER_FORGET: FocusKey = FocusKey(0x0C00_0005);
    pub const FORGET_CONFIRM: FocusKey = FocusKey(0x0C00_0006);
    pub const FORGET_CANCEL: FocusKey = FocusKey(0x0C00_0007);
    /// Add peer's field, and its Message.
    pub const PEER_FIELD: FocusKey = FocusKey(0x0B00_0000);
    pub const MESSAGE_PEER: FocusKey = FocusKey(0x0B00_0001);
    /// The join screen's Name and Join channel.
    pub const JOIN_NAME: FocusKey = FocusKey(0x0800_0000);
    pub const JOIN_CHANNEL: FocusKey = FocusKey(0x0800_0001);
    /// The Device list's categories, in order.
    pub const IDENTITY: FocusKey = FocusKey(DEVICE);
    pub const CHANNELS: FocusKey = FocusKey(DEVICE | 1);
    pub const RADIO: FocusKey = FocusKey(DEVICE | 2);
    pub const ALERTS: FocusKey = FocusKey(DEVICE | 3);
    pub const DISPLAY: FocusKey = FocusKey(DEVICE | 4);
    pub const PHONE_LINK: FocusKey = FocusKey(DEVICE | 5);
    pub const MAINTENANCE: FocusKey = FocusKey(DEVICE | 6);
    /// The Radio screen's settings and readout, in order.
    pub const REGION: FocusKey = FocusKey(0x0E00_0000);
    pub const PRESET_ROW: FocusKey = FocusKey(0x0E00_0001);
    pub const FORWARDING: FocusKey = FocusKey(0x0E00_0002);
    pub const SHARE_LOCATION: FocusKey = FocusKey(0x0E00_0003);
    pub const AIRTIME: FocusKey = FocusKey(0x0E00_0004);

    /// A reaction in the actions menu's palette.
    #[must_use]
    pub const fn reaction(reaction: Reaction) -> FocusKey {
        FocusKey(ACTION | reaction as u32)
    }

    #[must_use]
    pub const fn rail(section: Section) -> FocusKey {
        FocusKey(RAIL | section as u32)
    }

    /// A preset among Preset's choices.
    #[must_use]
    pub const fn preset(preset: Preset) -> FocusKey {
        FocusKey(PRESET | preset as u32)
    }

    /// Whether `key` names one of the Device list's categories.
    #[must_use]
    pub const fn is_category(key: FocusKey) -> bool {
        key.0 & KIND == DEVICE
    }

    /// A chat's row in the Chats list.
    #[must_use]
    pub const fn chat(chat: ChatId) -> FocusKey {
        FocusKey(CHAT | (chat & !KIND))
    }

    /// A peer's row in the Peers list.
    #[must_use]
    pub const fn peer(peer: PeerId) -> FocusKey {
        FocusKey(PEER | (peer & !KIND))
    }

    /// The peer whose row `key` names.
    #[must_use]
    pub const fn peer_of(key: FocusKey) -> Option<PeerId> {
        if key.0 & KIND == PEER {
            Some(key.0 & !KIND)
        } else {
            None
        }
    }

    #[must_use]
    pub const fn message(message: MessageId) -> FocusKey {
        FocusKey(MESSAGE | (message & !KIND))
    }

    /// The section whose rail item `key` names.
    #[must_use]
    pub const fn section(key: FocusKey) -> Option<Section> {
        if key.0 & KIND != RAIL {
            return None;
        }
        match key.0 & !KIND {
            0 => Some(Section::Chats),
            1 => Some(Section::Peers),
            2 => Some(Section::Device),
            _ => None,
        }
    }

    /// The chat whose row `key` names.
    #[must_use]
    pub const fn chat_of(key: FocusKey) -> Option<ChatId> {
        if key.0 & KIND == CHAT {
            Some(key.0 & !KIND)
        } else {
            None
        }
    }

    /// Whether `key` names a message.
    #[must_use]
    pub const fn is_message(key: FocusKey) -> bool {
        key.0 & KIND == MESSAGE
    }
}

/// A screen on the navigation stack.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Screen {
    Chats {
        focus: FocusState,
        /// The row to return to from the rail.
        row: Option<FocusKey>,
    },
    Conversation {
        chat: ChatId,
        focus: FocusState,
        /// The message whose actions menu is open.
        actions: Option<MessageId>,
        /// The message the draft replies to.
        reply: Option<MessageId>,
    },
    /// How a message reached this device.
    Details {
        message: MessageId,
        details: Details,
        focus: FocusState,
    },
    /// The member who sent a channel message.
    Member {
        message: MessageId,
        member: Member,
        /// Their identity has been asked for.
        requested: bool,
        focus: FocusState,
    },
    /// A saved peer.
    Peer {
        peer: PeerId,
        detail: PeerDetail,
        /// Forget is asking to be confirmed.
        confirm: bool,
        focus: FocusState,
    },
    /// The radio's settings.
    Radio {
        settings: RadioSettings,
        /// Preset's choices are open.
        picker: bool,
        focus: FocusState,
    },
    /// A peer's address or invitation, being typed.
    AddPeer { text: PeerText, focus: FocusState },
    /// A channel an invitation offers, before it is joined.
    Join {
        invite: Invite,
        /// What the channel is to be called here.
        name: JoinName,
        focus: FocusState,
    },
    /// The chats that got messages while the panel was dark, which the first
    /// of them woke it to.
    Wake { focus: FocusState },
}

impl Screen {
    /// The Chats screen, its list focused.
    #[must_use]
    pub fn chats() -> Self {
        Self::Chats {
            focus: FocusState::new(),
            row: None,
        }
    }

    #[must_use]
    pub fn focus(&self) -> &FocusState {
        match self {
            Self::Chats { focus, .. }
            | Self::Conversation { focus, .. }
            | Self::Details { focus, .. }
            | Self::Member { focus, .. }
            | Self::Join { focus, .. }
            | Self::AddPeer { focus, .. }
            | Self::Peer { focus, .. }
            | Self::Radio { focus, .. }
            | Self::Wake { focus } => focus,
        }
    }

    pub fn focus_mut(&mut self) -> &mut FocusState {
        match self {
            Self::Chats { focus, .. }
            | Self::Conversation { focus, .. }
            | Self::Details { focus, .. }
            | Self::Member { focus, .. }
            | Self::Join { focus, .. }
            | Self::AddPeer { focus, .. }
            | Self::Peer { focus, .. }
            | Self::Radio { focus, .. }
            | Self::Wake { focus } => focus,
        }
    }
}

impl State {
    /// The screen shown.
    ///
    /// # Panics
    ///
    /// When there is no screen, which the navigation here never allows.
    #[must_use]
    pub fn screen(&self) -> &Screen {
        self.screens.last().expect("the Chats screen is never left")
    }

    /// The focus of the screen shown, where the app keeps it.
    ///
    /// # Panics
    ///
    /// When there is no screen, which the navigation here never allows.
    pub fn focus(&mut self) -> &mut FocusState {
        self.screens
            .last_mut()
            .expect("the Chats screen is never left")
            .focus_mut()
    }

    /// Focuses the first chat in the list, as when the Chats screen first
    /// shows.
    pub fn focus_list(&mut self) {
        let row = self.list_row();
        if let Some(row) = row {
            self.focus().request(row);
        }
    }

    /// The row to focus on returning to the list: the one left, or the first.
    fn list_row(&self) -> Option<FocusKey> {
        let remembered = match self.screens.first() {
            Some(Screen::Chats { row, .. }) => *row,
            _ => None,
        };
        remembered
            .filter(|row| self.lists(*row))
            .or_else(|| match self.section {
                Section::Chats => self.chats.first().map(|c| keys::chat(c.id)),
                Section::Peers => Some(keys::ADD_PEER),
                Section::Device => Some(keys::IDENTITY),
            })
    }

    /// Whether `row` is a row of the list shown.
    fn lists(&self, row: FocusKey) -> bool {
        match self.section {
            Section::Chats => keys::chat_of(row).is_some_and(|chat| self.chat(chat).is_some()),
            Section::Peers => {
                row == keys::ADD_PEER
                    || keys::peer_of(row).is_some_and(|peer| self.peer(peer).is_some())
            }
            Section::Device => keys::is_category(row),
        }
    }

    /// Moves from the list to the rail, remembering the row left.
    pub fn focus_rail(&mut self) {
        let section = self.section;
        let left = self
            .screen()
            .focus()
            .current()
            .filter(|key| self.lists(*key));
        if let Some(Screen::Chats { focus, row }) = self.screens.last_mut() {
            *row = left;
            focus.request(keys::rail(section));
        }
    }

    /// Shows `section` and moves from the rail into its list.
    pub fn enter_section(&mut self, section: Section) {
        self.section = section;
        self.focus_list();
    }

    /// Opens the conversation of `chat`, with its composer focused.
    pub fn open_chat(&mut self, chat: ChatId) {
        if self.chat(chat).is_none() || self.screens.is_full() {
            return;
        }
        self.load_conversation(chat);
        if let Some(summary) = self.chats.iter_mut().find(|c| c.id == chat) {
            summary.unread = false;
        }
        _ = self.screens.push(Screen::Conversation {
            chat,
            focus: FocusState::requesting(keys::COMPOSER),
            actions: None,
            reply: None,
        });
    }

    /// Opens the actions menu of `message` in the conversation shown, with
    /// Reply focused.
    pub fn open_actions(&mut self, message: MessageId) {
        // An invitation offers joining first.
        let first = if self.invite(message).is_some() {
            keys::JOIN
        } else {
            keys::REPLY
        };
        if let Some(Screen::Conversation { actions, focus, .. }) = self.screens.last_mut() {
            *actions = Some(message);
            focus.request(first);
        }
    }

    /// The invitation `message` in the open chat carries.
    fn invite(&self, message: MessageId) -> Option<&Invite> {
        self.conversation
            .as_ref()?
            .messages
            .iter()
            .find(|m| m.id == message)?
            .invite
            .as_ref()
    }

    /// Shows the channel the message whose actions menu is open invites to,
    /// before it is joined.
    pub fn join_from_actions(&mut self) {
        let Some(message) = self.close_actions() else {
            return;
        };
        let Some(invite) = self.invite(message).cloned() else {
            return;
        };
        let name = JoinName::with_text(String::try_from(invite.name.as_str()).unwrap_or_default());
        _ = self.screens.push(Screen::Join {
            invite,
            name,
            focus: FocusState::requesting(keys::JOIN_CHANNEL),
        });
    }

    /// The name being given to the channel on the join screen.
    pub fn join_name_mut(&mut self) -> Option<&mut JoinName> {
        match self.screens.last_mut() {
            Some(Screen::Join { name, .. }) => Some(name),
            _ => None,
        }
    }

    /// Joins the channel on the join screen, by the name given it, or its
    /// own when none is, and opens it.
    pub fn join(&mut self) {
        let Some(Screen::Join { invite, name, .. }) = self.screens.last() else {
            return;
        };
        let given = name.text().trim();
        let given = if given.is_empty() {
            invite.name.as_str()
        } else {
            given
        };
        if let Some(summary) = self.node.join(invite, given) {
            let chat = self.add_chat(summary);
            self.go_to_chat(chat);
        }
    }

    /// Shows the saved peer `peer`, Message focused.
    pub fn open_peer(&mut self, peer: PeerId) {
        if self.peer(peer).is_none() {
            return;
        }
        let Some(detail) = self.node.peer_detail(peer) else {
            return;
        };
        _ = self.screens.push(Screen::Peer {
            peer,
            detail,
            confirm: false,
            focus: FocusState::requesting(keys::PEER_MESSAGE),
        });
    }

    /// The saved peer shown.
    fn shown_peer(&self) -> Option<PeerId> {
        match self.screens.last() {
            Some(Screen::Peer { peer, .. }) => Some(*peer),
            _ => None,
        }
    }

    /// Reads the saved peer shown from the node again.
    fn refresh_peer(&mut self) {
        let node = self.node;
        if let Some(Screen::Peer { peer, detail, .. }) = self.screens.last_mut()
            && let Some(fresh) = node.peer_detail(*peer)
        {
            *detail = fresh;
        }
    }

    /// Opens the direct chat with the saved peer shown.
    pub fn message_peer(&mut self) {
        let Some(Screen::Peer { peer, detail, .. }) = self.screens.last() else {
            return;
        };
        let Some(summary) = self.peer(*peer) else {
            return;
        };
        if let Some(chat) = self
            .node
            .direct_chat(&detail.address, &summary.name, &summary.avatar)
        {
            let chat = self.add_chat(chat);
            self.go_to_chat(chat);
        }
    }

    /// Turns forward secrecy with the saved peer shown on or off.
    pub fn toggle_forward_secrecy(&mut self) {
        let Some(Screen::Peer { peer, detail, .. }) = self.screens.last() else {
            return;
        };
        self.node
            .set_forward_secrecy(*peer, !detail.forward_secrecy);
        self.refresh_peer();
    }

    /// Pings the saved peer shown.
    pub fn ping_peer(&mut self) {
        if let Some(peer) = self.shown_peer() {
            self.node.ping(peer);
            self.refresh_peer();
        }
    }

    /// Asks to confirm forgetting the saved peer shown, Cancel focused.
    pub fn ask_forget(&mut self) {
        if let Some(Screen::Peer { confirm, focus, .. }) = self.screens.last_mut() {
            *confirm = true;
            focus.request(keys::FORGET_CANCEL);
        }
    }

    /// Keeps the saved peer shown after all.
    pub fn cancel_forget(&mut self) {
        if let Some(Screen::Peer { confirm, focus, .. }) = self.screens.last_mut() {
            *confirm = false;
            focus.request(keys::PEER_FORGET);
        }
    }

    /// Forgets the saved peer shown and returns to the Peers list, at the
    /// row that took its place.
    pub fn forget_peer(&mut self) {
        let Some(peer) = self.shown_peer() else {
            return;
        };
        self.node.forget_peer(peer);
        let Some(index) = self.peers.iter().position(|p| p.id == peer) else {
            return;
        };
        self.peers.remove(index);
        self.back();
        let next = self
            .peers
            .get(index)
            .or_else(|| self.peers.last())
            .map_or(keys::ADD_PEER, |p| keys::peer(p.id));
        self.focus().request(next);
    }

    /// Shows the radio's settings, Region focused.
    pub fn open_radio(&mut self) {
        _ = self.screens.push(Screen::Radio {
            settings: self.node.radio(),
            picker: false,
            focus: FocusState::requesting(keys::REGION),
        });
    }

    /// Reads the radio's settings shown from the node again.
    fn refresh_radio(&mut self) {
        let node = self.node;
        if let Some(Screen::Radio { settings, .. }) = self.screens.last_mut() {
            *settings = node.radio();
        }
    }

    /// Opens Preset's choices, the current one focused.
    pub fn open_presets(&mut self) {
        if let Some(Screen::Radio {
            settings,
            picker,
            focus,
        }) = self.screens.last_mut()
        {
            *picker = true;
            focus.request(keys::preset(settings.preset));
        }
    }

    /// Closes Preset's choices, Preset focused.
    pub fn close_presets(&mut self) {
        if let Some(Screen::Radio { picker, focus, .. }) = self.screens.last_mut() {
            *picker = false;
            focus.request(keys::PRESET_ROW);
        }
    }

    /// Sets the radio to `preset` and closes Preset's choices.
    pub fn choose_preset(&mut self, preset: Preset) {
        self.node.set_preset(preset);
        self.refresh_radio();
        self.close_presets();
    }

    pub fn toggle_forwarding(&mut self) {
        if let Some(Screen::Radio { settings, .. }) = self.screens.last() {
            self.node.set_forwarding(!settings.forwarding);
            self.refresh_radio();
        }
    }

    pub fn toggle_share_location(&mut self) {
        if let Some(Screen::Radio { settings, .. }) = self.screens.last() {
            self.node.set_share_location(!settings.share_location);
            self.refresh_radio();
        }
    }

    /// Shows Add peer, its field focused, so typing starts the address.
    pub fn open_add_peer(&mut self) {
        _ = self.screens.push(Screen::AddPeer {
            text: PeerText::new(),
            focus: FocusState::requesting(keys::PEER_FIELD),
        });
    }

    /// The address or invitation being typed into Add peer.
    pub fn peer_text_mut(&mut self) -> Option<&mut PeerText> {
        match self.screens.last_mut() {
            Some(Screen::AddPeer { text, .. }) => Some(text),
            _ => None,
        }
    }

    /// What has been typed into Add peer names so far.
    #[must_use]
    pub fn peer_entry(&self) -> PeerEntry {
        match self.screens.last() {
            Some(Screen::AddPeer { text, .. }) => self.node.inspect_peer(text.text()),
            _ => PeerEntry::Empty,
        }
    }

    /// Opens the direct chat with the peer Add peer names, once it names one
    /// whole, called by the name its invitation gives, or by their hint.
    pub fn message_entry(&mut self) {
        let PeerEntry::Complete {
            avatar,
            hint,
            address,
            name,
        } = self.peer_entry()
        else {
            return;
        };
        let name = name.as_ref().map_or(hint.as_str(), String::as_str);
        if let Some(summary) = self.node.direct_chat(&address, name, &avatar) {
            let chat = self.add_chat(summary);
            self.go_to_chat(chat);
        }
    }

    /// Lists `summary` first, unless its chat is listed already, and returns
    /// its chat. The oldest gives way when the list is full.
    fn add_chat(&mut self, summary: ChatSummary) -> ChatId {
        let chat = summary.id;
        if self.chat(chat).is_none() {
            if self.chats.is_full() {
                self.chats.pop();
            }
            _ = self.chats.insert(0, summary);
        }
        chat
    }

    /// Closes the actions menu, focusing the message it was opened on, and
    /// returns that message.
    pub fn close_actions(&mut self) -> Option<MessageId> {
        let Some(Screen::Conversation { actions, focus, .. }) = self.screens.last_mut() else {
            return None;
        };
        let message = actions.take()?;
        focus.request(keys::message(message));
        Some(message)
    }

    /// Reacts to the message whose actions menu is open, and closes it.
    pub fn react_from_actions(&mut self, reaction: Reaction) {
        if let Some(message) = self.close_actions() {
            self.react(message, reaction);
        }
    }

    /// Starts a reply to the message whose actions menu is open, in the
    /// composer.
    pub fn reply_from_actions(&mut self) {
        let Some(message) = self.close_actions() else {
            return;
        };
        if let Some(Screen::Conversation { reply, focus, .. }) = self.screens.last_mut() {
            *reply = Some(message);
            focus.request(keys::COMPOSER);
        }
    }

    /// Drops the reply being written, keeping its text. Returns `false` when
    /// the draft is not a reply.
    pub fn drop_reply(&mut self) -> bool {
        match self.screens.last_mut() {
            Some(Screen::Conversation { reply, .. }) => reply.take().is_some(),
            _ => false,
        }
    }

    /// Shows how the message whose actions menu is open reached this device.
    pub fn details_from_actions(&mut self) {
        let Some(message) = self.close_actions() else {
            return;
        };
        let Some(chat) = self.conversation.as_ref().map(|c| c.chat) else {
            return;
        };
        let details = self.node.details(chat, message).unwrap_or_default();
        _ = self.screens.push(Screen::Details {
            message,
            details,
            focus: FocusState::requesting(keys::DETAIL_LIST),
        });
    }

    /// Shows the member who sent the message whose actions menu is open.
    pub fn view_member_from_actions(&mut self) {
        let Some(message) = self.close_actions() else {
            return;
        };
        let Some(chat) = self.conversation.as_ref().map(|c| c.chat) else {
            return;
        };
        let Some(member) = self.node.member(chat, message) else {
            return;
        };
        let focus = if member.address.is_none() {
            keys::REQUEST_IDENTITY
        } else {
            keys::MESSAGE_MEMBER
        };
        _ = self.screens.push(Screen::Member {
            message,
            member,
            requested: false,
            focus: FocusState::requesting(focus),
        });
    }

    /// Asks the channel for the identity of the member shown.
    pub fn request_identity(&mut self) {
        let chat = self.conversation.as_ref().map(|c| c.chat);
        let Some(Screen::Member {
            message, requested, ..
        }) = self.screens.last_mut()
        else {
            return;
        };
        if let Some(chat) = chat {
            self.node.request_identity(chat, *message);
            *requested = true;
        }
    }

    /// Shows the identities the node has learned since they were drawn:
    /// members' names and addresses in the conversation and on the member
    /// screen. Focus on a Request identity that is no longer offered moves
    /// to Message.
    pub fn identities_arrived(&mut self) {
        let Some(chat) = self.conversation.as_ref().map(|c| c.chat) else {
            return;
        };
        self.load_conversation(chat);
        let node = self.node;
        for screen in &mut self.screens {
            let Screen::Member {
                message,
                member,
                focus,
                ..
            } = screen
            else {
                continue;
            };
            if let Some(known) = node.member(chat, *message) {
                *member = known;
            }
            let asking = focus.current().or(focus.pending()) == Some(keys::REQUEST_IDENTITY);
            if asking && member.address.is_some() {
                focus.request(keys::MESSAGE_MEMBER);
            }
        }
    }

    /// Opens the direct chat with the member shown, made if there is none.
    /// Back from it goes to the list, as the interface holds one
    /// conversation.
    pub fn message_member(&mut self) {
        let Some(Screen::Member { member, .. }) = self.screens.last() else {
            return;
        };
        let Some(address) = member.address.as_ref() else {
            return;
        };
        let Some(summary) =
            self.node
                .direct_chat(address, &member.sender.name, &member.sender.avatar)
        else {
            return;
        };
        let chat = self.add_chat(summary);
        self.go_to_chat(chat);
    }

    /// Keeps the member shown among my peers. Focus on Save peer, which is
    /// no longer offered, moves to Message.
    pub fn save_member(&mut self) {
        let node = self.node;
        let Some(Screen::Member { member, focus, .. }) = self.screens.last_mut() else {
            return;
        };
        if member.address.is_none() || member.saved {
            return;
        }
        let saved = node.save_peer(member);
        member.saved = true;
        focus.request(keys::MESSAGE_MEMBER);
        if let Some(peer) = saved
            && self.peer(peer.id).is_none()
        {
            if self.peers.is_full() {
                self.peers.pop();
            }
            _ = self.peers.insert(0, peer);
        }
    }

    /// Shows the chats that got messages while the panel was dark, the first
    /// focused, over wherever it went dark.
    pub(crate) fn show_arrivals(&mut self) {
        if matches!(self.screen(), Screen::Wake { .. }) || self.screens.is_full() {
            return;
        }
        let first = self.arrived.first().map(|a| keys::chat(a.chat));
        _ = self.screens.push(Screen::Wake {
            focus: first.map_or_else(FocusState::new, FocusState::requesting),
        });
    }

    /// Opens `chat` from the screen the panel woke to, in place of whatever
    /// was open under it.
    pub fn open_arrival(&mut self, chat: ChatId) {
        self.arrived.clear();
        self.go_to_chat(chat);
    }

    /// Opens `chat` in place of whatever is shown. The interface holds one
    /// conversation, so whatever was open closes; Back from the chat goes to
    /// the list, at its row.
    fn go_to_chat(&mut self, chat: ChatId) {
        self.screens.truncate(1);
        self.section = Section::Chats;
        self.conversation = None;
        self.focus().request(keys::chat(chat));
        self.open_chat(chat);
    }

    /// Returns to the screen before this one, focused where it was left.
    /// Returns `false` on the Chats screen, which nothing comes before.
    pub fn back(&mut self) -> bool {
        if self.screens.len() < 2 {
            return false;
        }
        match self.screens.pop() {
            Some(Screen::Conversation { .. }) => self.conversation = None,
            Some(Screen::Wake { .. }) => self.arrived.clear(),
            _ => {}
        }
        // Focus returns where it was, unless the screen was left asking for
        // somewhere else.
        let focus = self.focus();
        if focus.pending().is_none()
            && let Some(key) = focus.current()
        {
            focus.request(key);
        }
        true
    }
}
