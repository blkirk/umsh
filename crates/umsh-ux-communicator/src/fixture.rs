//! Stand-in content until a real node backs the interface: the chats from
//! the T-LoRa Pager mockup, so a render compares directly with it.

use core::cell::{Cell, RefCell};

use crate::avatar::{AvatarStyle, BuiltinChannel, PeerShape};
use crate::model::{
    ChatId, ChatKind, ChatSummary, Delivery, Details, Direction, History, MESSAGE_LEN, Message,
    MessageId, Messages, Reacted, Reaction, Reactions, Sender, State, StatusStrip, push_newest,
};
use crate::text::{DraftText, SharedText, TextHeap};
use buoyant::view::{EditBuffer, EditText as _};
use heapless::{String, Vec};

fn s<const N: usize>(text: &str) -> String<N> {
    String::try_from(text).expect("fixture text fits")
}

/// A channel member as the channel shows them: a name the mockup gives
/// them, or their hint when nothing better is known.
fn member(name: &str) -> Sender {
    let avatar = match name {
        "Sam" => AvatarStyle::peer([218, 50, 133], "j3DC", PeerShape::Circle),
        "Kit" => AvatarStyle::peer([187, 150, 80], "iMyA", PeerShape::Circle),
        _ => AvatarStyle::peer([245, 212, 31], "4Fz*", PeerShape::Hexagon),
    };
    Sender {
        name: s(name),
        avatar,
    }
}

/// Whether `chat` is one of the fixture's channels, Trail Crew or public.
fn is_channel(chat: ChatId) -> bool {
    matches!(chat, 2 | 4)
}

struct Chat {
    avatar: AvatarStyle,
    name: &'static str,
    age: &'static str,
    kind: ChatKind,
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
        2 => &[
            (From::Member("Sam"), "At the saddle"),
            (From::Member("Member 4Fz*"), "Two of us behind you"),
            (From::Member("Sam"), "Will wait 10 min"),
        ],
        3 => &[
            (From::Peer, "Is Bald Knob back up?"),
            (From::Me, "Not yet, Saturday."),
            (From::Me, "Bring the spare antenna?"),
        ],
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

/// The peer's reaction to a message of the fixture's.
fn their_reaction(chat: ChatId, index: usize) -> Option<Reaction> {
    (chat == 1 && index == 1).then_some(Reaction::Heart)
}

/// Messages that arrive after the fixture starts, in order.
const ARRIVALS: [(ChatId, From, &str); 3] = [
    (
        2,
        From::Member("Sam"),
        "Will wait 10 min at the saddle, then we head down",
    ),
    (3, From::Peer, "Got the antenna. Bringing two."),
    (5, From::Peer, "Are you on the ridge yet?"),
];

/// How far a message of the fixture's got.
fn delivery(chat: ChatId, index: usize) -> Delivery {
    if chat == 3 && index == 2 {
        Delivery::Unconfirmed
    } else {
        Delivery::Delivered
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
            From::Member(name) => Some(member(name)),
            From::Me | From::Peer => None,
        },
        text: SharedText::new(heap, text)?,
        delivery: delivery(chat, index),
        reactions: their_reaction(chat, index)
            .map(|reaction| Reacted {
                reaction,
                count: 1,
                mine: false,
            })
            .into_iter()
            .collect::<Reactions>(),
    })
}

/// How many messages sent or received since it started the fixture keeps,
/// across all chats.
const KEPT: usize = 8;

/// The fixture's messages, and the ones sent or received since it started, with their
/// text kept on a heap of the device's choosing.
pub struct Store {
    heap: TextHeap,
    kept: RefCell<Vec<(ChatId, Message), KEPT>>,
    /// How many of the fixture's arrivals have arrived.
    arrived: Cell<usize>,
    /// My reactions, by message.
    mine: RefCell<Vec<(MessageId, Reaction), KEPT>>,
    /// Unconfirmed messages sent again.
    retried: RefCell<Vec<MessageId, KEPT>>,
    next: Cell<MessageId>,
}

impl Store {
    #[must_use]
    pub const fn new(heap: TextHeap) -> Self {
        Self {
            heap,
            kept: RefCell::new(Vec::new()),
            arrived: Cell::new(0),
            mine: RefCell::new(Vec::new()),
            retried: RefCell::new(Vec::new()),
            // Above the fixture's own messages.
            next: Cell::new(0x1_0000),
        }
    }
}

impl Store {
    /// The next message to arrive, the ones on the T-LoRa Pager mockup's
    /// wake screen, and the chat it arrived in. It is kept as the newest
    /// message there. `None` once all have arrived.
    pub fn arrive(&self) -> Option<(ChatId, Message)> {
        let index = self.arrived.get();
        let &(chat, from, text) = ARRIVALS.get(index)?;
        self.arrived.set(index + 1);
        Some((chat, self.keep(chat, from, text)?))
    }

    /// Keeps `text`, from `from`, as the newest message of `chat`.
    fn keep(&self, chat: ChatId, from: From, text: &str) -> Option<Message> {
        let message = Message {
            id: self.next.get(),
            direction: match from {
                From::Me => Direction::Outbound,
                From::Peer | From::Member(_) => Direction::Inbound,
            },
            sender: match from {
                From::Member(name) => Some(member(name)),
                From::Me | From::Peer => None,
            },
            text: SharedText::new(self.heap, text)?,
            delivery: match from {
                // A channel's sends have no acknowledgment to wait for.
                From::Me if is_channel(chat) => Delivery::Sent,
                From::Me => Delivery::Sending,
                From::Peer | From::Member(_) => Delivery::Delivered,
            },
            reactions: Reactions::new(),
        };
        self.next.set(message.id + 1);
        let mut kept = self.kept.borrow_mut();
        if kept.is_full() {
            kept.remove(0);
        }
        _ = kept.push((chat, message.clone()));
        Some(message)
    }
}

impl core::fmt::Debug for Store {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Store")
            .field("kept", &self.kept)
            .field("arrived", &self.arrived)
            .field("mine", &self.mine)
            .field("retried", &self.retried)
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
        for (_, kept) in self.kept.borrow().iter().filter(|(c, _)| *c == chat) {
            push_newest(into, kept.clone());
        }
        for &(id, reaction) in self.mine.borrow().iter() {
            if let Some(message) = into.iter_mut().find(|m| m.id == id) {
                message.set_my_reaction(Some(reaction));
            }
        }
        for id in self.retried.borrow().iter() {
            if let Some(message) = into.iter_mut().find(|m| m.id == *id) {
                message.delivery = Delivery::Sending;
            }
        }
    }

    fn send(&self, chat: ChatId, text: &str, _reply_to: Option<MessageId>) -> Option<Message> {
        self.keep(chat, From::Me, text)
    }

    fn react(&self, _chat: ChatId, message: MessageId, reaction: Option<Reaction>) {
        let mut mine = self.mine.borrow_mut();
        mine.retain(|(id, _)| *id != message);
        if let Some(reaction) = reaction {
            if mine.is_full() {
                mine.remove(0);
            }
            _ = mine.push((message, reaction));
        }
    }

    fn retry(&self, _chat: ChatId, message: MessageId) -> bool {
        let mut retried = self.retried.borrow_mut();
        if !retried.contains(&message) {
            if retried.is_full() {
                retried.remove(0);
            }
            _ = retried.push(message);
        }
        true
    }

    fn details(&self, chat: ChatId, message: MessageId) -> Option<Details> {
        // Sent or received since the fixture started, or the fixture's own.
        let (outbound, time) = if message >= 0x1_0000 {
            let kept = self.kept.borrow();
            let (_, kept) = kept.iter().find(|(_, m)| m.id == message)?;
            (kept.direction == Direction::Outbound, "14:32")
        } else {
            let (from, _) = *messages(chat).get((message & 0xFF) as usize)?;
            match from {
                From::Me => (true, "14:27"),
                From::Peer | From::Member(_) => (false, "14:29"),
            }
        };
        Some(if outbound {
            Details {
                time: s(time),
                ..Details::default()
            }
        } else {
            Details {
                time: s(time),
                hops: Some(2),
                route: s("Gh* \u{203A} 4Tn"),
                rssi: Some(-97),
                snr: Some(65),
                authenticated: true,
            }
        })
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
            kind: ChatKind::Peer { hops: Some(2) },
            unread: false,
            muted: false,
            draft: "On my way, save me some hot coffee",
        },
        Chat {
            avatar: AvatarStyle::channel(None, "Trail Crew", [203, 24, 112]),
            name: "Trail Crew",
            age: "9m",
            kind: ChatKind::Channel,
            unread: true,
            muted: false,
            draft: "",
        },
        Chat {
            avatar: AvatarStyle::peer([179, 246, 158], "YmRG", PeerShape::Circle),
            name: "Pat Okafor",
            age: "41m",
            kind: ChatKind::Peer { hops: Some(0) },
            unread: false,
            muted: false,
            draft: "",
        },
        Chat {
            avatar: AvatarStyle::channel(Some(BuiltinChannel::Public), "public", [0, 0, 0]),
            name: "public",
            age: "1h",
            kind: ChatKind::Channel,
            unread: false,
            muted: true,
            draft: "",
        },
        Chat {
            avatar: AvatarStyle::peer([94, 10, 224], "H1x*", PeerShape::Circle),
            name: "Jo Reyes",
            age: "3h",
            kind: ChatKind::Peer { hops: Some(3) },
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
        summary.kind = chat.kind;
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
