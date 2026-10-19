//! The node, against the soft device's real session through the device's
//! own interface, and fed channel frames as the device node hands them
//! over.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

mod support;

use support::{Port, SharedFlash, config, device, provision, starting_keys};
use umsh_communicator::settings::SAVE_DELAY_MS;
use umsh_communicator::task::Arrival;
use umsh_communicator::task::Command;
use umsh_communicator::{
    ChannelCrypto, Clock, CommunicatorNode, FlashMap, FlashMapStore, MemoryStore, Quota, Seeds,
};
use umsh_core::{
    ChannelId, ChannelKey, ChannelTag, Fcf, PacketHeader, PacketType, ParsedOptions, PayloadType,
    PublicKey, SourceAddrRef,
};
use umsh_crypto::CryptoEngine;
use umsh_crypto::software::{SoftwareAes, SoftwareSha256};
use umsh_mac::{ChannelInfoRef, ReceivedPacketRef, RxMetadata, Snr};
use umsh_node::{NodeCapabilities, NodeIdentityPayload, NodeRole};
use umsh_text::ConversationKey;
use umsh_text::chat::ChatFacade;
use umsh_text::engine::{ComposeIntent, EngineConfig};
use umsh_text::node_adapter::{
    Inbound, InboundText, RxSummary, Sent, WellKnownChannels, inbound_text,
};
use umsh_text::validate::DirectChannelProfile;
use umsh_ulcp::frame;
use umsh_ulcp::hdlc;
use umsh_ulcp::ids::prop;
use umsh_ulcp::profiles::VETTED;
use umsh_ulcp_simdev::{SessionConfig, SimulatedDevice};
use umsh_ux_communicator::model::{
    ChatKind, Chats, Delivery, Direction, KeyboardSettings, Messages, Node, NodeEvent, PeerEntry,
    Peers, Profile, Reaction, Setting, Shareable,
};

/// 14:32 UTC, when the test clock starts.
const STARTED: u32 = 1_792_247_520;

/// A clock the test moves by hand.
struct TestClock(AtomicU64);

impl TestClock {
    fn set(&self, ms: u64) {
        self.0.store(ms, Ordering::Relaxed);
    }
}

impl Clock for TestClock {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }

    fn wall(&self) -> Option<u32> {
        Some(STARTED + (self.now_ms() / 1_000) as u32)
    }

    fn utc_offset_min(&self) -> i16 {
        0
    }
}

fn tag(key: &ChannelKey) -> ChannelTag {
    CryptoEngine::new(SoftwareAes, SoftwareSha256).derive_channel_tag(key)
}

/// Anyone's key but the device's: the device's own is made at random.
const LOCAL: PublicKey = PublicKey([0x42; 32]);

/// A node over `sim`, with its settings read, and the clock it reads.
fn node(sim: &Rc<RefCell<SimulatedDevice>>) -> (CommunicatorNode<Port>, &'static TestClock) {
    let clock: &'static TestClock = Box::leak(Box::new(TestClock(AtomicU64::new(0))));
    let node = CommunicatorNode::new(
        Port(sim.clone()),
        MemoryStore::new(),
        ChannelCrypto::software(),
        &std::alloc::System,
        clock,
        Seeds {
            session_id: 1,
            jitter_seed: 2,
        },
    );
    node.pump();
    assert!(node.primed());
    (node, clock)
}

/// Gives `sim` an identity, as whoever holds the device may.
fn identify(sim: &Rc<RefCell<SimulatedDevice>>) {
    provision(sim, None);
}

/// A node over a device with an identity, whose chats have started, with
/// what it first said taken.
fn started(sim: &Rc<RefCell<SimulatedDevice>>) -> (CommunicatorNode<Port>, &'static TestClock) {
    identify(sim);
    let (node, clock) = node(sim);
    assert!(node.started());
    events(&node);
    (node, clock)
}

fn events(node: &CommunicatorNode<Port>) -> Vec<NodeEvent> {
    core::iter::from_fn(|| node.next_event()).collect()
}

fn chats(node: &CommunicatorNode<Port>) -> Chats {
    let mut chats = Chats::new();
    node.chats(&mut chats);
    chats
}

/// What a channel member's own chat session sends: `body` under the
/// Sender Handle `handle`, as the payload of one frame.
fn composed(from: PublicKey, key: ChannelKey, handle: &str, body: &str) -> Vec<u8> {
    let mut peer: ChatFacade = ChatFacade::new(
        DirectChannelProfile,
        from,
        EngineConfig::default(),
        Seeds {
            session_id: 7,
            jitter_seed: 8,
        },
    );
    peer.channels_mut().register(tag(&key), key, None);
    peer.set_local_handle(handle);
    let conversation = ConversationKey::ChannelGroup { channel: tag(&key) };
    let batch = peer
        .compose(
            conversation,
            1,
            ComposeIntent::Text {
                body,
                status: false,
            },
            None,
            0,
        )
        .unwrap();
    let outbound = peer.commit(batch).unwrap();
    outbound.transmissions()[0].payload.to_vec()
}

/// A channel frame from `from`, as the MAC hands it to the device node
/// once its channel key has authenticated it.
fn received(key: &ChannelKey, from: PublicKey, payload: &[u8]) -> InboundText {
    channel_frame(key, from, true, payload)
}

/// A channel frame from `from`, naming them by full key or by hint.
fn channel_frame(key: &ChannelKey, from: PublicKey, full_key: bool, payload: &[u8]) -> InboundText {
    let mut bytes = vec![PayloadType::TextMessage as u8];
    bytes.extend_from_slice(payload);
    let header = PacketHeader {
        fcf: Fcf::new(PacketType::Multicast, full_key, false),
        options_range: 0..0,
        flood_hops: None,
        dst: None,
        channel: Some(ChannelId([0, 0])),
        source: if full_key {
            SourceAddrRef::FullKeyAt { offset: 0 }
        } else {
            SourceAddrRef::Hint(from.hint())
        },
        sec_info: None,
        body_range: 0..0,
        mic_range: 0..0,
        total_len: 0,
    };
    let packet = ReceivedPacketRef::new(
        &[],
        &bytes,
        header,
        ParsedOptions::default(),
        full_key.then_some(from),
        Some(from.hint()),
        false,
        Some(ChannelInfoRef {
            id: ChannelId([0, 0]),
            key,
        }),
        RxMetadata::new(Some(-97), Some(Snr::from_centibels(65)), None, None),
    );
    match inbound_text(&packet, &LOCAL, &WellKnownChannels::derive()) {
        Some(Inbound::Text(text)) => text,
        _ => panic!("not a text frame"),
    }
}

// ─── Settings ────────────────────────────────────────────────────────

#[test]
fn the_first_boot_saves_what_the_device_starts_with() {
    let sim = device();
    assert!(sim.borrow().saved_snapshot().is_none());
    let (_node, _) = node(&sim);
    assert!(sim.borrow().saved_snapshot().is_some());

    // With a snapshot in effect, the next boot leaves it alone.
    let before = sim.borrow().saved_snapshot().unwrap().to_vec();
    let (_node, _) = node(&sim);
    assert_eq!(sim.borrow().saved_snapshot().unwrap(), &before[..]);
}

/// A change reads at once, and the device keeps the snapshot in step with
/// it: no save is asked for while one is in effect.
#[test]
fn a_change_reads_at_once_and_is_kept() {
    let sim = device();
    let (node, clock) = node(&sim);
    events(&node);
    let before = sim.borrow().saved_snapshot().unwrap().to_vec();

    node.set_name("Juniper");
    assert_eq!(node.device_name(), "Juniper");
    clock.set(1_000);
    node.pump();
    assert_eq!(events(&node), [NodeEvent::SettingsChanged]);
    assert_eq!(node.device_name(), "Juniper");
    let kept = sim.borrow().saved_snapshot().unwrap().to_vec();
    assert_ne!(kept, before, "the snapshot was kept in step");
    clock.set(1_000 + SAVE_DELAY_MS);
    node.pump();
    assert_eq!(sim.borrow().saved_snapshot().unwrap(), &kept[..]);
}

#[test]
fn the_profile_picker_reads_vetted_and_custom() {
    let sim = device();
    let (node, clock) = node(&sim);
    let index = VETTED
        .iter()
        .position(|profile| profile.id == "meshcore-us-ca")
        .unwrap();
    assert_eq!(node.radio().profile, Profile::Vetted(index));
    assert_eq!(node.profile(index), Some(VETTED[index].name));
    assert_eq!(node.profile_count(), VETTED.len());

    let other = (index + 1) % VETTED.len();
    node.set_profile(other);
    assert_eq!(node.radio().profile, Profile::Vetted(other));
    clock.set(1_000);
    node.pump();
    assert_eq!(node.radio().profile, Profile::Vetted(other));

    node.set_profile(VETTED.len());
    assert_eq!(
        events(&node).last(),
        Some(&NodeEvent::SettingRefused(Setting::Profile))
    );
}

#[test]
fn a_refused_change_reads_back_what_the_device_kept() {
    // A radio that cannot reach the band above 900 MHz.
    let mut sim = SimulatedDevice::holding_lists(
        SessionConfig {
            freq_khz_max: 900_000,
            ..config()
        },
        starting_keys(),
    );
    sim.open_device_interface();
    let sim = Rc::new(RefCell::new(sim));
    let (node, clock) = node(&sim);
    events(&node);
    let kept = node.radio().profile;
    let unreachable = VETTED
        .iter()
        .position(|profile| profile.freq_khz > 900_000)
        .unwrap();
    node.set_profile(unreachable);
    assert_eq!(node.radio().profile, Profile::Vetted(unreachable));
    clock.set(1_000);
    node.pump();
    assert_eq!(events(&node), [NodeEvent::SettingRefused(Setting::Profile)]);
    assert_eq!(node.radio().profile, kept);

    // A setting the device does not have is refused before it goes out.
    node.set_forwarding(true);
    assert_eq!(
        events(&node),
        [NodeEvent::SettingRefused(Setting::Forwarding)]
    );
    assert!(!node.radio().forwarding);
}

#[test]
fn brightness_and_the_keyboard_are_device_settings() {
    let sim = device();
    let (node, clock) = node(&sim);
    assert_eq!(node.brightness(), 113);
    assert_eq!(
        node.keyboard(),
        KeyboardSettings {
            capitalize: true,
            period: true,
            light: true
        }
    );
    let before = sim.borrow().saved_snapshot().unwrap().to_vec();
    node.set_brightness(0);
    let keyboard = KeyboardSettings {
        capitalize: false,
        period: true,
        light: false,
    };
    node.set_keyboard(keyboard);
    assert_eq!(node.brightness(), 0);
    assert_eq!(node.keyboard(), keyboard);
    clock.set(1_000);
    node.pump();
    assert_eq!(node.brightness(), 0);
    assert_eq!(node.keyboard(), keyboard);
    // Saved with the rest, as a switch at the device is.
    assert_ne!(sim.borrow().saved_snapshot().unwrap(), &before[..]);
}

/// What a phone attached to the device changes, the node hears.
#[test]
fn a_change_an_attached_phone_makes_reaches_the_node() {
    let sim = device();
    let (node, clock) = node(&sim);
    events(&node);
    sim.borrow_mut().attach();
    while sim.borrow_mut().take_outbound().is_some() {}

    let mut request = [0u8; 32];
    let len = frame::prop_set(&mut request, 2, prop::DEV_NAME, b"Willow").unwrap();
    let mut encoded = vec![0u8; hdlc::max_encoded_len(len)];
    let len = hdlc::encode_frame(&request[..len], &mut encoded).unwrap();
    sim.borrow_mut().ingest(&encoded[..len], 2_000).unwrap();

    clock.set(2_000);
    node.pump();
    assert_eq!(events(&node), [NodeEvent::SettingsChanged]);
    assert_eq!(node.device_name(), "Willow");
}

#[test]
fn the_address_and_share_link_come_from_the_device_identity() {
    let sim = device();
    {
        let (node, _) = node(&sim);
        assert_eq!(node.address(), "");
        assert!(!node.started());
        assert_eq!(node.share_uri(Shareable::Identity), None);
    }
    identify(&sim);
    let (node, _) = node(&sim);
    let address = node.address();
    assert_eq!(address.len(), 44);
    let uri = node.share_uri(Shareable::Identity).unwrap();
    assert_eq!(uri.as_str(), format!("umsh:n:{address}"));
    assert_eq!(node.share_uri(Shareable::Channel(1)), None);
}

// ─── Chats ───────────────────────────────────────────────────────────

/// A communicator starts out on `public` and `EMERGENCY`, and its chats
/// for them are there before anything arrives.
#[test]
fn a_communicator_starts_out_on_public_and_emergency() {
    let sim = device();
    let (node, _) = started(&sim);
    let names: Vec<String> = chats(&node)
        .iter()
        .map(|chat| chat.name.as_str().into())
        .collect();
    assert_eq!(names.len(), 2);
    assert!(names.contains(&"public".into()), "{names:?}");
    assert!(names.contains(&"EMERGENCY".into()), "{names:?}");
}

/// A message on `public` arrives in its chat, under the name its sender
/// gave, with what the radio said about it.
#[test]
fn a_public_message_arrives_with_its_sender_and_details() {
    let sim = device();
    let (node, clock) = started(&sim);
    let public = WellKnownChannels::derive().public;
    let robin = PublicKey([0x37; 32]);
    let payload = composed(robin, public, "Robin", "Anyone near the trailhead?");

    clock.set(5_000);
    node.arrive(Arrival::Text(received(&public, robin, &payload)));
    let arrived = events(&node);
    let Some(&NodeEvent::Arrived { chat, message }) = arrived
        .iter()
        .find(|event| matches!(event, NodeEvent::Arrived { .. }))
    else {
        panic!("nothing arrived: {arrived:?}");
    };
    assert!(arrived.contains(&NodeEvent::IdentitiesLearned));

    let summary = node.chat(chat).unwrap();
    assert_eq!(summary.name, "public");
    assert!(summary.unread);
    assert_eq!(summary.preview, "Robin: Anyone near the trailhead?");

    let shown = node.message(chat, message).unwrap();
    assert_eq!(&*shown.text, "Anyone near the trailhead?");
    assert_eq!(shown.sender.unwrap().name, "Robin");
    assert_eq!(shown.time, Some(STARTED + 5));

    let details = node.details(chat, message).unwrap();
    assert_eq!(details.time, "14:32");
    assert_eq!(details.rssi, Some(-97));
    assert_eq!(details.snr, Some(65));

    let member = node.member(chat, message).unwrap();
    assert_eq!(member.sender.name, "Robin");
    assert_eq!(member.address.unwrap().len(), 44);

    let mut messages = Messages::new();
    node.load(chat, &mut messages);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].id, message);

    node.opened(chat);
    assert!(!node.chat(chat).unwrap().unread);
}

/// A sender who gives no name is shown by their address.
#[test]
fn a_sender_without_a_name_is_shown_by_address() {
    let sim = device();
    let (node, _) = started(&sim);
    let public = WellKnownChannels::derive().public;
    let quiet = PublicKey([0x91; 32]);
    let payload = composed(quiet, public, "", "hello");
    node.arrive(Arrival::Text(received(&public, quiet, &payload)));
    let Some(NodeEvent::Arrived { chat, message }) = events(&node)
        .into_iter()
        .find(|event| matches!(event, NodeEvent::Arrived { .. }))
    else {
        panic!("nothing arrived");
    };
    let sender = node.message(chat, message).unwrap().sender.unwrap();
    assert!(sender.name.starts_with("Member "), "{}", sender.name);
}

/// A draft of only whitespace is not kept.
#[test]
fn drafts_are_kept_until_cleared() {
    let sim = device();
    let (node, _) = started(&sim);
    let chat = chats(&node)[0].id;
    node.save_draft(chat, "Back at six");
    assert_eq!(node.chat(chat).unwrap().draft.text(), "Back at six");
    node.save_draft(chat, "   ");
    assert_eq!(node.chat(chat).unwrap().draft.text(), "");
}

// ─── Sending ─────────────────────────────────────────────────────────

/// Robin's address, as typed.
fn address_of(key: &PublicKey) -> String {
    umsh_core::base58::encode(&key.0)
        .iter()
        .map(|&digit| char::from(digit))
        .collect()
}

/// A direct message is kept as Sending and handed to the device node; a
/// frame that cannot go makes it Unconfirmed, and a retry sends it again.
#[test]
fn a_direct_message_goes_out_and_can_be_retried() {
    let sim = device();
    let (node, _) = started(&sim);
    let robin = PublicKey([0x37; 32]);
    let avatar = node.avatar();
    let chat = node
        .direct_chat(&address_of(&robin), "Robin", &avatar)
        .unwrap();
    assert_eq!(chat.name, "Robin");
    assert_eq!(chat.kind, ChatKind::Peer { hops: None });
    events(&node);

    let message = node.send(chat.id, "On my way", None).unwrap();
    assert_eq!(message.delivery, Delivery::Sending);
    assert_eq!(message.direction, Direction::Outbound);
    let departure = node.depart().expect("the message leaves");
    assert!(node.depart().is_none(), "one frame");

    assert!(node.departed(departure, Sent::Unsendable));
    assert!(events(&node).contains(&NodeEvent::MessageChanged {
        chat: chat.id,
        message: message.id
    }));
    let failed = node.message(chat.id, message.id).unwrap();
    assert_eq!(failed.delivery, Delivery::Unconfirmed);

    assert!(node.retry(chat.id, message.id));
    assert_eq!(
        node.message(chat.id, message.id).unwrap().delivery,
        Delivery::Sending
    );
    let again = node.depart().expect("the message leaves again");
    assert!(node.departed(again, Sent::Unsendable));
    assert_eq!(
        node.message(chat.id, message.id).unwrap().delivery,
        Delivery::Unconfirmed
    );
}

/// A node over the store kept on `flash`, started.
fn node_on(
    sim: &Rc<RefCell<SimulatedDevice>>,
    flash: &SharedFlash,
) -> CommunicatorNode<Port, FlashMapStore<SharedFlash>> {
    let clock: &'static TestClock = Box::leak(Box::new(TestClock(AtomicU64::new(0))));
    let buffer = Box::leak(vec![0u8; 2048].into_boxed_slice());
    let size = flash.0.borrow().bytes().len() as u32;
    let store = FlashMapStore::mount(
        FlashMap::new(flash.clone(), 0..size, buffer),
        Quota::DEFAULT,
    )
    .unwrap();
    let node = CommunicatorNode::new(
        Port(sim.clone()),
        store,
        ChannelCrypto::software(),
        &std::alloc::System,
        clock,
        Seeds {
            session_id: 1,
            jitter_seed: 2,
        },
    );
    node.pump();
    assert!(node.started());
    while node.next_event().is_some() {}
    node
}

/// A node started again over the same flash has its chats, and carries a
/// conversation's sequence on rather than resetting it.
#[test]
fn a_restarted_node_keeps_its_chats_and_its_sequence() {
    let sim = device();
    identify(&sim);
    let flash = SharedFlash::new(64 * 4096);
    let robin = PublicKey([0x37; 32]);

    let first = node_on(&sim, &flash);
    let chat = first
        .direct_chat(&address_of(&robin), "Robin", &first.avatar())
        .unwrap();
    let said = first.send(chat.id, "On my way", None).unwrap();
    let departed = first.depart().unwrap();
    let before = umsh_text::codec::parse(departed.payload()).unwrap();
    let before = before.sequence.unwrap().message_id;
    drop(first);

    let second = node_on(&sim, &flash);
    let mut kept = Chats::new();
    second.chats(&mut kept);
    let chat = kept.iter().find(|kept| kept.name == "Robin").unwrap();
    let message = second.message(chat.id, said.id).unwrap();
    assert_eq!(&*message.text, "On my way");

    second.send(chat.id, "Almost there", None).unwrap();
    let departed = second.depart().unwrap();
    let after = umsh_text::codec::parse(departed.payload()).unwrap();
    assert!(!after.sequence_reset, "the stream goes on");
    assert_eq!(after.sequence.unwrap().message_id, before.wrapping_add(1));
}

/// A message on its way when the node stopped reads Unconfirmed once it
/// starts again, and a retry sends it under its wire ID, with word of it
/// reaching the message.
#[test]
fn a_message_cut_off_by_a_restart_can_be_retried() {
    let sim = device();
    identify(&sim);
    let flash = SharedFlash::new(64 * 4096);
    let robin = PublicKey([0x37; 32]);

    let first = node_on(&sim, &flash);
    let chat = first
        .direct_chat(&address_of(&robin), "Robin", &first.avatar())
        .unwrap();
    let said = first.send(chat.id, "On my way", None).unwrap();
    let departed = first.depart().unwrap();
    let wire = umsh_text::codec::parse(departed.payload()).unwrap();
    let wire = wire.sequence.unwrap().message_id;
    drop(first);

    let second = node_on(&sim, &flash);
    assert_eq!(
        second.message(chat.id, said.id).unwrap().delivery,
        Delivery::Unconfirmed
    );
    assert!(second.retry(chat.id, said.id));
    assert_eq!(
        second.message(chat.id, said.id).unwrap().delivery,
        Delivery::Sending
    );
    let again = second.depart().expect("the message leaves again");
    let resent = umsh_text::codec::parse(again.payload()).unwrap();
    assert_eq!(resent.sequence.unwrap().message_id, wire);
    assert!(second.departed(again, Sent::Unsendable));
    assert_eq!(
        second.message(chat.id, said.id).unwrap().delivery,
        Delivery::Unconfirmed
    );
}

/// Sending clears the chat's draft, and the chat moves to the top.
#[test]
fn sending_clears_the_draft() {
    let sim = device();
    let (node, _) = started(&sim);
    let chat = chats(&node)[1].id;
    node.save_draft(chat, "Back at six");
    let message = node.send(chat, "Back at six", None).unwrap();
    assert_eq!(&*message.text, "Back at six");
    let summary = node.chat(chat).unwrap();
    assert_eq!(summary.draft.text(), "");
    assert_eq!(chats(&node)[0].id, chat);
    // A channel message goes as one multicast.
    assert!(node.depart().is_some());
}

/// A reaction is kept on what it is about, and sent.
#[test]
fn a_reaction_is_kept_and_sent() {
    let sim = device();
    let (node, _) = started(&sim);
    let public = WellKnownChannels::derive().public;
    let robin = PublicKey([0x37; 32]);
    let payload = composed(robin, public, "Robin", "Anyone near the trailhead?");
    node.arrive(Arrival::Text(received(&public, robin, &payload)));
    let Some(NodeEvent::Arrived { chat, message }) = events(&node)
        .into_iter()
        .find(|event| matches!(event, NodeEvent::Arrived { .. }))
    else {
        panic!("nothing arrived");
    };
    node.react(chat, message, Some(Reaction::Heart));
    let reacted = node.message(chat, message).unwrap();
    assert!(
        reacted
            .reactions
            .iter()
            .any(|r| r.reaction == Reaction::Heart && r.mine)
    );
    assert!(node.depart().is_some());
}

// ─── Peers ───────────────────────────────────────────────────────────

/// How a frame came, one link away or more.
fn rx(hop_count: u8) -> RxSummary {
    RxSummary {
        rssi_dbm: Some(-97),
        snr_centibels: Some(65),
        lqi: None,
        hop_count: Some(hop_count),
        route_hints: Vec::new(),
        source_authenticated: true,
        buffered_age_s: 0,
    }
}

/// A chatting node's identity, named `name`.
fn chatter(name: &str) -> NodeIdentityPayload {
    let mut bytes = vec![
        NodeRole::Chat.as_byte(),
        NodeCapabilities::TEXT_MESSAGES.bits(),
        name.len() as u8,
    ];
    bytes.extend_from_slice(name.as_bytes());
    NodeIdentityPayload::from_bytes(&bytes).unwrap()
}

/// A node heard is found by a word of its name, until it is saved.
#[test]
fn a_heard_node_is_found_by_name() {
    let sim = device();
    let (node, clock) = started(&sim);
    let robin = PublicKey([0x37; 32]);
    clock.set(5_000);
    node.arrive(Arrival::Identity {
        key: robin,
        identity: chatter("Robin Ridge"),
        rx: rx(1),
    });
    clock.set(65_000);
    let found = node.find_heard("rid");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "Robin Ridge");
    assert_eq!(found[0].distance, "Nearby");
    assert_eq!(found[0].age, "1m");
    assert_eq!(found[0].address.as_str(), address_of(&robin));
    assert!(node.find_heard("trail").is_empty());

    // Its direct chat is called by its own name.
    let avatar = node.avatar();
    let chat = node
        .direct_chat(&address_of(&robin), "Robin Ridge", &avatar)
        .unwrap();
    assert_eq!(chat.name, "Robin Ridge");
    assert_eq!(chat.kind, ChatKind::Peer { hops: Some(0) });
}

/// A member saved is kept by the device, listed among the peers, and let
/// go of again.
#[test]
fn a_saved_member_is_kept_by_the_device() {
    let sim = device();
    let (node, clock) = started(&sim);
    let public = WellKnownChannels::derive().public;
    let robin = PublicKey([0x37; 32]);
    let payload = composed(robin, public, "Robin", "Anyone near the trailhead?");
    node.arrive(Arrival::Text(received(&public, robin, &payload)));
    let Some(NodeEvent::Arrived { chat, message }) = events(&node)
        .into_iter()
        .find(|event| matches!(event, NodeEvent::Arrived { .. }))
    else {
        panic!("nothing arrived");
    };
    let member = node.member(chat, message).unwrap();
    assert!(!member.saved);
    let summary = node.save_peer(&member).unwrap();
    assert_eq!(summary.name, "Robin");
    assert!(node.member(chat, message).unwrap().saved);

    clock.set(1_000);
    node.pump();
    let listed = peers(&node);
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, summary.id);
    // Another node over the device reads it there.
    assert_eq!(peers(&node_over(&sim)).len(), 1);
    let detail = node.peer_detail(summary.id).unwrap();
    assert_eq!(detail.address.as_str(), address_of(&robin));
    events(&node);

    node.forget_peer(summary.id);
    assert_eq!(events(&node), [NodeEvent::PeerRemoved(summary.id)]);
    clock.set(2_000);
    node.pump();
    assert!(peers(&node).is_empty());
    assert!(peers(&node_over(&sim)).is_empty());
    assert!(!node.member(chat, message).unwrap().saved);
}

/// A peer a phone saves on the device shows among the peers.
#[test]
fn a_peer_saved_elsewhere_is_listed() {
    let sim = device();
    let (node, clock) = started(&sim);
    let robin = PublicKey([0x37; 32]);
    phone_inserts(&sim, prop::DEV_PEERS, &robin.0);
    clock.set(2_000);
    node.pump();
    let peers = peers(&node);
    assert_eq!(peers.len(), 1);
    assert!(events(&node).contains(&NodeEvent::PeerChanged(peers[0].id)));
}

/// A member known only by their hint is asked about on their channel,
/// and known by name and address once they answer.
#[test]
fn a_member_known_by_hint_is_asked_for() {
    let sim = device();
    let (node, clock) = started(&sim);
    let crew = ChannelKey([0x5C; 32]);
    phone_inserts(&sim, prop::DEV_CHANNEL_KEYS, &crew.0);
    clock.set(1_000);
    node.pump();
    let robin = PublicKey([0x37; 32]);
    let payload = composed(robin, crew, "", "hello");
    node.arrive(Arrival::Text(channel_frame(&crew, robin, false, &payload)));
    let Some(NodeEvent::Arrived { chat, message }) = events(&node)
        .into_iter()
        .find(|event| matches!(event, NodeEvent::Arrived { .. }))
    else {
        panic!("nothing arrived");
    };
    assert_eq!(node.member(chat, message).unwrap().address, None);

    node.request_identity(chat, message);
    let Some(Command::AskChannel { channel, hint, .. }) = node.command() else {
        panic!("nothing asked");
    };
    assert_eq!(channel, crew);
    assert_eq!(hint, robin.hint());

    clock.set(3_000);
    node.arrive(Arrival::Identity {
        key: robin,
        identity: chatter("Robin"),
        rx: rx(2),
    });
    assert!(events(&node).contains(&NodeEvent::IdentitiesLearned));
    let member = node.member(chat, message).unwrap();
    assert_eq!(member.address.as_deref(), Some(address_of(&robin).as_str()));
}

/// Add peer reads an address as far as it goes, and a whole one as the
/// node it names.
#[test]
fn an_address_is_read_as_it_is_typed() {
    let sim = device();
    let (node, _) = started(&sim);
    let robin = PublicKey([0x37; 32]);
    let address = address_of(&robin);
    assert_eq!(node.inspect_peer(""), PeerEntry::Empty);
    assert_eq!(node.inspect_peer("0OIl"), PeerEntry::Invalid);
    let PeerEntry::Partial { missing, .. } = node.inspect_peer(&address[..10]) else {
        panic!("not partial");
    };
    assert_eq!(usize::from(missing), address.len() - 10);
    let PeerEntry::Complete {
        address: whole,
        name,
        ..
    } = node.inspect_peer(&format!("umsh:n:{address}"))
    else {
        panic!("not complete");
    };
    assert_eq!(whole.as_str(), address);
    assert_eq!(name, None);
}

/// A phone attached to `sim` inserts `item` into the table `key`.
fn phone_inserts(sim: &Rc<RefCell<SimulatedDevice>>, key: u32, item: &[u8]) {
    sim.borrow_mut().attach();
    while sim.borrow_mut().take_outbound().is_some() {}
    let mut request = [0u8; 64];
    let len = frame::prop_insert(&mut request, 2, key, item).unwrap();
    let mut encoded = vec![0u8; hdlc::max_encoded_len(len)];
    let len = hdlc::encode_frame(&request[..len], &mut encoded).unwrap();
    sim.borrow_mut().ingest(&encoded[..len], 0).unwrap();
}

/// A second node over `sim`, as a reboot would start one.
fn node_over(sim: &Rc<RefCell<SimulatedDevice>>) -> CommunicatorNode<Port> {
    let (node, _) = node(sim);
    node.pump();
    node
}

fn peers(node: &CommunicatorNode<Port>) -> Peers {
    let mut peers = Peers::new();
    node.peers(&mut peers);
    peers
}
