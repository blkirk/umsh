//! Engine tests: the interoperability vectors, then the local behavior
//! the vectors cannot see.

use serde_json::Value;
use umsh_crypto::Sha256Provider;
use umsh_crypto::software::SoftwareSha256;
use umsh_hal::Snr;

use super::*;
use crate::packet::{self, Packet, RouteType, payload_type};

const VECTORS: &str = include_str!("../vectors/vectors.json");

/// The vectors' fixed local policy: four milliseconds per octet, and the
/// largest draw, so a randomized delay lands on its window's upper bound.
struct VectorEnv;

impl Environment for VectorEnv {
    fn airtime_ms(&self, len: usize) -> u32 {
        4 * len as u32
    }

    fn random_u32(&mut self) -> u32 {
        u32::MAX
    }
}

fn hex(value: &Value) -> std::vec::Vec<u8> {
    let text = value.as_str().expect("hex string");
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}

fn heard(value: &Value) -> Heard {
    match value {
        Value::String(kind) if kind == "local" => Heard::LocalTx,
        Value::String(kind) if kind == "backhaul" => Heard::Backhaul,
        Value::Object(map) => {
            let air = &map["air"];
            Heard::Air {
                rssi: air["rssi"].as_i64().unwrap() as i16,
                snr: Snr::from_centibels(air["snr_centibels"].as_i64().unwrap() as i16),
            }
        }
        other => panic!("unknown origin {other}"),
    }
}

fn declined(reason: &str) -> Declined {
    match reason {
        "not_next_hop" => Declined::NotNextHop,
        "trace_complete" => Declined::TraceComplete,
        "zero_hop" => Declined::ZeroHop,
        "local_control" => Declined::LocalControl,
        "payload_type" => Declined::PayloadType,
        "incomplete" => Declined::Incomplete,
        "forged_advert" => Declined::ForgedAdvert,
        "path_full" => Declined::PathFull,
        "hop_limit" => Declined::HopLimit,
        "region" => Declined::Region,
        other => panic!("unknown reason {other}"),
    }
}

fn vectors() -> Value {
    serde_json::from_str(VECTORS).expect("vectors parse")
}

#[test]
fn vectors_name_the_baseline() {
    assert_eq!(vectors()["baseline_commit"], BASELINE_COMMIT);
    // What `VectorEnv` hard-codes.
    assert_eq!(vectors()["airtime_ms_per_octet"], 4);
    assert_eq!(vectors()["random_draw"], "maximum");
}

#[test]
fn duplicate_identities_match_the_vectors() {
    for case in vectors()["hashes"].as_array().unwrap() {
        let frame = hex(&case["frame"]);
        let packet = Packet::parse(&frame).unwrap();
        assert_eq!(packet.hash().as_slice(), hex(&case["hash"]), "{case}");
    }
}

#[test]
fn transport_codes_match_the_vectors() {
    for case in vectors()["transport_codes"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap().as_bytes();
        let key = TransportKey::for_region_name(name).unwrap();
        let expected: [u8; 16] = hex(&case["key"]).try_into().unwrap();
        assert_eq!(key, TransportKey::from_bytes(expected), "{case}");
        let payload_type = case["payload_type"].as_u64().unwrap() as u8;
        let code = key.code(payload_type, &hex(&case["payload"]));
        assert_eq!(code.as_slice(), hex(&case["code"]), "{case}");
    }
}

#[test]
fn the_reserved_codes_are_both_exercised() {
    let raw: std::vec::Vec<_> = vectors()["transport_codes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|case| {
            case.get("raw_code")
                .map(|raw| raw.as_str().unwrap().to_owned())
        })
        .collect();
    assert_eq!(raw, ["0000", "ffff"]);
}

#[test]
fn every_scenario_matches_the_baseline() {
    let document = vectors();
    let scenarios = document["scenarios"].as_array().unwrap();
    assert!(scenarios.len() > 100);
    for scenario in scenarios {
        let name = scenario["name"].as_str().unwrap();
        let key: [u8; 32] = hex(&scenario["device_key"]).try_into().unwrap();
        let regions: std::vec::Vec<std::string::String> = scenario["regions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r.as_str().unwrap().to_owned())
            .collect();
        let settings = Settings {
            region_policy: RegionPolicy::from_u8(scenario["region_policy"].as_u64().unwrap() as u8)
                .unwrap(),
            regions: Default::default(),
            min_rssi: scenario["min_rssi"].as_i64().map(|v| v as i16),
            min_snr: scenario["min_snr"].as_i64().map(|v| v as i8),
        }
        .with_region_names(regions.iter().map(|r| r.as_bytes()));
        let mut forwarder = Forwarder::<8>::new(key, settings);
        for (index, step) in scenario["steps"].as_array().unwrap().iter().enumerate() {
            let now_ms = step["now_ms"].as_u64().unwrap();
            let verdict = forwarder.receive(
                &hex(&step["frame"]),
                heard(&step["heard"]),
                now_ms,
                &mut VectorEnv,
            );
            let context = format!("{name}, step {index}: got {verdict:?}, want {step}");
            match step["verdict"].as_str().unwrap() {
                "scheduled" => {
                    let due_ms = step["due_ms"].as_u64().unwrap();
                    assert_eq!(verdict, Verdict::Scheduled { due_ms }, "{context}");
                    let priority = step["priority"].as_u64().unwrap() as u8;
                    assert_eq!(forwarder.due_priority(due_ms), Some(priority), "{context}");
                    let frame = forwarder.pop_due(due_ms).expect(&context);
                    assert_eq!(frame.as_slice(), hex(&step["forwarded"]), "{context}");
                }
                "declined" => assert_eq!(
                    verdict,
                    Verdict::Declined(declined(step["reason"].as_str().unwrap())),
                    "{context}"
                ),
                "duplicate" => assert_eq!(verdict, Verdict::Duplicate, "{context}"),
                "foreign" => assert_eq!(verdict, Verdict::Foreign, "{context}"),
                "malformed" => assert_eq!(verdict, Verdict::Malformed, "{context}"),
                "link_gated" => assert_eq!(verdict, Verdict::LinkGated, "{context}"),
                "remembered" => assert_eq!(verdict, Verdict::Remembered, "{context}"),
                other => panic!("unknown verdict {other}"),
            }
            assert_eq!(forwarder.queued(), 0, "{context}");
        }
    }
}

// ─── Local behavior ─────────────────────────────────────────────────────────

const KEY: [u8; 32] = [0x5A; 32];

/// Airtime of one millisecond per octet, and a draw the test sets.
struct TestEnv {
    draw: u32,
}

impl Environment for TestEnv {
    fn airtime_ms(&self, len: usize) -> u32 {
        len as u32
    }

    fn random_u32(&mut self) -> u32 {
        self.draw
    }
}

fn frame(
    kind: u8,
    route: RouteType,
    path_len: u8,
    path: &[u8],
    payload: &[u8],
) -> std::vec::Vec<u8> {
    let codes = route
        .has_transport_codes()
        .then_some([0x11, 0x22, 0x33, 0x44]);
    packet::encode(packet::header(kind, route), codes, path_len, path, payload)
        .unwrap()
        .to_vec()
}

fn air() -> Heard {
    Heard::Air {
        rssi: -70,
        snr: Snr::from_decibels(6),
    }
}

#[test]
fn direct_traffic_goes_ahead_of_floods_and_traces() {
    let mut forwarder = Forwarder::<4>::new(KEY, Settings::default());
    let mut env = TestEnv { draw: 0 };
    let flood = frame(payload_type::TXT_MSG, RouteType::Flood, 0, &[], &[1; 12]);
    let trace = frame(
        payload_type::TRACE,
        RouteType::Direct,
        0,
        &[],
        &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0x5A, 0x01],
    );
    let direct = frame(
        payload_type::TXT_MSG,
        RouteType::Direct,
        2,
        &[0x5A, 0x01],
        &[2; 12],
    );
    for f in [&flood, &trace, &direct] {
        assert!(matches!(
            forwarder.receive(f, air(), 0, &mut env),
            Verdict::Scheduled { due_ms: 0 }
        ));
    }
    let order: std::vec::Vec<u8> = core::iter::from_fn(|| forwarder.pop_due(0))
        .map(|f| Packet::parse(&f).unwrap().payload_type())
        .collect();
    // Direct at priority 0, the one-hop flood at 1, the trace at 5.
    assert_eq!(
        order,
        [
            payload_type::TXT_MSG,
            payload_type::TXT_MSG,
            payload_type::TRACE
        ]
    );
}

#[test]
fn a_full_queue_drops_the_new_forward() {
    let mut forwarder = Forwarder::<1>::new(KEY, Settings::default());
    let mut env = TestEnv { draw: 0 };
    let a = frame(payload_type::TXT_MSG, RouteType::Flood, 0, &[], &[1; 12]);
    let b = frame(payload_type::TXT_MSG, RouteType::Flood, 0, &[], &[2; 12]);
    assert!(matches!(
        forwarder.receive(&a, air(), 0, &mut env),
        Verdict::Scheduled { .. }
    ));
    assert_eq!(
        forwarder.receive(&b, air(), 0, &mut env),
        Verdict::QueueFull
    );
    assert_eq!(forwarder.counters().queue_full, 1);
    assert_eq!(
        Packet::parse(&forwarder.pop_due(0).unwrap())
            .unwrap()
            .payload(),
        &[1; 12]
    );
}

#[test]
fn the_window_spans_zero_to_five_times_the_scaled_airtime() {
    // A one-hop flood of 12 octets: path 1 + payload 12 + 2 = 15 ms, so
    // t = 7 and the window is 0..=35.
    let payload = [7u8; 12];
    let flood = frame(payload_type::TXT_MSG, RouteType::Flood, 0, &[], &payload);
    for (draw, expected) in [(0, 0), (u32::MAX / 2, 17), (u32::MAX, 35)] {
        let mut forwarder = Forwarder::<1>::new(KEY, Settings::default());
        let verdict = forwarder.receive(&flood, air(), 100, &mut TestEnv { draw });
        assert_eq!(
            verdict,
            Verdict::Scheduled {
                due_ms: 100 + expected
            }
        );
    }
}

#[test]
fn reset_forgets_what_was_seen_and_queued() {
    let mut forwarder = Forwarder::<2>::new(KEY, Settings::default());
    let mut env = TestEnv { draw: 0 };
    let flood = frame(payload_type::TXT_MSG, RouteType::Flood, 0, &[], &[1; 12]);
    assert!(matches!(
        forwarder.receive(&flood, air(), 0, &mut env),
        Verdict::Scheduled { .. }
    ));
    forwarder.reset();
    assert_eq!(forwarder.queued(), 0);
    assert!(matches!(
        forwarder.receive(&flood, air(), 0, &mut env),
        Verdict::Scheduled { .. }
    ));
}

#[test]
fn new_settings_keep_the_duplicate_memory() {
    let mut forwarder = Forwarder::<2>::new(KEY, Settings::default());
    let mut env = TestEnv { draw: 0 };
    let flood = frame(payload_type::TXT_MSG, RouteType::Flood, 0, &[], &[1; 12]);
    assert!(matches!(
        forwarder.receive(&flood, air(), 0, &mut env),
        Verdict::Scheduled { .. }
    ));
    forwarder.set_settings(Settings {
        min_rssi: Some(-120),
        ..Settings::default()
    });
    assert_eq!(
        forwarder.receive(&flood, air(), 0, &mut env),
        Verdict::Duplicate
    );
}

#[test]
fn private_regions_contribute_no_key() {
    let settings = Settings::default().with_region_names([&b"$ops"[..], b"sf-bay", b"#au-nsw"]);
    assert_eq!(settings.regions.len(), 2);
}

#[test]
fn reserved_identity_prefixes() {
    assert!(!identity_allowed(0x00));
    assert!(!identity_allowed(0xFF));
    assert!((0x01..=0xFE).all(identity_allowed));
}

/// Deterministic fuzz input: SHA-256 in counter mode.
fn noise(seed: u32, len: usize) -> std::vec::Vec<u8> {
    let mut out = std::vec::Vec::with_capacity(len);
    let mut block = 0u32;
    while out.len() < len {
        out.extend_from_slice(&SoftwareSha256.hash(&[&seed.to_be_bytes(), &block.to_be_bytes()]));
        block += 1;
    }
    out.truncate(len);
    out
}

/// Arbitrary input never panics, and everything the forwarder emits is a
/// well-formed packet carrying a payload it received: it relays, and
/// originates nothing.
#[test]
fn arbitrary_frames_are_handled_and_nothing_is_originated() {
    let settings = Settings {
        region_policy: RegionPolicy::Ignore,
        ..Settings::default()
    };
    // A key whose prefixes appear in the fuzzed paths often enough for
    // direct and trace forwarding to be reached.
    let key = [0x00; 32];
    let mut forwarder = Forwarder::<8>::new(key, settings);
    let mut env = TestEnv { draw: 0x8000_0000 };
    let mut scheduled = 0;
    for seed in 0..20_000u32 {
        let bytes = noise(seed, 1 + (seed as usize * 7) % 255);
        let mut input = bytes.clone();
        // Bias toward version-1 frames and short paths so the deep paths
        // are exercised, not just the version check.
        input[0] &= 0x3F;
        if input.len() > 1 && seed % 3 != 0 {
            input[1] &= 0x83;
        }
        for frame in [&bytes, &input] {
            let verdict = forwarder.receive(frame, air(), u64::from(seed), &mut env);
            if let Verdict::Scheduled { .. } = verdict {
                scheduled += 1;
                let sent = forwarder.pop_due(u64::MAX).unwrap();
                let sent = Packet::parse(&sent).expect("forwards are well formed");
                let received = Packet::parse(frame).unwrap();
                let relayed = sent.payload() == received.payload()
                    // A multipart acknowledgment goes on as the one it carries.
                    || (received.payload_type() == payload_type::MULTIPART
                        && sent.payload() == &received.payload()[1..]);
                assert!(relayed, "{frame:02x?}");
            }
        }
    }
    assert!(scheduled > 1_000, "only {scheduled} forwards exercised");
}
