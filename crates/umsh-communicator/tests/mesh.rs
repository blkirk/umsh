//! Two communicators on a simulated radio link, each over a real MAC and
//! its own soft device, as two boards would run: what one sends, the other
//! keeps, and the sender sees acknowledged.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use tokio::task::{LocalSet, spawn_local};
use umsh::crypto::software::{SoftwareAes, SoftwareIdentity, SoftwareSha256};
use umsh::crypto::{CryptoEngine, NodeIdentity};
use umsh::mac::test_support::{SimulatedNetwork, SimulatedRadio};
use umsh::mac::{Mac, MacHandle, OperatingPolicy, RepeaterConfig};
use umsh::node::{Host, LocalNode, PfsStatus};
use umsh::tokio_support::{MemoryCounterStore, MemoryKeyValueStore, StdClock, TokioPlatform};
use umsh_communicator::task::{self, Inbox};
use umsh_communicator::{
    ChannelCrypto, Clock, CommunicatorNode, MemoryStore, Seeds, WellKnownChannels,
};
use umsh_core::PublicKey;
use umsh_sync::AsyncRefCell;
use umsh_ulcp::ids::prop;
use umsh_ulcp_simdev::SimulatedDevice;
use umsh_ux_communicator::model::{ChatKind, Delivery, Node, NodeEvent, Peers};

mod support;

use support::{Port, device, phone_inserts, provision};

type Platform = TokioPlatform<SimulatedRadio, MemoryCounterStore, MemoryKeyValueStore>;
/// Room for the board's identity and the ephemeral ones its sessions with
/// forward secrecy take, as the device node has.
type MeshMac = Mac<Platform, 5, 16, 4, 8, 8, 255, 32>;

/// A test that stops making progress fails rather than hangs.
const PATIENCE: Duration = Duration::from_secs(20);

/// How long one turn of a device node's pump waits for its radio.
const TURN: Duration = Duration::from_millis(5);

/// The board's clock, from when the test started.
struct Since(Instant);

impl Clock for Since {
    fn now_ms(&self) -> u64 {
        self.0.elapsed().as_millis() as u64
    }

    fn wall(&self) -> Option<u32> {
        Some(1_792_247_520 + self.0.elapsed().as_secs() as u32)
    }

    fn utc_offset_min(&self) -> i16 {
        0
    }
}

/// One board: its communicator's node, the soft device it holds its
/// settings on, the device node, and the device identity's key.
struct Board {
    node: &'static CommunicatorNode<Port>,
    sim: Rc<RefCell<SimulatedDevice>>,
    mesh: LocalNode<MacHandle<'static, Platform, 5, 16, 4, 8, 8, 255, 32>>,
    key: PublicKey,
}

/// Brings a board up on `radio` as the identity `secret` makes: the soft
/// device holds it, and the device node's MAC runs as it.
async fn board(radio: SimulatedRadio, secret: [u8; 32]) -> Board {
    let sim = device();
    provision(&sim, Some(&secret));
    let clock: &'static Since = Box::leak(Box::new(Since(Instant::now())));
    let node: &'static CommunicatorNode<Port> = Box::leak(Box::new(CommunicatorNode::new(
        Port(sim.clone()),
        MemoryStore::new(),
        ChannelCrypto::software(),
        &std::alloc::System,
        clock,
        Seeds {
            session_id: u64::from(secret[0]),
            jitter_seed: 2,
        },
    )));
    node.pump();
    assert!(node.started());

    let identity = SoftwareIdentity::from_secret_bytes(&secret);
    let key = *identity.public_key();
    let mac: &'static AsyncRefCell<MeshMac> = Box::leak(Box::new(AsyncRefCell::new(Mac::new(
        radio,
        CryptoEngine::new(SoftwareAes, SoftwareSha256),
        StdClock::new(),
        rand::rng(),
        MemoryCounterStore::default(),
        RepeaterConfig::default(),
        OperatingPolicy::default(),
    ))));
    let handle = MacHandle::new(mac);
    handle.set_auto_register_full_key_peers(true).await;
    let id = handle.add_identity(identity).await.unwrap();
    let mut host = Host::new(handle);
    let mesh = host.add_node(id);

    let inbox: &'static Inbox<NoopRawMutex> = Box::leak(Box::new(Inbox::new()));
    std::mem::forget(task::listen(&mesh, inbox, key, WellKnownChannels::derive()));
    // The simulated radio wakes no one when a frame reaches it, so the
    // pump is polled again every few milliseconds.
    spawn_local(async move {
        loop {
            if let Ok(pumped) = tokio::time::timeout(TURN, host.pump_once()).await {
                pumped.unwrap();
            }
            handle.service_counter_persistence().await.unwrap();
        }
    });
    let board = Board {
        node,
        sim,
        mesh: mesh.clone(),
        key,
    };
    spawn_local(async move {
        task::run(
            node,
            &mesh,
            inbox,
            |at| tokio::time::sleep(Duration::from_millis(at.saturating_sub(clock.now_ms()))),
            || {},
        )
        .await;
    });
    board
}

fn address(key: &PublicKey) -> String {
    umsh_core::base58::encode(&key.0)
        .iter()
        .map(|&digit| char::from(digit))
        .collect()
}

/// Waits until `done` holds, failing the test if it never does.
async fn until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + PATIENCE;
    while !done() {
        assert!(Instant::now() < deadline, "{what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// The first direct message to a node that has never heard of us arrives
/// and is acknowledged, and so does the answer.
#[tokio::test(flavor = "current_thread")]
async fn a_direct_message_is_delivered() {
    LocalSet::new()
        .run_until(async {
            let network = SimulatedNetwork::new();
            let near = network.add_radio_with_config(255, 20);
            let far = network.add_radio_with_config(255, 20);
            network.connect_bidirectional(near.id(), far.id());
            let alice = board(near, [0x11; 32]).await;
            let bob = board(far, [0x22; 32]).await;
            while bob.node.next_event().is_some() {}

            let chat = alice
                .node
                .direct_chat(&address(&bob.key), "Bob", &alice.node.avatar())
                .unwrap();
            let sent = alice
                .node
                .send(chat.id, "Meet at the ridge", None, false)
                .unwrap();
            let mut arrived = None;
            until("the message was not delivered", || {
                while let Some(event) = bob.node.next_event() {
                    if let NodeEvent::Arrived { chat, message } = event {
                        arrived = Some((chat, message));
                    }
                }
                let delivery = alice.node.message(chat.id, sent.id).unwrap().delivery;
                arrived.is_some() && delivery == Delivery::Delivered
            })
            .await;
            let (chat, message) = arrived.unwrap();
            let message = bob.node.message(chat, message).unwrap();
            assert_eq!(&*message.text, "Meet at the ridge");
            assert_eq!(
                bob.node.chat(chat).unwrap().kind,
                ChatKind::Peer { hops: Some(0) }
            );

            // The answer goes back the same way.
            while alice.node.next_event().is_some() {}
            let answer = bob.node.send(chat, "On my way", None, false).unwrap();
            let mut answered = false;
            until("the answer was not delivered", || {
                while let Some(event) = alice.node.next_event() {
                    answered |= matches!(event, NodeEvent::Arrived { .. });
                }
                let delivery = bob.node.message(chat, answer.id).unwrap().delivery;
                answered && delivery == Delivery::Delivered
            })
            .await;
        })
        .await;
}

/// Waits until `alice` and `bob` both hold a session with forward secrecy
/// with each other, or neither does.
async fn until_secrecy(alice: &Board, bob: &Board, held: bool) {
    let deadline = Instant::now() + PATIENCE;
    loop {
        let alice_holds = alice.mesh.pfs_status(&bob.key).await.unwrap();
        let bob_holds = bob.mesh.pfs_status(&alice.key).await.unwrap();
        let active = |status: &PfsStatus| matches!(status, PfsStatus::Active { .. });
        let inactive = |status: &PfsStatus| matches!(status, PfsStatus::Inactive);
        let done = if held {
            active(&alice_holds) && active(&bob_holds)
        } else {
            inactive(&alice_holds) && inactive(&bob_holds)
        };
        if done {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "forward secrecy held: {alice_holds:?} and {bob_holds:?}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// A saved peer who prefers forward secrecy is asked for a session, which
/// both boards then hold, the chat shows, and what is said goes through
/// under; it ends when they no longer prefer one. Only what was sent while
/// the chat showed the session says it went under one.
#[tokio::test(flavor = "current_thread")]
async fn forward_secrecy_is_held_with_a_peer_who_prefers_it() {
    LocalSet::new()
        .run_until(async {
            let network = SimulatedNetwork::new();
            let near = network.add_radio_with_config(255, 20);
            let far = network.add_radio_with_config(255, 20);
            network.connect_bidirectional(near.id(), far.id());
            let alice = board(near, [0x11; 32]).await;
            let bob = board(far, [0x22; 32]).await;

            phone_inserts(&alice.sim, prop::DEV_PEERS, &bob.key.0);
            alice.node.pump();
            let mut peers = Peers::new();
            alice.node.peers(&mut peers);
            let peer = peers[0].id;
            let chat = alice
                .node
                .direct_chat(&address(&bob.key), "Bob", &alice.node.avatar())
                .unwrap();
            let shown = || alice.node.chat(chat.id).unwrap().forward_secret;
            let say = async |text| {
                let sent = alice.node.send(chat.id, text, None, shown()).unwrap();
                until("the message was not delivered", || {
                    alice.node.message(chat.id, sent.id).unwrap().delivery == Delivery::Delivered
                })
                .await;
                sent.id
            };
            let secret = |id| alice.node.message(chat.id, id).unwrap().forward_secret;

            let before = say("Leaving now").await;
            assert!(!secret(before));

            alice.node.set_forward_secrecy(peer, true);
            until_secrecy(&alice, &bob, true).await;
            until("the chat does not show the session", shown).await;
            let during = say("Meet at the ridge").await;
            assert!(secret(during));

            alice.node.set_forward_secrecy(peer, false);
            assert!(!shown(), "the chat shows the session ended at once");
            until_secrecy(&alice, &bob, false).await;
            let after = say("Almost there").await;
            assert!(!secret(after));
            assert!(!secret(before) && secret(during), "each keeps its own");
        })
        .await;
}
