//! Moving between screens, and where focus goes when you do.
//!
//! Each screen remembers its own focus, so Back returns to what you left.
//! Opening and leaving screens happen here and nowhere else, whether a
//! press or a key asked for it.

use buoyant::focus::{FocusKey, FocusState};

use crate::model::{ChatId, Details, MessageId, Reaction, Section, State};

/// The names of focusable elements. The high byte says what kind of element
/// a key names, so keys of different kinds never collide.
pub mod keys {
    use super::{ChatId, FocusKey, MessageId, Reaction, Section};

    const KIND: u32 = 0xFF00_0000;
    const RAIL: u32 = 0x0100_0000;
    const CHAT: u32 = 0x0200_0000;
    const MESSAGE: u32 = 0x0300_0000;
    const ACTION: u32 = 0x0500_0000;

    /// A conversation's composer. Only one conversation is shown at a time.
    pub const COMPOSER: FocusKey = FocusKey(0x0400_0000);
    /// The actions menu's Reply and Details.
    pub const REPLY: FocusKey = FocusKey(ACTION | 0x100);
    pub const DETAILS: FocusKey = FocusKey(ACTION | 0x101);
    /// What the details screen shows, which the wheel scrolls through.
    pub const DETAIL_LIST: FocusKey = FocusKey(0x0600_0000);

    /// A reaction in the actions menu's palette.
    #[must_use]
    pub const fn reaction(reaction: Reaction) -> FocusKey {
        FocusKey(ACTION | reaction as u32)
    }

    #[must_use]
    pub const fn rail(section: Section) -> FocusKey {
        FocusKey(RAIL | section as u32)
    }

    /// A chat's row in the Chats list.
    #[must_use]
    pub const fn chat(chat: ChatId) -> FocusKey {
        FocusKey(CHAT | (chat & !KIND))
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
            | Self::Details { focus, .. } => focus,
        }
    }

    pub fn focus_mut(&mut self) -> &mut FocusState {
        match self {
            Self::Chats { focus, .. }
            | Self::Conversation { focus, .. }
            | Self::Details { focus, .. } => focus,
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
            .filter(|row| keys::chat_of(*row).is_some_and(|chat| self.chat(chat).is_some()))
            .or_else(|| self.chats.first().map(|c| keys::chat(c.id)))
    }

    /// Moves from the list to the rail, remembering the row left.
    pub fn focus_rail(&mut self) {
        let section = self.section;
        if let Some(Screen::Chats { focus, row }) = self.screens.last_mut() {
            *row = focus.current().filter(|key| keys::chat_of(*key).is_some());
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
        if let Some(Screen::Conversation { actions, focus, .. }) = self.screens.last_mut() {
            *actions = Some(message);
            focus.request(keys::REPLY);
        }
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
        let details = self.history.details(chat, message).unwrap_or_default();
        _ = self.screens.push(Screen::Details {
            message,
            details,
            focus: FocusState::requesting(keys::DETAIL_LIST),
        });
    }

    /// Returns to the screen before this one, focused where it was left.
    /// Returns `false` on the Chats screen, which nothing comes before.
    pub fn back(&mut self) -> bool {
        if self.screens.len() < 2 {
            return false;
        }
        if let Some(Screen::Conversation { .. }) = self.screens.pop() {
            self.conversation = None;
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
