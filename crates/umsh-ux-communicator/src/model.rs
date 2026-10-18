//! What the screens show: plain data the device fills in from its node,
//! chat store, and peripherals, kept separate from how it is drawn.

use buoyant::view::EditBuffer;
use heapless::{String, Vec};

use crate::avatar::{AvatarStyle, BuiltinChannel};
use crate::keyboard::{Aids, Keyboard};
use crate::nav::Screen;
use crate::text::{DraftText, SharedText, TextHeap};

/// The most chats the list holds at once.
pub const MAX_CHATS: usize = 12;
/// How many saved peers the Peers list holds.
pub const MAX_PEERS: usize = 12;
/// The most messages a conversation keeps; older ones are dropped.
pub const MAX_MESSAGES: usize = 16;
/// The longest message, in bytes: a UMSH text message is at most ten
/// fragments of 160 bytes.
pub const MESSAGE_LEN: usize = 1600;

/// Identifies a chat for as long as it exists, wherever it sits in the list.
pub type ChatId = u32;
/// Identifies a message within the interface.
pub type MessageId = u32;
/// An unsent message and the cursor in it, up to a message long.
pub type Draft = EditBuffer<DraftText>;

/// The always-visible strip along the top of the screen.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct StatusStrip {
    pub device_name: String<24>,
    /// Local time, already formatted, such as `14:32`.
    pub time: String<8>,
    /// Remaining charge in thirds, 0 to 3.
    pub battery_segments: u8,
    /// External power is charging the battery, or has charged it.
    pub charging: bool,
    /// A phone is connected.
    pub phone_linked: bool,
    /// GNSS is on; whether it has a fix is not shown here.
    pub gnss: bool,
    /// Optional sounds are silenced.
    pub silent: bool,
    /// The charge is low enough to say so.
    pub battery_low: bool,
}

/// A chat as the list knows it, whether or not it is open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatSummary {
    pub id: ChatId,
    pub avatar: AvatarStyle,
    pub name: String<32>,
    /// The last message.
    pub preview: String<64>,
    /// How long ago the last message arrived, such as `2m`.
    pub age: String<6>,
    pub unread: bool,
    pub muted: bool,
    pub kind: ChatKind,
    /// The unsent message, kept until it is sent or erased.
    pub draft: Draft,
}

impl ChatSummary {
    /// A chat with no messages, whose draft is kept on `heap`.
    #[must_use]
    pub fn new(id: ChatId, avatar: AvatarStyle, name: &str, heap: TextHeap) -> Self {
        let mut summary = Self {
            id,
            avatar,
            name: String::new(),
            preview: String::new(),
            age: String::new(),
            unread: false,
            muted: false,
            kind: ChatKind::Peer { hops: None },
            draft: EditBuffer::with_text(DraftText::new(heap)),
        };
        push_cut(&mut summary.name, name);
        summary
    }

    /// Shows `message` as the last one.
    pub fn preview(&mut self, message: &Message) {
        self.preview.clear();
        match (message.direction, &message.sender) {
            (Direction::Outbound, _) => push_cut(&mut self.preview, "You: "),
            (Direction::Inbound, Some(sender)) => {
                push_cut(&mut self.preview, &sender.name);
                push_cut(&mut self.preview, ": ");
            }
            (Direction::Inbound, None) => {}
        }
        match &message.invite {
            Some(invite) if message.text.is_empty() => {
                push_cut(&mut self.preview, "Channel invite: ");
                push_cut(&mut self.preview, &invite.name);
            }
            _ => push_cut(&mut self.preview, &message.text),
        }
    }
}

/// What a saved peer's screen shows beyond the Peers list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerDetail {
    pub address: String<44>,
    /// How they are reached, such as `Direct · −97 dBm`, when known.
    pub route: Option<String<24>>,
    /// When they were last heard, such as `5 min ago`.
    pub heard: String<12>,
    pub forward_secrecy: bool,
    /// The round trip of the last ping they answered, such as `1.4 s`.
    pub ping: Option<String<8>>,
}

/// A node heard nearby but not saved, as Find shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Heard {
    pub address: String<44>,
    pub avatar: AvatarStyle,
    pub name: String<32>,
    /// How far away it is, such as `Nearby` or `2 hops`.
    pub distance: String<12>,
    /// How long ago it was heard, such as `12m`.
    pub age: String<6>,
}

/// One of Find's results.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub kind: FoundKind,
    pub avatar: AvatarStyle,
    pub name: String<32>,
    /// How far away a peer or a heard node is.
    pub distance: String<12>,
    pub age: String<6>,
}

/// What a result of Find is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FoundKind {
    Chat(ChatId),
    Peer(PeerId),
    /// A node heard but not saved, at its address.
    Heard(String<44>),
}

/// The most results Find lists.
pub const MAX_FOUND: usize = 8;

/// The most heard nodes Find lists.
pub const MAX_HEARD: usize = 4;

/// Where `query` starts a word of `name`, ignoring ASCII case: the byte
/// offset of the match. An empty query matches nothing.
#[must_use]
pub fn word_match(name: &str, query: &str) -> Option<usize> {
    if query.is_empty() {
        return None;
    }
    let mut at_word = true;
    for (i, c) in name.char_indices() {
        if at_word
            && name
                .get(i..i + query.len())
                .is_some_and(|s| s.eq_ignore_ascii_case(query))
        {
            return Some(i);
        }
        at_word = !c.is_alphanumeric();
    }
    None
}

/// What the text typed into Add channel names, as far as it goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelEntry {
    /// Nothing yet.
    Empty,
    /// Neither a channel's name nor an invitation.
    Invalid,
    /// A named channel, whose key anyone can derive from its name.
    Named,
    /// The start of a private channel's invitation, and how many characters
    /// its key lacks.
    Partial { missing: u8 },
    /// A private channel's whole invitation.
    Private,
}

/// What Share shows as a code: this node, or a channel it has joined.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shareable {
    Identity,
    Channel(ChatId),
}

/// A link that shares a node or a channel, such as `umsh:n:…`.
pub type ShareUri = String<96>;

/// How the radio trades reach for speed, farthest reaching first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    LongRange,
    MediumRange,
    ShortRange,
}

impl Preset {
    pub const ALL: [Self; 3] = [Self::LongRange, Self::MediumRange, Self::ShortRange];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::LongRange => "Long range",
            Self::MediumRange => "Medium range",
            Self::ShortRange => "Short range",
        }
    }
}

/// The radio's settings, and how much of its airtime it has spent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RadioSettings {
    /// Where the radio is, which sets what it may use.
    pub region: String<24>,
    pub preset: Preset,
    /// Repeats what it hears for others.
    pub forwarding: bool,
    pub share_location: bool,
    /// The radio's transmit power, in dBm.
    pub transmit_power: i8,
    /// How much of this hour's airtime is spent, in percent.
    pub airtime: u8,
}

/// The transmit powers the radio can be set to, in dBm.
pub const TRANSMIT_POWER: core::ops::RangeInclusive<i8> = -9..=22;

/// The keyboard's settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyboardSettings {
    /// Shift arms itself at the start of a sentence.
    pub capitalize: bool,
    /// Two quick spaces after a word type a period.
    pub period: bool,
    /// The keyboard is lit while the screen is.
    pub light: bool,
}

impl KeyboardSettings {
    #[must_use]
    pub const fn aids(self) -> Aids {
        Aids {
            capitalize: self.capitalize,
            period: self.period,
        }
    }
}

/// The screen's brightness levels, from 1 to this.
pub const BRIGHTNESS_LEVELS: u8 = 10;

/// What the text typed into Add peer names, as far as it goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PeerEntry {
    /// Nothing yet.
    Empty,
    /// Not the start of an address or an invitation.
    Invalid,
    /// The start of an address, and how many characters it lacks. The avatar
    /// and hint show once the characters settle them.
    Partial {
        avatar: Option<AvatarStyle>,
        hint: String<8>,
        missing: u8,
    },
    /// A whole address, typed or in an invitation, with the name the
    /// invitation gives.
    Complete {
        avatar: AvatarStyle,
        hint: String<8>,
        address: String<44>,
        name: Option<String<32>>,
    },
}

/// Names a saved peer.
pub type PeerId = u32;

/// A saved peer, as the Peers list shows them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerSummary {
    pub id: PeerId,
    pub avatar: AvatarStyle,
    pub name: String<32>,
    /// How many hops away they are, when known; 0 is direct.
    pub hops: Option<u8>,
    /// Of four bars, how strongly they are heard, when direct.
    pub signal: Option<u8>,
    /// How long ago they were last heard, already formatted, such as `5m`.
    pub heard: String<6>,
}

/// Whom a chat is with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatKind {
    /// One peer, and how far away it is when that is known.
    Peer { hops: Option<u8> },
    /// Everyone holding the channel's key.
    Channel { access: ChannelAccess },
}

/// Who can read a channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelAccess {
    /// Those its key was given to.
    Private,
    /// Anyone who knows its name, which its key is derived from.
    Named,
    /// Anyone within earshot: EMERGENCY is sent unencrypted.
    Clear,
}

impl ChannelAccess {
    /// Whether anyone, not only those given its key, can read it.
    #[must_use]
    pub const fn is_open(self) -> bool {
        !matches!(self, Self::Private)
    }
}

impl ChatKind {
    /// Whether anyone within earshot can read what is written in the chat.
    #[must_use]
    pub const fn is_open(self) -> bool {
        match self {
            Self::Channel { access } => access.is_open(),
            Self::Peer { .. } => false,
        }
    }
}

/// Who sent a channel message, as the channel shows them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sender {
    /// The best name known for them, or their hint.
    pub name: String<24>,
    pub avatar: AvatarStyle,
}

/// A channel member as the member screen shows them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub sender: Sender,
    /// The leading digits of their address, which their messages carry.
    pub hint: String<8>,
    /// Their whole address, once their identity is known.
    pub address: Option<String<44>>,
    /// How long ago they were last heard, already formatted, such as `2m`.
    pub heard: String<6>,
    /// They are among my peers.
    pub saved: bool,
}

/// Which way a message went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Inbound,
    Outbound,
}

/// How far an outgoing message has got.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    Sending,
    Sent,
    Delivered,
    /// No acknowledgment came back: it may not have arrived.
    Unconfirmed,
}

/// A reaction to a message: one of UMSH's emote tokens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reaction {
    Heart,
    ThumbsUp,
    ThumbsDown,
    Laugh,
    Exclaim,
    Question,
}

impl Reaction {
    /// The palette, in the order the actions menu shows it.
    pub const ALL: [Self; 6] = [
        Self::Heart,
        Self::ThumbsUp,
        Self::ThumbsDown,
        Self::Laugh,
        Self::Exclaim,
        Self::Question,
    ];

    /// The emote's body on the air.
    #[must_use]
    pub const fn token(self) -> &'static str {
        match self {
            Self::Heart => "<3",
            Self::ThumbsUp => "+1",
            Self::ThumbsDown => "-1",
            Self::Laugh => "ha",
            Self::Exclaim => "!",
            Self::Question => "?",
        }
    }
}

/// Everyone who reacted to a message one way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reacted {
    pub reaction: Reaction,
    pub count: u8,
    /// One of them is me.
    pub mine: bool,
}

/// A message's reactions, in the order they first arrived. Each sender has
/// at most one.
pub type Reactions = Vec<Reacted, { Reaction::ALL.len() }>;

/// One message in a conversation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub id: MessageId,
    pub direction: Direction,
    /// Who sent an incoming channel message.
    pub sender: Option<Sender>,
    pub text: SharedText,
    pub delivery: Delivery,
    pub reactions: Reactions,
    /// The channel it invites to, when it carries an invitation. Its text is
    /// what it says besides.
    pub invite: Option<Invite>,
}

/// An invitation to a channel. Every message carries room for one, so it
/// keeps its name on the text heap and its avatar as what draws it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invite {
    pub name: SharedText,
    pub builtin: Option<BuiltinChannel>,
    pub tint: [u8; 3],
    /// Who can read it: those given the invitation, or anyone who knows
    /// the name.
    pub access: ChannelAccess,
}

impl Invite {
    #[must_use]
    pub fn avatar(&self) -> AvatarStyle {
        AvatarStyle::channel(self.builtin, &self.name, self.tint)
    }
}

impl Message {
    /// My reaction, if I have reacted.
    #[must_use]
    pub fn my_reaction(&self) -> Option<Reaction> {
        self.reactions.iter().find(|r| r.mine).map(|r| r.reaction)
    }

    /// Makes `reaction` mine, in place of any I had.
    pub fn set_my_reaction(&mut self, reaction: Option<Reaction>) {
        if let Some(index) = self.reactions.iter().position(|r| r.mine) {
            let old = &mut self.reactions[index];
            old.mine = false;
            old.count -= 1;
            if old.count == 0 {
                self.reactions.remove(index);
            }
        }
        let Some(reaction) = reaction else {
            return;
        };
        match self.reactions.iter_mut().find(|r| r.reaction == reaction) {
            Some(same) => {
                same.count = same.count.saturating_add(1);
                same.mine = true;
            }
            None => {
                _ = self.reactions.push(Reacted {
                    reaction,
                    count: 1,
                    mine: true,
                });
            }
        }
    }
}

/// How a message reached this device, for its details.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Details {
    /// When it was received or sent, already formatted, such as `14:29`.
    pub time: String<8>,
    pub hops: Option<u8>,
    /// The routers it came through, already formatted.
    pub route: String<24>,
    /// The signal's strength, in dBm.
    pub rssi: Option<i16>,
    /// Signal-to-noise, in tenths of a dB.
    pub snr: Option<i16>,
    /// Its sender's signature checked out.
    pub authenticated: bool,
}

/// The newest messages of the open chat, oldest first.
pub type Messages = Vec<Message, MAX_MESSAGES>;

/// The open chat's messages, as many as the screen keeps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conversation {
    pub chat: ChatId,
    pub messages: Messages,
}

impl Conversation {
    /// Adds `message` as the newest, dropping the oldest when full.
    pub fn push(&mut self, message: Message) {
        push_newest(&mut self.messages, message);
    }
}

/// Adds `message` to `messages` as the newest, dropping the oldest when full.
pub fn push_newest(messages: &mut Messages, message: Message) {
    if messages.is_full() {
        messages.remove(0);
    }
    _ = messages.push(message);
}

/// The node behind the interface: where every chat's messages are kept, and
/// what it knows of peers and channels. The interface holds only the open
/// chat's newest messages, and reads them from here when it opens.
pub trait Node {
    /// Fills `messages`, which is empty, with the newest messages of `chat`.
    fn load(&self, chat: ChatId, messages: &mut Messages);

    /// Keeps `text`, sent now in reply to `reply_to` if it is a reply, as the
    /// newest message of `chat`, and returns the message as kept, or `None`
    /// when there is no room for it.
    fn send(&self, chat: ChatId, text: &str, reply_to: Option<MessageId>) -> Option<Message>;

    /// Keeps `reaction` as my reaction to `message` in `chat`; `None`
    /// withdraws it.
    fn react(&self, chat: ChatId, message: MessageId, reaction: Option<Reaction>);

    /// How `message` in `chat` reached this device, when that is known.
    fn details(&self, chat: ChatId, message: MessageId) -> Option<Details>;

    /// The member who sent `message` in the channel `chat`.
    fn member(&self, chat: ChatId, message: MessageId) -> Option<Member>;

    /// Asks the channel `chat` for the identity of the member who sent
    /// `message`.
    fn request_identity(&self, chat: ChatId, message: MessageId);

    /// The direct chat with the peer at `address`, made if there is none yet,
    /// named `name` and drawn as `avatar`. `None` when there is no room for it.
    fn direct_chat(&self, address: &str, name: &str, avatar: &AvatarStyle) -> Option<ChatSummary>;

    /// What `text`, typed as an address or a peer invitation, names so far.
    fn inspect_peer(&self, text: &str) -> PeerEntry;

    /// What is known of the saved peer `peer`.
    fn peer_detail(&self, peer: PeerId) -> Option<PeerDetail>;

    /// Turns forward secrecy with `peer` on or off.
    fn set_forward_secrecy(&self, peer: PeerId, on: bool);

    /// Pings `peer`. Its answer shows in their detail.
    fn ping(&self, peer: PeerId);

    /// Forgets the saved peer `peer`.
    fn forget_peer(&self, peer: PeerId);

    /// Keeps `member`, whose identity is known, among my peers, and returns
    /// them as the Peers list shows them. `None` when there is no room.
    fn save_peer(&self, member: &Member) -> Option<PeerSummary>;

    /// Joins the channel `invite` invites to, called `name` here, and returns
    /// its chat: the one already joined, if it is. `None` when there is no
    /// room for it.
    fn join(&self, invite: &Invite, name: &str) -> Option<ChatSummary>;

    /// Sends `message` in `chat`, which is unconfirmed, again. Returns whether
    /// it is on its way.
    fn retry(&self, chat: ChatId, message: MessageId) -> bool;

    /// What `text`, typed as a channel's name or invitation, names so far.
    fn inspect_channel(&self, text: &str) -> ChannelEntry;

    /// Joins the channel `text` names or invites to, and returns its chat:
    /// the one already joined, if it is. `None` when `text` names no channel
    /// or there is no room for it.
    fn add_channel(&self, text: &str) -> Option<ChatSummary>;

    /// Nodes heard but not saved whose names have a word starting with
    /// `query`, most recently heard first.
    fn find_heard(&self, query: &str) -> Vec<Heard, MAX_HEARD>;

    /// This node's address.
    fn address(&self) -> String<44>;

    /// How this node is drawn.
    fn avatar(&self) -> AvatarStyle;

    /// Calls this node `name`, which may be empty.
    fn set_name(&self, name: &str);

    /// The regions the radio can be set for, by name.
    fn regions(&self) -> &[&'static str];

    /// Sets the radio for the region at `index` in `regions`.
    fn set_region(&self, index: usize);

    /// The link that shares `what`. `None` when it cannot be shared.
    fn share_uri(&self, what: Shareable) -> Option<ShareUri>;

    /// The radio's settings as they stand.
    fn radio(&self) -> RadioSettings;

    fn set_preset(&self, preset: Preset);

    fn set_forwarding(&self, on: bool);

    fn set_share_location(&self, on: bool);

    /// Sets the radio's transmit power, within [`TRANSMIT_POWER`].
    fn set_transmit_power(&self, dbm: i8);

    fn keyboard(&self) -> KeyboardSettings;

    fn set_keyboard(&self, settings: KeyboardSettings);

    /// The screen's brightness, from 1 to [`BRIGHTNESS_LEVELS`].
    fn brightness(&self) -> u8;

    fn set_brightness(&self, level: u8);
}

/// At or below this charge, in percent, the battery is low.
pub const BATTERY_LOW_PERCENT: u8 = 10;
/// The charge, in percent, at which a low battery stops being low.
const BATTERY_RECOVERED_PERCENT: u8 = 15;

/// The strip's lit thirds for a charge of `percent`. Each count is centered
/// on the level it depicts, and the ends are narrower: a body should look
/// neither full at 80 % nor empty at 20 %.
#[must_use]
pub const fn battery_segments(percent: u8) -> u8 {
    match percent {
        0..=16 => 0,
        17..=49 => 1,
        50..=82 => 2,
        _ => 3,
    }
}

/// How long a banner stays, in milliseconds.
pub const BANNER_MS: u64 = 4000;

/// Something shown over the screen for a moment, taking no input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Banner {
    pub notice: Notice,
    /// When it goes, on the board's clock, in milliseconds.
    pub until: u64,
}

/// What a banner says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Notice {
    /// A message arrived in another chat; the banner shows the chat as the
    /// list does.
    Message(ChatId),
    /// The charge has fallen low, to `percent`.
    BatteryLow { percent: u8 },
}

/// A chat that got a message while the panel was dark.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Arrived {
    pub chat: ChatId,
    /// The newest message, with its sender ahead of it in a channel.
    pub text: SharedText,
}

/// The top-level lists the rail switches between.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Section {
    #[default]
    Chats,
    Peers,
    Device,
}

/// Everything the interface draws from.
pub struct State {
    pub status: StatusStrip,
    pub section: Section,
    /// Newest first.
    pub chats: Vec<ChatSummary, MAX_CHATS>,
    /// Saved peers, most recently heard first.
    pub peers: Vec<PeerSummary, MAX_PEERS>,
    /// The open chat's messages, while one is open.
    pub conversation: Option<Conversation>,
    /// The screens, the one shown last. The Chats screen is always first.
    pub screens: Vec<Screen, 4>,
    pub node: &'static dyn Node,
    /// Seconds until sending is allowed again, while the airtime it may use
    /// is spent.
    pub airtime_wait: Option<u16>,
    /// A message that arrived in a chat not on screen, shown for a moment.
    pub banner: Option<Banner>,
    /// The panel is dark: nothing on it is being read.
    pub dark: bool,
    /// The chats that got messages while the panel was dark, in the order
    /// they first did, for the screen the first of them woke it to.
    pub arrived: Vec<Arrived, MAX_CHATS>,
    /// The board is turning off: shown over everything, and the end of
    /// input.
    pub ending: Option<Ending>,
    /// What keys type: the focused field's modifiers and aids.
    pub keyboard: Keyboard,
    /// The character picker is open, with this character focused.
    pub picker: Option<u8>,
    /// A brief card for a setting a key changed.
    pub glance: Option<Glance>,
}

/// A brief card, with no words, for a setting changed from the keyboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glance {
    /// The screen's brightness, now at this level.
    Brightness { level: u8, until: u64 },
    /// The keyboard light, now on or off.
    KeyboardLight { on: bool, until: u64 },
}

impl Glance {
    const fn until(self) -> u64 {
        match self {
            Self::Brightness { until, .. } | Self::KeyboardLight { until, .. } => until,
        }
    }
}

/// How long a glance stays, in milliseconds.
const GLANCE_MS: u64 = 1500;

/// What lies over the screen: the topmost of what could.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overlay {
    Ending(Ending),
    Picker(u8),
    Glance(Glance),
    Banner(Notice),
}

/// What the screen says as the board turns off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ending {
    /// Turning off as asked.
    ShuttingDown,
    /// The battery is empty: the screen shows only that, its icon lit or
    /// not as the board blinks it.
    BatteryEmpty { lit: bool },
}

impl core::fmt::Debug for State {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("State")
            .field("status", &self.status)
            .field("section", &self.section)
            .field("chats", &self.chats)
            .field("peers", &self.peers)
            .field("conversation", &self.conversation)
            .field("screens", &self.screens)
            .field("airtime_wait", &self.airtime_wait)
            .field("banner", &self.banner)
            .field("dark", &self.dark)
            .field("arrived", &self.arrived)
            .field("ending", &self.ending)
            .field("keyboard", &self.keyboard)
            .field("picker", &self.picker)
            .field("glance", &self.glance)
            .finish_non_exhaustive()
    }
}

impl State {
    /// No chats, on the Chats screen, with chats' messages kept in `node`.
    #[must_use]
    pub fn new(node: &'static dyn Node) -> Self {
        let mut screens = Vec::new();
        _ = screens.push(Screen::chats());
        Self {
            status: StatusStrip::default(),
            section: Section::default(),
            chats: Vec::new(),
            peers: Vec::new(),
            conversation: None,
            screens,
            node,
            airtime_wait: None,
            banner: None,
            dark: false,
            arrived: Vec::new(),
            ending: None,
            keyboard: Keyboard::default(),
            picker: None,
            glance: None,
        }
    }

    /// How many chats have unread messages, for the rail's badge.
    #[must_use]
    pub fn unread_chats(&self) -> usize {
        self.chats.iter().filter(|c| c.unread).count()
    }

    #[must_use]
    pub fn chat(&self, chat: ChatId) -> Option<&ChatSummary> {
        self.chats.iter().find(|c| c.id == chat)
    }

    #[must_use]
    pub fn peer(&self, peer: PeerId) -> Option<&PeerSummary> {
        self.peers.iter().find(|p| p.id == peer)
    }

    fn chat_mut(&mut self, chat: ChatId) -> Option<&mut ChatSummary> {
        self.chats.iter_mut().find(|c| c.id == chat)
    }

    /// The messages of `chat`, when it is the open chat.
    #[must_use]
    pub fn conversation(&self, chat: ChatId) -> Option<&Conversation> {
        self.conversation.as_ref().filter(|c| c.chat == chat)
    }

    /// The draft of `chat`, which its composer edits.
    pub fn draft_mut(&mut self, chat: ChatId) -> Option<&mut Draft> {
        self.chat_mut(chat).map(|c| &mut c.draft)
    }

    /// Reads the newest messages of `chat` from the node, as the open
    /// chat's.
    pub(crate) fn load_conversation(&mut self, chat: ChatId) {
        // Filled where it lies: a conversation is too large to build aside.
        let conversation = self.conversation.get_or_insert_with(|| Conversation {
            chat,
            messages: Vec::new(),
        });
        conversation.chat = chat;
        conversation.messages.clear();
        self.node.load(chat, &mut conversation.messages);
    }

    /// Sends the draft of `chat`: it becomes the newest message, and the
    /// chat moves to the top of the list. A draft stays a draft while airtime
    /// is spent, or when the node has no room for it.
    pub fn send(&mut self, chat: ChatId) {
        if self.airtime_wait.is_some() {
            return;
        }
        let Some(index) = self
            .chats
            .iter()
            .position(|c| c.id == chat && !c.draft.text().trim().is_empty())
        else {
            return;
        };
        let reply_to = match self.screens.last() {
            Some(Screen::Conversation {
                chat: shown, reply, ..
            }) if *shown == chat => *reply,
            _ => None,
        };
        let Some(message) = self
            .node
            .send(chat, self.chats[index].draft.text(), reply_to)
        else {
            return;
        };
        if let Some(Screen::Conversation { reply, .. }) = self.screens.last_mut() {
            *reply = None;
        }
        let mut summary = self.chats.remove(index);
        summary.draft.clear();
        summary.preview(&message);
        summary.age.clear();
        push_cut(&mut summary.age, "now");
        _ = self.chats.insert(0, summary);
        if let Some(conversation) = self.conversation.as_mut().filter(|c| c.chat == chat) {
            conversation.push(message);
        }
    }
}

impl State {
    /// Whether `message` in the open chat was received rather than sent.
    #[must_use]
    pub fn inbound(&self, message: MessageId) -> bool {
        self.conversation
            .as_ref()
            .and_then(|c| c.messages.iter().find(|m| m.id == message))
            .is_some_and(|m| m.direction == Direction::Inbound)
    }

    /// Reacts to `message` in the open chat with `reaction`, in place of any
    /// reaction I had; choosing the one I had withdraws it.
    pub fn react(&mut self, message: MessageId, reaction: Reaction) {
        let Some(conversation) = self.conversation.as_mut() else {
            return;
        };
        let Some(target) = conversation.messages.iter_mut().find(|m| m.id == message) else {
            return;
        };
        let mine = (target.my_reaction() != Some(reaction)).then_some(reaction);
        target.set_my_reaction(mine);
        self.node.react(conversation.chat, message, mine);
    }

    /// Sends `message` in the open chat again, if it is unconfirmed and
    /// airtime allows.
    pub fn retry(&mut self, message: MessageId) {
        if self.airtime_wait.is_some() {
            return;
        }
        let Some(conversation) = self.conversation.as_mut() else {
            return;
        };
        let Some(target) = conversation
            .messages
            .iter_mut()
            .find(|m| m.id == message && m.delivery == Delivery::Unconfirmed)
        else {
            return;
        };
        if self.node.retry(conversation.chat, message) {
            target.delivery = Delivery::Sending;
        }
    }

    /// Takes in `message`, which arrived in `chat` and is already kept by
    /// the node, at `now` on the board's clock. The chat moves to the top of
    /// the list. Returns whether the message calls for attention: it does
    /// not while its chat is being read, or when the chat is muted. One that
    /// does while the panel is dark is listed for the screen it wakes to.
    pub fn receive(&mut self, chat: ChatId, message: Message, now: u64) -> bool {
        let Some(index) = self.chats.iter().position(|c| c.id == chat) else {
            return false;
        };
        let reading = !self.dark
            && matches!(
                self.screen(),
                Screen::Conversation { chat: shown, .. } if *shown == chat
            );
        let mut summary = self.chats.remove(index);
        summary.preview(&message);
        summary.age.clear();
        push_cut(&mut summary.age, "now");
        summary.unread |= !reading;
        let muted = summary.muted;
        _ = self.chats.insert(0, summary);
        if let Some(conversation) = self.conversation.as_mut().filter(|c| c.chat == chat) {
            conversation.push(message.clone());
        }
        if reading || muted {
            return false;
        }
        if self.dark || matches!(self.screen(), Screen::Wake { .. }) {
            self.list_arrival(chat, &message);
        } else if !matches!(self.screen(), Screen::Chats { .. }) {
            // The list shows it arrive; anywhere else, a banner does.
            self.banner = Some(Banner {
                notice: Notice::Message(chat),
                until: now + BANNER_MS,
            });
        }
        true
    }

    /// Lists `message`, the newest of `chat`, for the screen the panel wakes
    /// to, and shows that screen.
    fn list_arrival(&mut self, chat: ChatId, message: &Message) {
        let text = message
            .sender
            .as_ref()
            .and_then(|sender| {
                SharedText::joined(message.text.heap(), &[&sender.name, ": ", &message.text])
            })
            .unwrap_or_else(|| message.text.clone());
        match self.arrived.iter_mut().find(|a| a.chat == chat) {
            Some(arrived) => arrived.text = text,
            None => {
                if self.arrived.is_full() {
                    self.arrived.remove(0);
                }
                _ = self.arrived.push(Arrived { chat, text });
            }
        }
        self.show_arrivals();
    }

    /// The battery as the board last read it: its charge in percent, when
    /// that is known, and whether external power is charging it. The strip
    /// shows it; falling to [`BATTERY_LOW_PERCENT`] with nothing charging
    /// turns the strip's battery amber, and a banner says so once while the
    /// panel is lit.
    pub fn battery(&mut self, percent: Option<u8>, charging: bool, now: u64) {
        self.status.battery_segments = percent.map_or(0, battery_segments);
        self.status.charging = charging;
        let was = self.status.battery_low;
        // Low until it charges or climbs clear, so a reading wavering at
        // the threshold says so once.
        let low = !charging
            && match percent {
                Some(percent) if was => percent < BATTERY_RECOVERED_PERCENT,
                Some(percent) => percent <= BATTERY_LOW_PERCENT,
                None => was,
            };
        self.status.battery_low = low;
        if let Some(percent) = percent.filter(|_| low && !was && !self.dark) {
            self.banner = Some(Banner {
                notice: Notice::BatteryLow { percent },
                until: now + BANNER_MS,
            });
        }
    }

    /// The battery is empty: the screen shows only its icon, `lit` or not.
    pub fn battery_empty(&mut self, lit: bool) {
        self.ending = Some(Ending::BatteryEmpty { lit });
    }

    /// The board is turning off as asked, and says so.
    pub fn shutting_down(&mut self) {
        self.ending = Some(Ending::ShuttingDown);
    }

    /// When the screen next changes by itself, on the board's clock.
    #[must_use]
    pub fn next_deadline(&self) -> Option<u64> {
        let banner = self.banner.map(|b| b.until);
        let glance = self.glance.map(Glance::until);
        match (banner, glance) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// Lets go of whatever has had its time by `now`.
    pub fn expire(&mut self, now: u64) {
        if self.banner.is_some_and(|b| b.until <= now) {
            self.banner = None;
        }
        if self.glance.is_some_and(|g| g.until() <= now) {
            self.glance = None;
        }
    }

    /// What lies over the screen.
    #[must_use]
    pub fn overlay(&self) -> Option<Overlay> {
        self.ending
            .map(Overlay::Ending)
            .or(self.picker.map(Overlay::Picker))
            .or(self.glance.map(Overlay::Glance))
            .or(self.banner.map(|b| Overlay::Banner(b.notice)))
    }

    /// Turns the screen's brightness by `detents` levels, and shows it:
    /// turned up, it brightens, and turned down, it dims.
    pub fn adjust_brightness(&mut self, detents: i8, now: u64) {
        let level = (i16::from(self.node.brightness()) - i16::from(detents))
            .clamp(1, i16::from(BRIGHTNESS_LEVELS)) as u8;
        self.node.set_brightness(level);
        self.glance = Some(Glance::Brightness {
            level,
            until: now + GLANCE_MS,
        });
    }

    /// Switches the keyboard light, and shows it.
    pub fn switch_keyboard_light(&mut self, now: u64) {
        self.toggle_keyboard_light();
        self.glance = Some(Glance::KeyboardLight {
            on: self.node.keyboard().light,
            until: now + GLANCE_MS,
        });
    }
}

/// Appends as much of `s` as fits.
fn push_cut<const N: usize>(into: &mut String<N>, s: &str) {
    for c in s.chars() {
        if into.push(c).is_err() {
            break;
        }
    }
}
