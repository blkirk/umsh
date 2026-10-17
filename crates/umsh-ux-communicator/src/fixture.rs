//! Stand-in content until a real node backs the interface: the chats from
//! the T-LoRa Pager mockup, so a render compares directly with it.

use core::cell::{Cell, RefCell};

use crate::avatar::{AvatarStyle, BuiltinChannel, PeerShape};
use crate::model::{
    ChatId, ChatSummary, Delivery, Direction, History, MESSAGE_LEN, Message, MessageId, Messages,
    State, StatusStrip, push_newest,
};
use crate::text::{DraftText, SharedText, TextHeap};
use buoyant::view::{EditBuffer, EditText as _};
use heapless::{String, Vec};

fn s<const N: usize>(text: &str) -> String<N> {
    String::try_from(text).expect("fixture text fits")
}

struct Chat {
    avatar: AvatarStyle,
    name: &'static str,
    age: &'static str,
    hops: Option<u8>,
    unread: bool,
    muted: bool,
    draft: &'static str,
}

/// Who sent a message.
#[derive(Clone, Copy)]
enum From {
    Me,
    /// The peer of a peer chat.
    Peer,
    /// A member of a channel.
    Member(&'static str),
}

/// The messages of `chat`, oldest first.
fn messages(chat: ChatId) -> &'static [(From, &'static str)] {
    match chat {
        1 => &[
            (
                From::Peer,
                "At the north junction yet? Sam says the creek is high",
            ),
            (From::Me, "Just passed it. ETA 20 min."),
            (From::Peer, "See you at the hut"),
        ],
        2 => &[(From::Member("Sam"), "Will wait 10 min")],
        3 => &[(From::Me, "Bring the spare antenna?")],
        4 => &[(From::Member("Kit"), "Anyone near Mill Creek?")],
        5 => &[
            (
                From::Peer,
                "Hut is open and the stove works. Water from the spring is fine if you \
                 filter it. Snow starts about 200 m above the hut, so leave the trail \
                 runners and bring boots. The bunk room sleeps six; we have four, so there \
                 is room for you and Sam. Radio check at 19:00 on the usual channel.",
            ),
            (From::Peer, "Thanks!"),
        ],
        _ => &[],
    }
}

/// The fixture's message, with its text kept on `heap`, when there is room.
fn message(
    heap: TextHeap,
    chat: ChatId,
    index: usize,
    (from, text): (From, &str),
) -> Option<Message> {
    assert!(text.len() <= MESSAGE_LEN, "fixture message fits");
    Some(Message {
        id: (chat << 8) | index as MessageId,
        direction: match from {
            From::Me => Direction::Outbound,
            From::Peer | From::Member(_) => Direction::Inbound,
        },
        sender: match from {
            From::Member(name) => Some(s(name)),
            From::Me | From::Peer => None,
        },
        text: SharedText::new(heap, text)?,
        delivery: Delivery::Delivered,
    })
}

/// How many sent messages the fixture keeps, across all chats.
const SENT: usize = 8;

/// The fixture's messages, and the ones sent since it started, with their
/// text kept on a heap of the device's choosing.
pub struct Store {
    heap: TextHeap,
    sent: RefCell<Vec<(ChatId, Message), SENT>>,
    next: Cell<MessageId>,
}

impl Store {
    #[must_use]
    pub const fn new(heap: TextHeap) -> Self {
        Self {
            heap,
            sent: RefCell::new(Vec::new()),
            // Above the fixture's own messages.
            next: Cell::new(0x1_0000),
        }
    }
}

impl core::fmt::Debug for Store {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Store")
            .field("sent", &self.sent)
            .field("next", &self.next)
            .finish_non_exhaustive()
    }
}

impl History for Store {
    fn load(&self, chat: ChatId, into: &mut Messages) {
        for (index, &entry) in messages(chat).iter().enumerate() {
            if let Some(message) = message(self.heap, chat, index, entry) {
                push_newest(into, message);
            }
        }
        for (_, sent) in self.sent.borrow().iter().filter(|(c, _)| *c == chat) {
            push_newest(into, sent.clone());
        }
    }

    fn send(&self, chat: ChatId, text: &str) -> Option<Message> {
        let message = Message {
            id: self.next.get(),
            direction: Direction::Outbound,
            sender: None,
            text: SharedText::new(self.heap, text)?,
            delivery: Delivery::Sending,
        };
        self.next.set(message.id + 1);
        let mut sent = self.sent.borrow_mut();
        if sent.is_full() {
            sent.remove(0);
        }
        _ = sent.push((chat, message.clone()));
        Some(message)
    }
}

/// The mockup's chats, with their messages kept in `store`.
pub fn state(store: &'static Store) -> State {
    let mut state = State::new(store);
    state.status = StatusStrip {
        device_name: s("Juniper"),
        time: s("14:32"),
        battery_segments: 3,
        charging: false,
        phone_linked: true,
        gnss: true,
        silent: false,
    };
    let chats = [
        Chat {
            avatar: AvatarStyle::peer([235, 187, 21], "kBkQ", PeerShape::Circle),
            name: "Ridge Medic",
            age: "2m",
            hops: Some(2),
            unread: false,
            muted: false,
            draft: "On my way, save me some hot coffee",
        },
        Chat {
            avatar: AvatarStyle::channel(None, "Trail Crew", [203, 24, 112]),
            name: "Trail Crew",
            age: "9m",
            hops: None,
            unread: true,
            muted: false,
            draft: "",
        },
        Chat {
            avatar: AvatarStyle::peer([179, 246, 158], "YmRG", PeerShape::Circle),
            name: "Pat Okafor",
            age: "41m",
            hops: Some(1),
            unread: false,
            muted: false,
            draft: "",
        },
        Chat {
            avatar: AvatarStyle::channel(Some(BuiltinChannel::Public), "public", [0, 0, 0]),
            name: "public",
            age: "1h",
            hops: None,
            unread: false,
            muted: true,
            draft: "",
        },
        Chat {
            avatar: AvatarStyle::peer([94, 10, 224], "H1x*", PeerShape::Circle),
            name: "Jo Reyes",
            age: "3h",
            hops: Some(3),
            unread: false,
            muted: false,
            draft: "",
        },
    ];
    for (id, chat) in (1..).zip(chats) {
        let mut summary = ChatSummary::new(id, chat.avatar, chat.name, store.heap);
        summary.age = s(chat.age);
        summary.unread = chat.unread;
        summary.muted = chat.muted;
        summary.hops = chat.hops;
        let mut draft = DraftText::new(store.heap);
        assert!(draft.splice(0..0, chat.draft), "fixture draft fits");
        summary.draft = EditBuffer::with_text(draft);
        let all = messages(id);
        if let Some(last) = all
            .last()
            .and_then(|&last| message(store.heap, id, all.len() - 1, last))
        {
            summary.preview(&last);
        }
        let _ = state.chats.push(summary);
    }
    state.focus_list();
    state
}
