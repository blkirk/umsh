//! What the screens show: plain data the device fills in from its node,
//! chat store, and peripherals, kept separate from how it is drawn.

use buoyant::view::EditBuffer;
use heapless::{String, Vec};

use crate::avatar::AvatarStyle;
use crate::nav::Screen;
use crate::text::{DraftText, SharedText, TextHeap};

/// The most chats the list holds at once.
pub const MAX_CHATS: usize = 12;
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
    pub charging: bool,
    /// A phone is connected.
    pub phone_linked: bool,
    /// GNSS is on; whether it has a fix is not shown here.
    pub gnss: bool,
    /// Optional sounds are silenced.
    pub silent: bool,
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
        push_cut(&mut self.preview, &message.text);
    }
}

/// Whom a chat is with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatKind {
    /// One peer, and how far away it is when that is known.
    Peer { hops: Option<u8> },
    /// Everyone holding the channel's key. An `open` channel's key is
    /// derived from its name, or it is EMERGENCY, which is sent in the clear:
    /// anyone can read it.
    Channel { open: bool },
}

impl ChatKind {
    /// Whether anyone within earshot can read what is written in the chat.
    #[must_use]
    pub const fn is_open(self) -> bool {
        matches!(self, Self::Channel { open: true })
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

/// Where every chat's messages are kept. The interface holds only the open
/// chat's newest messages, and reads them from here when it opens.
pub trait History {
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

    /// Sends `message` in `chat`, which is unconfirmed, again. Returns whether
    /// it is on its way.
    fn retry(&self, chat: ChatId, message: MessageId) -> bool;
}

/// How long a banner stays, in milliseconds.
pub const BANNER_MS: u64 = 4000;

/// A message that arrived in another chat, shown over the screen for a
/// moment. It shows the chat as the list does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Banner {
    pub chat: ChatId,
    /// When it goes, on the board's clock, in milliseconds.
    pub until: u64,
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
    /// The open chat's messages, while one is open.
    pub conversation: Option<Conversation>,
    /// The screens, the one shown last. The Chats screen is always first.
    pub screens: Vec<Screen, 4>,
    pub history: &'static dyn History,
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
}

impl core::fmt::Debug for State {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("State")
            .field("status", &self.status)
            .field("section", &self.section)
            .field("chats", &self.chats)
            .field("conversation", &self.conversation)
            .field("screens", &self.screens)
            .field("airtime_wait", &self.airtime_wait)
            .field("banner", &self.banner)
            .field("dark", &self.dark)
            .field("arrived", &self.arrived)
            .finish_non_exhaustive()
    }
}

impl State {
    /// No chats, on the Chats screen, with chats' messages kept in `history`.
    #[must_use]
    pub fn new(history: &'static dyn History) -> Self {
        let mut screens = Vec::new();
        _ = screens.push(Screen::chats());
        Self {
            status: StatusStrip::default(),
            section: Section::default(),
            chats: Vec::new(),
            conversation: None,
            screens,
            history,
            airtime_wait: None,
            banner: None,
            dark: false,
            arrived: Vec::new(),
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

    /// Reads the newest messages of `chat` from the history, as the open
    /// chat's.
    pub(crate) fn load_conversation(&mut self, chat: ChatId) {
        // Filled where it lies: a conversation is too large to build aside.
        let conversation = self.conversation.get_or_insert_with(|| Conversation {
            chat,
            messages: Vec::new(),
        });
        conversation.chat = chat;
        conversation.messages.clear();
        self.history.load(chat, &mut conversation.messages);
    }

    /// Sends the draft of `chat`: it becomes the newest message, and the
    /// chat moves to the top of the list. A draft stays a draft while airtime
    /// is spent, or when the history has no room for it.
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
            .history
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
        self.history.react(conversation.chat, message, mine);
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
        if self.history.retry(conversation.chat, message) {
            target.delivery = Delivery::Sending;
        }
    }

    /// Takes in `message`, which arrived in `chat` and is already in the
    /// history, at `now` on the board's clock. The chat moves to the top of
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
                chat,
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

    /// When the screen next changes by itself, on the board's clock.
    #[must_use]
    pub fn next_deadline(&self) -> Option<u64> {
        self.banner.map(|b| b.until)
    }

    /// Lets go of whatever has had its time by `now`.
    pub fn expire(&mut self, now: u64) {
        if self.banner.is_some_and(|b| b.until <= now) {
            self.banner = None;
        }
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
