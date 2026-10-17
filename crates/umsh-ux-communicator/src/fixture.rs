//! Stand-in content until a real node backs the interface: the chats from
//! the T-LoRa Pager mockup, so a render compares directly with it.

use core::cell::{Cell, RefCell};

use crate::avatar::{AvatarStyle, BuiltinChannel, PeerShape};
use crate::model::{
    ChannelAccess, ChannelEntry, ChatId, ChatKind, ChatSummary, Delivery, Details, Direction,
    Invite, MESSAGE_LEN, Member, Message, MessageId, Messages, Node, PeerDetail, PeerEntry, PeerId,
    PeerSummary, Preset, RadioSettings, Reacted, Reaction, Reactions, Sender, ShareUri, Shareable,
    State, StatusStrip, push_newest,
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
    let (color, hint, shape, _) = member_traits(name);
    Sender {
        name: s(name),
        avatar: AvatarStyle::peer(color, hint, shape),
    }
}

/// The channel member `name`'s color, hint, shape, and address. Those with
/// names have made their identity known; the one shown by their hint has
/// not.
fn member_traits(name: &str) -> ([u8; 3], &'static str, PeerShape, Option<&'static str>) {
    match name {
        "Sam" => (
            [218, 50, 133],
            "j3DC",
            PeerShape::Circle,
            Some("j3DCtP9oDcDsv6D8njFcSGvuiLjrAbbY2fru5HBh9gBk"),
        ),
        "Kit" | "Kit Moreno" => (
            [187, 150, 80],
            "iMyA",
            PeerShape::Circle,
            Some("iMyAmdi5uCtG278cpgWezD7WJ2nkTBYa8B92BXfWwRYR"),
        ),
        "Lee Park" => (
            [120, 140, 92],
            "R9f2",
            PeerShape::Circle,
            Some("R9f2wQe8LxV3nTGk5ZbYcJhAs7mPdU1oKrN4tFiHvXyE"),
        ),
        "Ranger Station" => (
            [104, 44, 156],
            "aExj",
            PeerShape::Circle,
            Some("aExjT6pRw2KsL9mQvBzN3cYhG8dUoF5iAeZ1tXkPnWr7"),
        ),
        _ => ([245, 212, 31], "4Fz*", PeerShape::Hexagon, None),
    }
}

/// The member known by their hint until asked, and who they turn out to be.
const UNIDENTIFIED: &str = "Member 4Fz*";
const IDENTIFIED: (&str, &str) = ("Robin Hale", "4FzRC8jVNGDYhYCXKWyHDHzm8MakqsCxtGBGDpQdZDtk");

/// What is known of the channel member `name`, who is `identified` when
/// their identity has arrived.
fn member_card(name: &str, identified: bool) -> Member {
    let (color, hint, shape, address) = member_traits(name);
    if name == UNIDENTIFIED && identified {
        let (name, address) = IDENTIFIED;
        let hint = &address[..4];
        return Member {
            sender: Sender {
                name: s(name),
                avatar: AvatarStyle::peer(color, hint, shape),
            },
            hint: s(hint),
            address: Some(s(address)),
            heard: s("2m"),
            saved: false,
        };
    }
    Member {
        sender: member(name),
        hint: s(hint),
        address: address.map(s),
        heard: s("2m"),
        saved: false,
    }
}

/// Whether `chat` is one of the fixture's channels: Trail Crew, public, or
/// EMERGENCY.
fn is_channel(chat: ChatId) -> bool {
    matches!(chat, 2 | 4 | 6)
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
        6 => &[
            (From::Member("Kit Moreno"), "Heard a slide near mile 3?"),
            (From::Member("Lee Park"), "Rockslide on FS 42 at mile 3"),
            (From::Member("Ranger Station"), "Copy. Crew on the way."),
        ],
        5 => &[
            (From::Me, "Which channel is base camp on?"),
            (From::Peer, "This one:"),
            // An invitation, and nothing besides.
            (From::Peer, ""),
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

/// The channel a message of the fixture's invites to.
fn invite(heap: TextHeap, chat: ChatId, index: usize) -> Option<Invite> {
    if chat != 5 || index != 2 {
        return None;
    }
    Some(Invite {
        name: SharedText::new(heap, "Base Camp")?,
        builtin: None,
        tint: [46, 125, 196],
        access: ChannelAccess::Private,
    })
}

/// Others' reactions to a message of the fixture's, one each.
fn their_reactions(chat: ChatId, index: usize) -> &'static [Reaction] {
    match (chat, index) {
        (1, 1) => &[Reaction::Heart],
        (2, 1) => &[Reaction::ThumbsUp, Reaction::Exclaim],
        _ => &[],
    }
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
        reactions: their_reactions(chat, index)
            .iter()
            .map(|&reaction| Reacted {
                reaction,
                count: 1,
                mine: false,
            })
            .collect::<Reactions>(),
        invite: invite(heap, chat, index),
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
    /// The unidentified member's identity has been asked for, and has
    /// arrived.
    asked: Cell<bool>,
    identified: Cell<bool>,
    /// Members saved as peers, and direct chats made with members, by
    /// address.
    saved: RefCell<Vec<String<44>, KEPT>>,
    direct: RefCell<Vec<String<44>, KEPT>>,
    /// Channels joined since, by name, and who can read them.
    joined: RefCell<Vec<(String<32>, ChannelAccess), KEPT>>,
    /// Peers with forward secrecy turned on, peers pinged, and peers forgotten.
    secrecy: RefCell<Vec<PeerId, KEPT>>,
    pinged: RefCell<Vec<PeerId, KEPT>>,
    forgotten: RefCell<Vec<PeerId, KEPT>>,
    /// The radio's settings.
    preset: Cell<Preset>,
    forwarding: Cell<bool>,
    share_location: Cell<bool>,
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
            asked: Cell::new(false),
            identified: Cell::new(false),
            saved: RefCell::new(Vec::new()),
            direct: RefCell::new(Vec::new()),
            joined: RefCell::new(Vec::new()),
            secrecy: RefCell::new(Vec::new()),
            pinged: RefCell::new(Vec::new()),
            forgotten: RefCell::new(Vec::new()),
            preset: Cell::new(Preset::LongRange),
            forwarding: Cell::new(true),
            share_location: Cell::new(false),
        }
    }
}

/// The fixture's saved peers' addresses, how they are reached, when they
/// were last heard, and the chat with them, by peer.
const PEERS: [(&str, Option<&str>, &str, Option<ChatId>); 4] = [
    (
        "YmRGmitsDJy13p2eiuxziZph6qDZFS1pcN1uaofYjM45",
        Some("Direct \u{B7} \u{2212}97 dBm"),
        "5 min ago",
        Some(3),
    ),
    (
        "H1xBRrvv1ExLhX3wqH3LdU7ew8dkovDZsgMuCcPZXa7y",
        Some("3 hops"),
        "1 hr ago",
        Some(5),
    ),
    (
        "hhaevhk7fePTPDkBgW55Sd5Zh9oy3bC5ueFrUxcVUtYg",
        Some("3 hops"),
        "3 hr ago",
        None,
    ),
    (
        "Dc7qZiXV4uuYKB4TvuRhW2XFUgdP5v986FkCyhq9FAyq",
        Some("2 hops"),
        "4 hr ago",
        None,
    ),
];

/// How a node URI begins.
const PEER_INVITATION: &str = "umsh:n:";

/// Where direct chats made with members are numbered from, above the
/// fixture's own chats.
const FIRST_DIRECT: ChatId = 0x100;
/// Where members saved as peers are numbered from, above the fixture's own
/// peers.
const FIRST_SAVED: PeerId = 0x100;
/// Where channels joined since are numbered from.
const FIRST_JOINED: ChatId = 0x200;
const CHANNEL_INVITATION: &str = "umsh:ck:";

/// This node's address, as the mockup's Share screen shows it.
const ADDRESS: &str = "Geb9QfXhe5f1vXHcFDnXbvcXShxAFhAxaRp1jr5Bqe3L";
/// Private channels' keys: Trail Crew's, and any joined from an invitation.
const TRAIL_CREW_KEY: &str = "5BFn8YGKJ6pZR4qV3tW7mNhDrXsCxEaL9kUv2wAjT8bP";
const JOINED_KEY: &str = "8xQvW3mZr7KpN2sTfYhB6cLdJ4gE9aUoRiV5tHnXkM1w";

/// As much of `text` as a name holds.
fn cut(text: &str) -> String<32> {
    let mut name = String::new();
    for c in text.chars() {
        if name.push(c).is_err() {
            break;
        }
    }
    name
}

/// A channel's name as its key is derived from: ASCII folded to lowercase.
fn folded(name: &str) -> String<32> {
    let mut folded = cut(name);
    folded.make_ascii_lowercase();
    folded
}

/// A URI parameter's value with its percent-encoding undone; what does not
/// decode to text stays as it was typed.
fn decoded(value: &str) -> String<32> {
    let mut bytes: Vec<u8, 32> = Vec::new();
    let mut rest = value.as_bytes();
    while let Some((&byte, tail)) = rest.split_first() {
        let escaped = match (byte, tail) {
            (b'%', [high, low, after @ ..]) => core::str::from_utf8(&[*high, *low])
                .ok()
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
                .map(|byte| (byte, after)),
            _ => None,
        };
        let (byte, tail) = escaped.unwrap_or((byte, tail));
        if bytes.push(byte).is_err() {
            break;
        }
        rest = tail;
    }
    core::str::from_utf8(&bytes).map_or_else(|_| cut(value), cut)
}

/// A channel avatar's color, from its name.
fn tint(name: &str) -> [u8; 3] {
    let hash = name.bytes().fold(0x811C_9DC5_u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    });
    let [r, g, b, _] = hash.to_be_bytes();
    [r, g, b]
}

/// `name` as a URI parameter: what is not a letter, a digit, or `-._~` is
/// percent-encoded.
fn push_encoded(uri: &mut ShareUri, name: &str) -> Option<()> {
    for byte in name.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            uri.push(char::from(byte)).ok()?;
        } else {
            core::fmt::write(uri, format_args!("%{byte:02X}")).ok()?;
        }
    }
    Some(())
}

fn private_uri(key: &str, name: &str) -> Option<ShareUri> {
    let mut uri = ShareUri::new();
    core::fmt::write(&mut uri, format_args!("umsh:ck:{key}?n=")).ok()?;
    push_encoded(&mut uri, name)?;
    Some(uri)
}

impl Store {
    /// The chat of the channel joined since as `name`, numbered the first
    /// time it is joined.
    fn joined_id(&self, name: &str, access: ChannelAccess) -> Option<ChatId> {
        let mut joined = self.joined.borrow_mut();
        let index = match joined.iter().position(|(n, _)| n.as_str() == name) {
            Some(index) => index,
            None => {
                joined.push((cut(name), access)).ok()?;
                joined.len() - 1
            }
        };
        Some(FIRST_JOINED + index as ChatId)
    }

    /// A fixture chat joined from the start, as `add_channel` returns it;
    /// the list already holds it, so only its ID matters.
    fn existing(&self, chat: ChatId) -> ChatSummary {
        ChatSummary::new(
            chat,
            AvatarStyle::channel(None, "", [0, 0, 0]),
            "",
            self.heap,
        )
    }

    /// The next message to arrive, the ones on the T-LoRa Pager mockup's
    /// wake screen, and the chat it arrived in. It is kept as the newest
    /// message there. `None` once all have arrived.
    pub fn arrive(&self) -> Option<(ChatId, Message)> {
        let index = self.arrived.get();
        let &(chat, from, text) = ARRIVALS.get(index)?;
        self.arrived.set(index + 1);
        Some((chat, self.keep(chat, from, text)?))
    }

    /// An identity has been asked for and has not yet arrived.
    #[must_use]
    pub fn identity_asked(&self) -> bool {
        self.asked.get() && !self.identified.get()
    }

    /// The identity asked for arrives. Returns whether one was asked for;
    /// [`State::identities_arrived`] then shows it.
    pub fn answer_identity(&self) -> bool {
        let answered = self.identity_asked();
        if answered {
            self.identified.set(true);
        }
        answered
    }

    /// What is known of the channel member `name`.
    fn card(&self, name: &str) -> Member {
        let mut member = member_card(name, self.identified.get());
        member.saved = member
            .address
            .as_ref()
            .is_some_and(|address| self.saved.borrow().contains(address));
        member
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
            invite: None,
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

impl Node for Store {
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
        if self.identified.get() {
            let known = self.card(UNIDENTIFIED).sender;
            for message in into.iter_mut() {
                if let Some(sender) = message.sender.as_mut().filter(|s| s.name == UNIDENTIFIED) {
                    *sender = known.clone();
                }
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

    fn member(&self, chat: ChatId, message: MessageId) -> Option<Member> {
        let name = if message >= 0x1_0000 {
            let kept = self.kept.borrow();
            let (_, kept) = kept.iter().find(|(_, m)| m.id == message)?;
            kept.sender.as_ref()?.name.clone()
        } else {
            match messages(chat).get((message & 0xFF) as usize)? {
                (From::Member(name), _) => s(name),
                _ => return None,
            }
        };
        Some(self.card(&name))
    }

    fn request_identity(&self, _chat: ChatId, _message: MessageId) {
        // Only the unidentified member has an identity to come.
        self.asked.set(true);
    }

    fn direct_chat(&self, address: &str, name: &str, avatar: &AvatarStyle) -> Option<ChatSummary> {
        // A saved peer the fixture already has a chat with.
        if let Some(chat) = PEERS
            .iter()
            .find(|(a, ..)| *a == address)
            .and_then(|&(.., chat)| chat)
        {
            return Some(ChatSummary::new(chat, avatar.clone(), name, self.heap));
        }
        let mut direct = self.direct.borrow_mut();
        let index = match direct.iter().position(|a| a == address) {
            Some(index) => index,
            None => {
                direct.push(String::try_from(address).ok()?).ok()?;
                direct.len() - 1
            }
        };
        Some(ChatSummary::new(
            FIRST_DIRECT + index as ChatId,
            avatar.clone(),
            name,
            self.heap,
        ))
    }

    fn peer_detail(&self, peer: PeerId) -> Option<PeerDetail> {
        if self.forgotten.borrow().contains(&peer) {
            return None;
        }
        let (address, route, heard) = match peer.checked_sub(1).and_then(|i| PEERS.get(i as usize))
        {
            Some(&(address, route, heard, _)) => (s(address), route.map(s), heard),
            // A member saved since the fixture started.
            None => {
                let saved = self.saved.borrow();
                let index = peer.checked_sub(FIRST_SAVED)? as usize;
                (saved.get(index)?.clone(), None, "2 min ago")
            }
        };
        Some(PeerDetail {
            address,
            route,
            heard: s(heard),
            forward_secrecy: self.secrecy.borrow().contains(&peer),
            // A ping is answered at once.
            ping: self.pinged.borrow().contains(&peer).then(|| s("1.4 s")),
        })
    }

    fn set_forward_secrecy(&self, peer: PeerId, on: bool) {
        let mut secrecy = self.secrecy.borrow_mut();
        secrecy.retain(|p| *p != peer);
        if on {
            _ = secrecy.push(peer);
        }
    }

    fn ping(&self, peer: PeerId) {
        let mut pinged = self.pinged.borrow_mut();
        if !pinged.contains(&peer) {
            _ = pinged.push(peer);
        }
    }

    fn forget_peer(&self, peer: PeerId) {
        _ = self.forgotten.borrow_mut().push(peer);
    }

    fn address(&self) -> String<44> {
        s(ADDRESS)
    }

    fn share_uri(&self, what: Shareable) -> Option<ShareUri> {
        match what {
            Shareable::Identity => {
                let mut uri = ShareUri::new();
                core::fmt::write(&mut uri, format_args!("umsh:n:{ADDRESS}")).ok()?;
                Some(uri)
            }
            Shareable::Channel(2) => private_uri(TRAIL_CREW_KEY, "Trail Crew"),
            Shareable::Channel(4) => ShareUri::try_from("umsh:cs:public").ok(),
            Shareable::Channel(6) => ShareUri::try_from("umsh:cs:emergency").ok(),
            Shareable::Channel(chat) => {
                let index = chat.checked_sub(FIRST_JOINED)? as usize;
                let joined = self.joined.borrow();
                let (name, access) = joined.get(index)?;
                match access {
                    ChannelAccess::Private => private_uri(JOINED_KEY, name),
                    _ => {
                        let mut uri = ShareUri::new();
                        core::fmt::write(&mut uri, format_args!("umsh:cs:")).ok()?;
                        push_encoded(&mut uri, &folded(name))?;
                        Some(uri)
                    }
                }
            }
        }
    }

    fn radio(&self) -> RadioSettings {
        RadioSettings {
            region: String::try_from("United States").expect("fixture text fits"),
            preset: self.preset.get(),
            forwarding: self.forwarding.get(),
            share_location: self.share_location.get(),
            airtime: 3,
        }
    }

    fn set_preset(&self, preset: Preset) {
        self.preset.set(preset);
    }

    fn set_forwarding(&self, on: bool) {
        self.forwarding.set(on);
    }

    fn set_share_location(&self, on: bool) {
        self.share_location.set(on);
    }

    fn inspect_peer(&self, text: &str) -> PeerEntry {
        use umsh_core::NodeHint;
        use umsh_core::base58::{ENCODED_LEN, prefix_bounds};

        let text = text.trim();
        if text.is_empty() {
            return PeerEntry::Empty;
        }
        // An invitation carries the address, and may carry an identity after
        // it, whose name a node would read; the fixture does not.
        let (digits, name) = match text.strip_prefix(PEER_INVITATION) {
            Some(rest) => (
                rest.split_once(':').map_or(rest, |(digits, _)| digits),
                None,
            ),
            // An invitation still being typed.
            None if PEER_INVITATION.starts_with(text) => {
                return PeerEntry::Partial {
                    avatar: None,
                    hint: String::new(),
                    missing: ENCODED_LEN as u8,
                };
            }
            None => (text, None),
        };
        if digits.is_empty() {
            return PeerEntry::Partial {
                avatar: None,
                hint: String::new(),
                missing: ENCODED_LEN as u8,
            };
        }
        let Ok((low, high)) = prefix_bounds(digits.as_bytes()) else {
            return PeerEntry::Invalid;
        };
        let bytes = [low[0], low[1], low[2]];
        let mut hint: String<8> = String::new();
        _ = core::fmt::write(&mut hint, format_args!("{}", NodeHint(bytes)));
        if digits.len() == ENCODED_LEN {
            // The whole address settles every character of the hint.
            let hint: String<8> = s(&digits[..4]);
            let avatar = AvatarStyle::peer(bytes, &hint, PeerShape::Hexagon);
            return PeerEntry::Complete {
                avatar,
                hint,
                address: s(digits),
                name,
            };
        }
        // A peer known by nothing but their address is drawn as a hexagon.
        let avatar = AvatarStyle::peer(bytes, &hint, PeerShape::Hexagon);
        // The avatar shows once the characters settle the hint.
        let settled = low[..3] == high[..3];
        PeerEntry::Partial {
            avatar: settled.then_some(avatar),
            hint: if settled { hint } else { String::new() },
            missing: (ENCODED_LEN - digits.len()) as u8,
        }
    }

    fn join(&self, invite: &Invite, name: &str) -> Option<ChatSummary> {
        let id = self.joined_id(&invite.name, invite.access)?;
        let mut summary = ChatSummary::new(id, invite.avatar(), name, self.heap);
        summary.kind = ChatKind::Channel {
            access: invite.access,
        };
        Some(summary)
    }

    fn inspect_channel(&self, text: &str) -> ChannelEntry {
        use umsh_core::base58::{ENCODED_LEN, prefix_bounds};

        let text = text.trim();
        if text.is_empty() {
            return ChannelEntry::Empty;
        }
        let missing = |key: &str| ChannelEntry::Partial {
            missing: (ENCODED_LEN - key.len()) as u8,
        };
        match text.strip_prefix(CHANNEL_INVITATION) {
            Some(rest) => {
                let key = rest.split_once('?').map_or(rest, |(key, _)| key);
                if key.is_empty() {
                    missing(key)
                } else if key.len() > ENCODED_LEN || prefix_bounds(key.as_bytes()).is_err() {
                    ChannelEntry::Invalid
                } else if key.len() == ENCODED_LEN {
                    ChannelEntry::Private
                } else {
                    missing(key)
                }
            }
            // An invitation still being typed.
            None if CHANNEL_INVITATION.starts_with(text) && text.contains(':') => missing(""),
            // A name is any ASCII without the invitation's colon.
            None if text.is_ascii() && !text.contains(':') => ChannelEntry::Named,
            None => ChannelEntry::Invalid,
        }
    }

    fn add_channel(&self, text: &str) -> Option<ChatSummary> {
        let text = text.trim();
        let (name, access): (String<32>, _) = match self.inspect_channel(text) {
            ChannelEntry::Named => match folded(text).as_str() {
                // Joined from the start.
                "public" => return Some(self.existing(4)),
                "emergency" => return Some(self.existing(6)),
                _ => (cut(text), ChannelAccess::Named),
            },
            ChannelEntry::Private => {
                let rest = &text[CHANNEL_INVITATION.len()..];
                let (key, query) = rest.split_once('?').unwrap_or((rest, ""));
                let name = query
                    .split(';')
                    .find_map(|p| p.strip_prefix("n="))
                    .map(decoded)
                    .filter(|n| !n.is_empty());
                let name = name.unwrap_or_else(|| {
                    let mut name: String<32> = String::new();
                    _ = core::fmt::write(&mut name, format_args!("Channel {}", &key[..4]));
                    name
                });
                (name, ChannelAccess::Private)
            }
            _ => return None,
        };
        let id = self.joined_id(&name, access)?;
        let avatar = AvatarStyle::channel(None, &name, tint(&name));
        let mut summary = ChatSummary::new(id, avatar, &name, self.heap);
        summary.kind = ChatKind::Channel { access };
        Some(summary)
    }

    fn save_peer(&self, member: &Member) -> Option<PeerSummary> {
        let address = member.address.as_ref()?;
        let mut saved = self.saved.borrow_mut();
        let index = match saved.iter().position(|a| a == address) {
            Some(index) => index,
            None => {
                saved.push(address.clone()).ok()?;
                saved.len() - 1
            }
        };
        Some(PeerSummary {
            id: FIRST_SAVED + index as PeerId,
            avatar: member.sender.avatar.clone(),
            name: s(&member.sender.name),
            hops: None,
            signal: None,
            heard: member.heard.clone(),
        })
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
            kind: ChatKind::Channel {
                access: ChannelAccess::Private,
            },
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
            kind: ChatKind::Channel {
                access: ChannelAccess::Named,
            },
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
        Chat {
            avatar: AvatarStyle::channel(Some(BuiltinChannel::Emergency), "EMERGENCY", [0, 0, 0]),
            name: "EMERGENCY",
            age: "5h",
            kind: ChatKind::Channel {
                access: ChannelAccess::Clear,
            },
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
    // The mockup's saved peers, most recently heard first.
    let peers = [
        (
            AvatarStyle::peer([179, 246, 158], "YmRG", PeerShape::Circle),
            "Pat Okafor",
            Some(0),
            Some(3),
            "5m",
        ),
        (
            AvatarStyle::peer([94, 10, 224], "H1x*", PeerShape::Circle),
            "Jo Reyes",
            Some(3),
            None,
            "1h",
        ),
        (
            AvatarStyle::peer([178, 186, 92], "hhae", PeerShape::Circle),
            "Rick Alvarez",
            Some(3),
            None,
            "3h",
        ),
        (
            AvatarStyle::peer([64, 160, 200], "Dc7q", PeerShape::Circle),
            "Dana Cho",
            Some(2),
            None,
            "4h",
        ),
    ];
    for (id, (avatar, name, hops, signal, heard)) in (1..).zip(peers) {
        let _ = state.peers.push(PeerSummary {
            id,
            avatar,
            name: s(name),
            hops,
            signal,
            heard: s(heard),
        });
    }
    state.focus_list();
    state
}
