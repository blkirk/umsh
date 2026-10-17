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
    /// How far away the peer is, for a peer chat.
    pub hops: Option<u8>,
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
            hops: None,
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
                push_cut(&mut self.preview, sender);
                push_cut(&mut self.preview, ": ");
            }
            (Direction::Inbound, None) => {}
        }
        push_cut(&mut self.preview, &message.text);
    }
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
    Failed,
}

/// One message in a conversation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub id: MessageId,
    pub direction: Direction,
    /// Who sent an incoming channel message.
    pub sender: Option<String<24>>,
    pub text: SharedText,
    pub delivery: Delivery,
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

    /// Keeps `text`, sent now, as the newest message of `chat`, and returns
    /// the message as kept, or `None` when there is no room for it.
    fn send(&self, chat: ChatId, text: &str) -> Option<Message>;
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
}

impl core::fmt::Debug for State {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("State")
            .field("status", &self.status)
            .field("section", &self.section)
            .field("chats", &self.chats)
            .field("conversation", &self.conversation)
            .field("screens", &self.screens)
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
    /// chat moves to the top of the list. A draft the history has no room
    /// for stays a draft.
    pub fn send(&mut self, chat: ChatId) {
        let Some(index) = self
            .chats
            .iter()
            .position(|c| c.id == chat && !c.draft.text().trim().is_empty())
        else {
            return;
        };
        let Some(message) = self.history.send(chat, self.chats[index].draft.text()) else {
            return;
        };
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

/// Appends as much of `s` as fits.
fn push_cut<const N: usize>(into: &mut String<N>, s: &str) {
    for c in s.chars() {
        if into.push(c).is_err() {
            break;
        }
    }
}
