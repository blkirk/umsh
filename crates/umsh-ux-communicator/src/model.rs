//! What the screens show: plain data the device fills in from its node,
//! chat store, and peripherals, kept separate from how it is drawn.

use heapless::{String, Vec};

use crate::avatar::AvatarStyle;

/// The most chats the list holds at once.
pub const MAX_CHATS: usize = 12;

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

/// One row of the Chats list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatSummary {
    pub avatar: AvatarStyle,
    pub name: String<32>,
    /// The last message, or the draft when there is one.
    pub preview: String<64>,
    /// How long ago the last message arrived, such as `2m`.
    pub age: String<6>,
    pub unread: bool,
    pub muted: bool,
    /// The preview is an unsent draft.
    pub draft: bool,
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
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct State {
    pub status: StatusStrip,
    pub section: Section,
    pub chats: Vec<ChatSummary, MAX_CHATS>,
}

impl State {
    /// How many chats have unread messages, for the rail's badge.
    #[must_use]
    pub fn unread_chats(&self) -> usize {
        self.chats.iter().filter(|c| c.unread).count()
    }
}
