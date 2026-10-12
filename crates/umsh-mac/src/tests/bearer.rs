//! Forwarding across bearers (lan-bearer.md § Forwarding Across Bearers,
//! § Channel Access).

use super::*;
use umsh_hal::BearerId;

type BearerMac = Mac<
    DummyPlatform,
    4,
    16,
    8,
    16,
    16,
    256,
    64,
    { crate::DEFAULT_CHANNEL_REPLAY },
    { crate::DEFAULT_CHANNEL_HINT_REPLAY },
    0,
    8,
>;

const LAN: BearerId = BearerId::LAN;

fn second_bearer() -> BearerId {
    BearerId::new(1).unwrap()
}

/// A node with an identity (so it has a router hint to forward under),
/// the repeater as given, and the LAN bearer enabled.
fn make_bearer_mac(repeater: bool) -> BearerMac {
    let mut mac = Mac::new(
        DummyRadio::default(),
        CryptoEngine::new(DummyAes, DummySha),
        DummyClock {
            now_ms: Cell::new(123),
        },
        DummyRng(7),
        DummyCounterStore::default(),
        RepeaterConfig::default(),
        OperatingPolicy::default(),
    );
    mac.repeater_config_mut().enabled = repeater;
    mac.set_bearer_enabled(LAN, true);
    mac.add_identity(DummyIdentity::new([0x10; 32])).unwrap();
    mac
}

fn flood_broadcast(hops: u8) -> heapless::Vec<u8, 256> {
    flood_broadcast_carrying(hops, b"bridge me")
}

fn flood_broadcast_carrying(hops: u8, payload: &[u8]) -> heapless::Vec<u8, 256> {
    let source = DummyIdentity::new([0xAB; 32]);
    let mut buf = [0u8; 256];
    let frame = PacketBuilder::new(&mut buf)
        .broadcast()
        .source_full(source.public_key())
        .flood_hops(hops)
        .payload(payload)
        .build()
        .unwrap();
    frame.iter().copied().collect()
}

fn receive(mac: &mut BearerMac, frame: &[u8], origin: RxOrigin) {
    mac.radio_mut().rx_origin = origin;
    mac.radio_mut().queue_received_frame(frame);
    let _ = block_on(mac.receive_one(|_, _| {})).unwrap();
}

fn take_export(mac: &mut BearerMac, bearer: BearerId) -> Option<heapless::Vec<u8, 256>> {
    let mut buf = [0u8; 256];
    let len = mac.pop_bearer_export(bearer, &mut buf)?;
    Some(buf[..len].iter().copied().collect())
}

fn remaining_hops(frame: &[u8]) -> u8 {
    PacketHeader::parse(frame)
        .unwrap()
        .flood_hops
        .unwrap()
        .remaining()
}

#[test]
fn a_frame_heard_on_the_air_goes_to_the_radio_and_the_bearer_as_one_rewrite() {
    let mut mac = make_bearer_mac(true);
    let frame = flood_broadcast(4);
    receive(&mut mac, &frame, RxOrigin::Air);

    let repeat = mac.tx_queue_mut().pop_next().expect("repeated on the air");
    let export = take_export(&mut mac, LAN).expect("offered to the LAN");
    assert_eq!(repeat.frame.as_slice(), export.as_slice());
    assert_eq!(remaining_hops(&export), 3, "one hop spent, once");
    assert!(take_export(&mut mac, LAN).is_none());
    assert_eq!(mac.counters().forwarded, 1);
    assert_eq!(mac.counters().bearer_exported, 1);
}

/// The bearer's switch alone bridges: with the repeater off, the node
/// carries frames between the radio and the bearer but not radio to radio.
#[test]
fn a_bearer_without_the_repeater_bridges_but_does_not_repeat() {
    let mut mac = make_bearer_mac(false);
    receive(&mut mac, &flood_broadcast(4), RxOrigin::Air);
    assert!(mac.tx_queue().is_empty(), "the repeater is off");
    assert!(take_export(&mut mac, LAN).is_some());

    receive(
        &mut mac,
        &flood_broadcast_carrying(4, b"from the LAN"),
        RxOrigin::Bearer(LAN),
    );
    assert!(mac.tx_queue_mut().pop_next().is_some(), "LAN to radio");
    assert!(take_export(&mut mac, LAN).is_none());
}

#[test]
fn a_frame_from_a_bearer_never_goes_back_to_it() {
    let mut mac = make_bearer_mac(true);
    mac.set_bearer_enabled(second_bearer(), true);
    receive(&mut mac, &flood_broadcast(4), RxOrigin::Bearer(LAN));

    assert!(mac.tx_queue_mut().pop_next().is_some());
    assert!(take_export(&mut mac, LAN).is_none());
    let onward = take_export(&mut mac, second_bearer()).expect("a different bearer carries it");
    assert_eq!(remaining_hops(&onward), 3);
}

#[test]
fn a_disabled_bearer_bridges_nothing() {
    let mut mac = make_bearer_mac(true);
    mac.set_bearer_enabled(LAN, false);
    receive(&mut mac, &flood_broadcast(4), RxOrigin::Bearer(LAN));
    assert!(mac.tx_queue().is_empty());

    receive(
        &mut mac,
        &flood_broadcast_carrying(4, b"from the air"),
        RxOrigin::Air,
    );
    assert!(mac.tx_queue_mut().pop_next().is_some());
    assert!(take_export(&mut mac, LAN).is_none());
    assert_eq!(mac.counters().bearer_exported, 0);
}

#[test]
fn a_backhauled_frame_is_exported_and_repeated_without_contention() {
    let mut mac = make_bearer_mac(true);
    receive(&mut mac, &flood_broadcast(4), RxOrigin::Backhaul);
    let repeat = mac.tx_queue_mut().pop_next().unwrap();
    assert_eq!(repeat.not_before_ms, mac.clock().now_ms());
    assert!(take_export(&mut mac, LAN).is_some());
}

/// Every radio on the bearer heard the datagram, so they contend for the
/// channel—with nothing measured to rank them, uniformly over the window.
#[test]
fn bearer_receptions_contend_over_the_whole_window() {
    let t_frame_ms = 100;
    let mut min_seen = u64::MAX;
    let mut max_seen = 0;
    for seed in 0..64 {
        let mut mac = make_bearer_mac(false);
        *mac.rng_mut() = DummyRng(seed);
        {
            let repeater = mac.repeater_config_mut();
            repeater.flood_contention_min_window_percent = 20;
            repeater.flood_contention_max_window_percent = 60;
            repeater.flood_contention_jitter_percent = 10;
        }
        let now_ms = mac.clock().now_ms();
        receive(&mut mac, &flood_broadcast(4), RxOrigin::Bearer(LAN));
        let delay = mac.tx_queue_mut().pop_next().unwrap().not_before_ms - now_ms;
        min_seen = min_seen.min(delay);
        max_seen = max_seen.max(delay);
    }
    assert!(min_seen >= t_frame_ms * 20 / 100, "{min_seen}");
    assert!(max_seen <= t_frame_ms * 70 / 100, "{max_seen}");
    assert!(max_seen > min_seen, "the window is sampled, not fixed");
}

/// A repeat this node loses to another repeater on the air is withdrawn,
/// but the bearer was offered the frame at the decision and keeps it.
#[test]
fn a_suppressed_repeat_is_still_bridged() {
    let mut mac = make_bearer_mac(true);
    let frame = flood_broadcast(4);
    receive(&mut mac, &frame, RxOrigin::Air);
    for _ in 0..=mac.repeater_config().flood_contention_max_deferrals {
        receive(&mut mac, &frame, RxOrigin::Air);
    }
    assert!(mac.tx_queue().is_empty(), "the repeat was suppressed");
    assert!(take_export(&mut mac, LAN).is_some());
    assert!(take_export(&mut mac, LAN).is_none(), "and offered once");
}

/// Only a copy heard on the radio channel says anything about who will
/// hear this node's repeat on it.
#[test]
fn only_an_over_the_air_copy_defers_a_pending_repeat() {
    for origin in [RxOrigin::Bearer(LAN), RxOrigin::Backhaul] {
        let mut mac = make_bearer_mac(true);
        let frame = flood_broadcast(4);
        receive(&mut mac, &frame, RxOrigin::Air);
        let scheduled = mac.tx_queue_mut().pop_next().unwrap();
        mac.tx_queue_mut()
            .enqueue_with_state(
                scheduled.priority,
                scheduled.frame.as_slice(),
                None,
                None,
                scheduled.not_before_ms,
                0,
                0,
            )
            .unwrap();

        mac.clock().advance_ms(5);
        receive(&mut mac, &frame, origin);
        let pending = mac.tx_queue_mut().pop_next().unwrap();
        assert_eq!(pending.forward_deferrals, 0, "{origin:?} deferred");
        assert_eq!(pending.not_before_ms, scheduled.not_before_ms);
    }
}

fn unicast_mac() -> (BearerMac, LocalIdentityId, PublicKey) {
    let mut mac = make_bearer_mac(false);
    let local_id = LocalIdentityId(0);
    let peer_key = test_pubkey(0xAB);
    let peer_id = mac.add_peer(peer_key).unwrap();
    mac.install_pairwise_keys(
        local_id,
        peer_id,
        PairwiseKeys {
            k_enc: [1; 32],
            k_mic: [2; 32],
        },
    )
    .unwrap();
    (mac, local_id, peer_key)
}

#[test]
fn a_bearer_copy_does_not_confirm_a_forwarded_send() {
    let (mut mac, local_id, peer_key) = unicast_mac();
    let route = [RouterHint([1, 2])];
    let options = SendOptions::default()
        .with_ack_requested(true)
        .try_with_source_route(&route)
        .unwrap();
    let receipt = mac
        .queue_unicast(local_id, &peer_key, b"hello", &options)
        .unwrap()
        .unwrap();
    let _ = block_on(mac.transmit_next(&mut |_, _| {})).unwrap();
    let original = mac
        .identity(local_id)
        .unwrap()
        .pending_ack(&receipt)
        .unwrap()
        .resend
        .frame
        .clone();
    let forwarded = rewrite_forwarded_fixture(original.as_slice());

    receive(&mut mac, &forwarded, RxOrigin::Bearer(LAN));
    let pending = mac
        .identity(local_id)
        .unwrap()
        .pending_ack(&receipt)
        .unwrap();
    assert!(
        matches!(pending.state, AckState::AwaitingForward { .. }),
        "{:?}",
        pending.state
    );

    receive(&mut mac, &forwarded, RxOrigin::Air);
    let pending = mac
        .identity(local_id)
        .unwrap()
        .pending_ack(&receipt)
        .unwrap();
    assert_eq!(pending.state, AckState::AwaitingAck);
}

/// A node's own frames go to its bearers when first queued, not again when
/// retransmitted: every node on the bearer already has them.
#[test]
fn an_originated_frame_is_exported_once_and_its_retries_never() {
    let (mut mac, local_id, peer_key) = unicast_mac();
    let options = SendOptions::default()
        .with_ack_requested(true)
        .with_flood_hops(4);
    let receipt = mac
        .queue_unicast(local_id, &peer_key, b"hello", &options)
        .unwrap()
        .unwrap();
    let export = take_export(&mut mac, LAN).expect("the first transmission is exported");
    let _ = block_on(mac.transmit_next(&mut |_, _| {})).unwrap();
    assert_eq!(mac.radio().transmitted[0].as_slice(), export.as_slice());

    // Let the forward-confirmation deadline lapse so a retry is queued.
    for _ in 0..50 {
        mac.clock().advance_ms(1_000);
        mac.service_pending_ack_timeouts(|_, _| {}).unwrap();
        if mac.tx_queue().len() > 0 {
            break;
        }
    }
    let retry = mac.tx_queue_mut().pop_next().expect("a retry is queued");
    assert_eq!(retry.priority, TxPriority::Retry);
    assert!(
        mac.identity(local_id)
            .unwrap()
            .pending_ack(&receipt)
            .is_some()
    );
    assert!(take_export(&mut mac, LAN).is_none(), "a retry was exported");
}

#[test]
fn an_attached_hosts_transmission_is_exported_as_this_nodes_own() {
    let mut mac = make_bearer_mac(true);
    let frame = flood_broadcast(4);
    receive(&mut mac, &frame, RxOrigin::LocalTx);
    assert!(mac.tx_queue().is_empty(), "never repeated");
    let export = take_export(&mut mac, LAN).expect("exported");
    assert_eq!(export.as_slice(), frame.as_slice(), "exactly as it aired");

    // A neighbor's repeat of it is already known.
    receive(&mut mac, &frame, RxOrigin::Air);
    assert!(mac.tx_queue().is_empty());
    assert!(take_export(&mut mac, LAN).is_none());
}

#[test]
fn disabling_a_bearer_forgets_what_was_waiting_for_it() {
    let mut mac = make_bearer_mac(true);
    receive(&mut mac, &flood_broadcast(4), RxOrigin::Air);
    mac.set_bearer_enabled(LAN, false);
    mac.set_bearer_enabled(LAN, true);
    assert!(take_export(&mut mac, LAN).is_none());
}

// ─── Topology ────────────────────────────────────────────────────────────────

type BridgeMac = Mac<
    crate::test_support::ModeledTestPlatform,
    4,
    16,
    8,
    16,
    16,
    256,
    64,
    { crate::DEFAULT_CHANNEL_REPLAY },
    { crate::DEFAULT_CHANNEL_HINT_REPLAY },
    0,
    8,
>;

/// Two LoRa channels joined by one LAN.
///
/// - Channel X: endpoint 0, and radios 1 and 2.
/// - Channel Y: radio 3, and endpoint 4.
/// - Radios 1, 2, and 3 share the LAN.
struct Bridged {
    network: crate::test_support::ModeledNetwork,
    macs: Vec<RefCell<BridgeMac>>,
    ids: Vec<LocalIdentityId>,
    keys: Vec<PublicKey>,
}

const RADIOS: [usize; 3] = [1, 2, 3];

impl Bridged {
    fn new() -> Self {
        let clock = crate::test_support::DummyClock::new(0);
        let network = crate::test_support::ModeledNetwork::with_clock(clock.clone());
        let mut macs = Vec::new();
        let mut ids = Vec::new();
        let mut keys = Vec::new();
        for index in 0..5u8 {
            let radio = network.add_radio_with_config(256, 50);
            let mut mac: BridgeMac = Mac::new(
                radio,
                CryptoEngine::new(crate::test_support::DummyAes, crate::test_support::DummySha),
                clock.clone(),
                crate::test_support::DummyRng(0x31 + index * 17),
                crate::test_support::DummyCounterStore,
                RepeaterConfig::default(),
                OperatingPolicy::default(),
            );
            if RADIOS.contains(&usize::from(index)) {
                mac.repeater_config_mut().enabled = true;
                mac.set_bearer_enabled(LAN, true);
            }
            let id = mac
                .add_identity(crate::test_support::DummyIdentity::new([0x40 + index; 32]))
                .unwrap();
            keys.push(*mac.identity(id).unwrap().identity().public_key());
            ids.push(id);
            macs.push(RefCell::new(mac));
        }
        for (a, b) in [(0, 1), (0, 2), (1, 2), (3, 4)] {
            connect_modeled_bidirectional(&network, a, b);
        }
        let bridged = Self {
            network,
            macs,
            ids,
            keys,
        };
        let pairwise = PairwiseKeys {
            k_enc: [0x21; 32],
            k_mic: [0x42; 32],
        };
        for (me, them) in [(0, 4), (4, 0)] {
            let mut mac = bridged.macs[me].borrow_mut();
            let peer = mac.add_peer(bridged.keys[them]).unwrap();
            mac.install_pairwise_keys(bridged.ids[me], peer, pairwise.clone())
                .unwrap();
        }
        bridged.network.advance_ms(0);
        bridged
    }

    /// Carry every exported frame to every other radio on the LAN.
    fn run_lan(&self) {
        let mut buf = [0u8; 256];
        for &from in &RADIOS {
            while let Some(len) = self.macs[from]
                .borrow_mut()
                .pop_bearer_export(LAN, &mut buf)
            {
                for &to in RADIOS.iter().filter(|&&to| to != from) {
                    self.network.deliver_from_bearer(to, &buf[..len], LAN);
                }
            }
        }
    }

    /// Run the mesh for `steps` five-millisecond steps, reporting every
    /// event by node.
    fn run(&self, steps: usize, mut on_event: impl FnMut(usize, &MacEventRef<'_>)) {
        for _ in 0..steps {
            for (node, mac) in self.macs.iter().enumerate() {
                block_on(
                    mac.borrow_mut()
                        .poll_cycle(|_, event| on_event(node, &event)),
                )
                .unwrap();
            }
            self.run_lan();
            self.network.advance_ms(5);
        }
    }
}

#[test]
fn a_direct_message_crosses_the_lan_in_each_direction_once() {
    let mesh = Bridged::new();
    let receipt = mesh.macs[0]
        .borrow_mut()
        .queue_unicast(
            mesh.ids[0],
            &mesh.keys[4],
            b"across",
            &SendOptions::default()
                .with_ack_requested(true)
                .with_flood_hops(4),
        )
        .unwrap()
        .unwrap();

    let mut delivered_hops = Vec::new();
    let mut acknowledged = false;
    mesh.run(2_000, |node, event| match event {
        MacEventRef::AckReceived { receipt: got, .. } if node == 0 => {
            assert_eq!(*got, receipt);
            acknowledged = true;
        }
        MacEventRef::AckTimeout { .. } => panic!("timed out"),
        _ => {
            if node == 4
                && let Some(packet) = received_of_type(event, PacketType::Unicast)
            {
                delivered_hops.push(packet.flood_hops().unwrap().remaining());
            }
        }
    });

    // One copy reaches the far endpoint, two flood hops into its budget:
    // one at the radio that heard it on X, one at the radio on Y.
    assert_eq!(delivered_hops, [2]);
    assert!(acknowledged);

    // Both radios on X bridged the frame, since either may be the only one
    // that does; the radio on Y carried exactly one copy of it.
    let exported: Vec<u32> = RADIOS
        .iter()
        .map(|&radio| mesh.macs[radio].borrow().counters().bearer_exported)
        .collect();
    assert!(exported[0] >= 1 && exported[1] >= 1, "{exported:?}");
}

/// A frame bridged onto a channel with two radios on it is staggered, not
/// transmitted by both at once: every radio on the LAN heard the datagram
/// at the same instant, and only contention keeps their repeats apart.
#[test]
fn radios_bridging_onto_one_channel_contend_instead_of_colliding() {
    let mesh = Bridged::new();
    mesh.macs[4]
        .borrow_mut()
        .queue_broadcast(
            mesh.ids[4],
            b"from Y",
            &SendOptions::default().with_flood_hops(4),
        )
        .unwrap();

    let mut heard_on_x = 0;
    mesh.run(400, |node, event| {
        if node == 0 && is_received_type(event, PacketType::Broadcast) {
            heard_on_x += 1;
        }
    });

    assert!(heard_on_x >= 1, "both repeats collided at the endpoint");
    for radio in [1, 2] {
        let mac = mesh.macs[radio].borrow();
        assert!(mac.counters().tx_frames <= 1, "repeated at most once");
        assert_eq!(
            mac.counters().bearer_exported,
            0,
            "bearer input went back to the bearer"
        );
    }
}
