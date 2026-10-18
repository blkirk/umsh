use super::*;
use core::future::Future;
use core::task::{Context, Poll, Waker};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::signal::Signal;
use std::cell::RefCell;
use std::rc::Rc;
use umsh_crypto::CryptoEngine;
use umsh_crypto::software::{SoftwareAes, SoftwareSha256};
use umsh_ulcp_device::{DutyLedger, RadioSettings, SessionConfig};

type TestSession = Session<SoftwareAes, SoftwareSha256, 1>;

fn session(name: &'static str) -> TestSession {
    Session::new(
        SessionConfig {
            dev_version: "test",
            dev_model: None,
            default_device_name: name,
            mtu: 255,
            tx_preamble_symbols: 32,
            sync_word: 0x1424,
            min_tx_power_dbm: -9,
            max_tx_power_dbm: 22,
            freq_khz_min: 150_000,
            freq_khz_max: 960_000,
            defaults: RadioSettings {
                enabled: false,
                freq_khz: 910_525,
                bw_hz: 62_500,
                sf: 7,
                cr_denom: 5,
                tx_power_dbm: 14,
            },
            default_duty_limit: 0xffff,
            duty: Box::leak(Box::new(DutyLedger::new())),
            battery: None,
            battery_diagnostics: Default::default(),
            stats: None,
            alert: None,
            time: None,
            gnss: None,
            display_motion_wake: false,
            illuminance: false,
            temperatures: false,
            ble: true,
            ble_pairing: true,
            ble_pin: true,
            reboot: true,
            dev_identity: true,
            mac_node: false,
            meshcore_forwarder: false,
            lan_bearer: umsh_ulcp_device::LanBearerRole::None,
            wifi: None,
            ip: None,
            bridge_client: false,
            i2c_buses: &[],
            i2c_devices: &[],
        },
        Status::RESET_POWER_ON,
        CryptoEngine::new(SoftwareAes, SoftwareSha256),
    )
}

#[derive(Debug, PartialEq)]
enum Event {
    Name(std::string::String),
    BleEnabled(bool),
    Domain,
    Ready,
    Dfu(umsh_ulcp::DfuMode),
    SnapshotPersisted,
}

struct Env {
    events: Rc<RefCell<std::vec::Vec<Event>>>,
    older: Option<std::vec::Vec<u8>>,
    restore_gate: &'static Signal<NoopRawMutex, ()>,
    name_gate: Option<&'static Signal<NoopRawMutex, ()>>,
    dfu_result: Result<umsh_ulcp::DfuMode, Status>,
}

impl DeviceEnv for Env {
    async fn prepare_dfu(&mut self, _: umsh_ulcp::DfuMode) -> Result<umsh_ulcp::DfuMode, Status> {
        self.dfu_result
    }
    fn enter_dfu(&mut self, mode: umsh_ulcp::DfuMode) -> ! {
        self.events.borrow_mut().push(Event::Dfu(mode));
        panic!("test bootloader handoff")
    }
    async fn persist_snapshot(&mut self, _: &[u8]) -> Result<(), ()> {
        self.events.borrow_mut().push(Event::SnapshotPersisted);
        Ok(())
    }
    async fn clear_snapshot(&mut self) -> Result<(), ()> {
        unreachable!()
    }
    async fn persist_identity(&mut self, _: &[u8]) -> Result<(), ()> {
        unreachable!()
    }
    async fn clear_identity(&mut self) -> Result<(), ()> {
        unreachable!()
    }
    fn fill_secret(&mut self, _: &mut [u8; 32]) -> Result<(), ()> {
        unreachable!()
    }
    async fn apply_pairing_pin(&mut self, _: Option<u32>) -> bool {
        unreachable!()
    }
    async fn factory_reset(&mut self) -> ! {
        unreachable!()
    }
    async fn reboot(&mut self) -> ! {
        unreachable!()
    }
    fn set_advertising_allowed(&mut self, _: bool) {}
    async fn older_snapshot(&mut self, out: &mut [u8]) -> Option<usize> {
        self.restore_gate.wait().await;
        let bytes = self.older.take()?;
        out[..bytes.len()].copy_from_slice(&bytes);
        Some(bytes.len())
    }
    async fn publish_device_name(&mut self, name: &str) {
        if let Some(gate) = self.name_gate.take() {
            gate.wait().await;
        }
        self.events.borrow_mut().push(Event::Name(name.into()));
    }
    fn set_ble_enabled(&mut self, enabled: bool) {
        self.events.borrow_mut().push(Event::BleEnabled(enabled));
    }
    fn publish_dev_domain(&mut self, _: DevDomainSnapshot) {
        self.events.borrow_mut().push(Event::Domain);
    }
    fn boot_settings_ready(&mut self) {
        self.events.borrow_mut().push(Event::Ready);
    }
}

#[test]
fn transport_readiness_follows_restore_name_and_settings_on_every_boot_path() {
    let mut saved_session = session("Saved radio");
    assert_eq!(saved_session.toggle_ble(&mut |_| {}), Some(false));
    let mut saved = [0; SNAPSHOT_MAX];
    let len = saved_session.encode_snapshot(&mut saved).unwrap();
    let saved = &saved[..len];
    let corrupt = &[0xff][..];
    for (boot, older, expected_name, expected_enabled) in [
        (Some(saved), None, "Saved radio", false),
        (Some(corrupt), Some(saved), "Saved radio", false),
        (None, None, "Default radio", true),
        (Some(corrupt), None, "Default radio", true),
    ] {
        let events = Rc::new(RefCell::new(std::vec::Vec::new()));
        let restore_gate = Box::leak(Box::new(Signal::new()));
        let name_gate = Box::leak(Box::new(Signal::new()));
        let env = Env {
            events: events.clone(),
            older: older.map(|bytes| bytes.to_vec()),
            restore_gate,
            name_gate: Some(name_gate),
            dfu_result: Err(Status::UNIMPLEMENTED),
        };
        let rt = DeviceRuntime {
            input: Box::leak(Box::new(InputChannel::<NoopRawMutex>::new())),
            radio: Box::leak(Box::new(Channels::<NoopRawMutex, 1, 1>::new())),
            ctl: Box::leak(Box::new(DeviceControl::new())),
            out: Box::leak(Box::new(TransportChannels::new())),
            session_gen: Box::leak(Box::new(AtomicU32::new(0))),
            device: None,
        };
        let mut session = session("Default radio");
        let mut storage = [0; SNAPSHOT_MAX];
        let mut run = core::pin::pin!(run_with_storage(
            &mut session,
            &mut storage,
            boot,
            None,
            rt,
            env
        ));
        let mut cx = Context::from_waker(Waker::noop());
        assert!(matches!(run.as_mut().poll(&mut cx), Poll::Pending));
        assert!(
            events.borrow().is_empty(),
            "Must wait for NVRAM and name publication"
        );
        restore_gate.signal(());
        assert!(matches!(run.as_mut().poll(&mut cx), Poll::Pending));
        assert!(
            events.borrow().is_empty(),
            "Name publication must finish before readiness"
        );
        name_gate.signal(());
        assert!(matches!(run.as_mut().poll(&mut cx), Poll::Pending));
        let events = events.borrow();
        assert_eq!(events.last(), Some(&Event::Ready));
        assert_eq!(events.iter().filter(|e| **e == Event::Ready).count(), 1);
        assert!(events.contains(&Event::Name(expected_name.into())));
        assert!(
            events
                .iter()
                .all(|e| !matches!(e, Event::Name(name) if name != expected_name))
        );
        assert!(events.ends_with(&[
            Event::BleEnabled(expected_enabled),
            Event::Domain,
            Event::Ready
        ]));
    }
}

#[test]
fn dfu_preparation_and_matching_transmission_precede_handoff() {
    use embassy_futures::block_on;
    use umsh_ulcp::{DfuMode, Frame, PropPayload, frame};
    let rt = DeviceRuntime {
        input: Box::leak(Box::new(InputChannel::<NoopRawMutex>::new())),
        radio: Box::leak(Box::new(Channels::<NoopRawMutex, 1, 1>::new())),
        ctl: Box::leak(Box::new(DeviceControl::new())),
        out: Box::leak(Box::new(TransportChannels::new())),
        session_gen: Box::leak(Box::new(AtomicU32::new(42))),
        device: None,
    };
    let events = Rc::new(RefCell::new(std::vec::Vec::new()));
    let mut env = Env {
        events: events.clone(),
        older: None,
        restore_gate: Box::leak(Box::new(Signal::new())),
        name_gate: None,
        dfu_result: Err(Status::UNIMPLEMENTED),
    };
    let mut session = session("DFU test");
    session.set_boot_identity([0x55; 32]);
    let mut snapshot = [0; SNAPSHOT_MAX];
    let mut before = [0; SNAPSHOT_MAX];
    let before_len = session.encode_snapshot(&mut before).unwrap();
    let mut request = [0; 3];
    let len = frame::dfu(&mut request, 3, Some(DfuMode::Ble)).unwrap();
    let mut emitter = Emitter::new();
    let mut previous: Option<crate::reply_completion::ReplyCompletion> = None;
    for (prepared, sent) in [
        (Err(Status::UNIMPLEMENTED), false),
        (Err(Status::FAILURE), false),
        (Ok(DfuMode::Ble), false),
        (Ok(DfuMode::Ble), true),
    ] {
        env.dfu_result = prepared;
        let dfu = block_on(serve_frame(
            &mut session,
            Exchange::Local(&request[..len]),
            &mut emitter,
            &mut ReplySink::Transport {
                destination: Some((Transport::Usb, 42)),
                out: rt.out,
                device: None,
            },
            &mut snapshot,
            &rt,
            &mut env,
        ));
        let response = rt.out.wired.try_receive().unwrap();
        assert_eq!(response.generation, 42);
        let parsed = Frame::parse(&response.frame).unwrap();
        assert_eq!(parsed.header.tid(), 3);
        let prop = PropPayload::parse(parsed.payload).unwrap();
        assert_eq!(prop.key, umsh_ulcp::ids::prop::LAST_STATUS);
        let expected = prepared.map(|_| Status::OK).unwrap_or_else(|error| error);
        assert_eq!(umsh_ulcp::pui::decode(prop.value).unwrap().0, expected.0);
        assert!(
            events
                .borrow()
                .iter()
                .all(|event| !matches!(event, Event::Dfu(_)))
        );
        let Some((_, completion)) = dfu else {
            assert!(
                response.completion.is_none(),
                "preparation failure must not authorize transmission completion"
            );
            continue;
        };
        let mut wait = core::pin::pin!(completion.wait());
        let mut cx = Context::from_waker(Waker::noop());
        assert!(
            wait.as_mut().poll(&mut cx).is_pending(),
            "queue acceptance must not authorize a handoff"
        );
        if let Some(stale) = previous {
            stale.complete(true);
        }
        assert!(
            wait.as_mut().poll(&mut cx).is_pending(),
            "stale completion must not authorize a handoff"
        );
        // Wait was only polled, not consumed, so the driver can own it now.
        response.completion.unwrap().complete(sent);
        previous = Some(completion);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            block_on(finish_dfu(&mut env, dfu))
        }));
        assert_eq!(result.is_err(), sent, "only the final handoff diverges");
        let mut after = [0; SNAPSHOT_MAX];
        let after_len = session.encode_snapshot(&mut after).unwrap();
        assert_eq!(&after[..after_len], &before[..before_len]);
    }
    let expired = crate::reply_completion::ReplyCompletion::begin(Instant::now());
    assert!(
        !expired.complete(true),
        "late transmission cannot retain cached success"
    );
    assert!(!block_on(expired.wait()));
    let timed_out = crate::reply_completion::ReplyCompletion::begin(Instant::now());
    assert!(
        !block_on(timed_out.wait()),
        "a missing completion must expire"
    );
    assert!(
        !timed_out.complete(true),
        "retired tokens cannot authorize a later action"
    );
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|event| matches!(event, Event::Dfu(DfuMode::Ble)))
            .count(),
        1
    );
}

#[test]
fn stored_identity_must_match_its_own_secret() {
    use umsh_journal_store::proto::StoredIdentity;
    let secret = [0x5a; 32];
    let (public, record) = device_identity_record(&secret);
    assert_eq!(
        stored_device_identity(Some(&record)),
        StoredIdentity::Keys { secret, public }
    );
    let mut damaged = record;
    damaged[40] ^= 1;
    assert_eq!(
        stored_device_identity(Some(&damaged)),
        StoredIdentity::Unparseable
    );
    assert_eq!(
        stored_device_identity(Some(&record[..32])),
        StoredIdentity::Unparseable
    );
    assert_eq!(stored_device_identity(None), StoredIdentity::Absent);
}

/// A change the device makes on its own is persisted only over a
/// snapshot that is current: never onto a device with nothing saved,
/// and never over a generation boot rejected, where the write would
/// bury the fault `PROP_SAVED` reports.
#[test]
fn device_changes_persist_only_over_a_current_snapshot() {
    use embassy_futures::block_on;
    let mut saved = [0; SNAPSHOT_MAX];
    let len = session("Saved radio").encode_snapshot(&mut saved).unwrap();
    let saved = &saved[..len];
    for (restored, rejected, expected_status, persists) in [
        (false, false, SavedStatus::None, false),
        (true, false, SavedStatus::Current, true),
        (true, true, SavedStatus::Fallback, false),
        (false, true, SavedStatus::Unreadable, false),
    ] {
        let mut session = session("Device change");
        if rejected {
            session.note_snapshot_rejected();
        }
        if restored {
            session.restore_at_boot(saved).unwrap();
        }
        assert_eq!(session.saved_status(), expected_status);
        let events = Rc::new(RefCell::new(std::vec::Vec::new()));
        let mut env = Env {
            events: events.clone(),
            older: None,
            restore_gate: Box::leak(Box::new(Signal::new())),
            name_gate: None,
            dfu_result: Err(Status::UNIMPLEMENTED),
        };
        let mut snapshot = [0; SNAPSHOT_MAX];
        assert_eq!(session.toggle_ble(&mut |_| {}), Some(false));
        block_on(persist_device_change(&mut session, &mut snapshot, &mut env));
        assert_eq!(
            events.borrow().contains(&Event::SnapshotPersisted),
            persists,
            "{expected_status:?}"
        );
        assert_eq!(session.saved_status(), expected_status);
    }
}

/// The device's own interface is answered on its link and nowhere else,
/// hears every publication the host hears, and is told when it fell too
/// far behind to hear them all.
#[test]
fn the_device_interface_is_answered_on_its_link_and_hears_every_publication() {
    use umsh_ulcp::{Frame, PropPayload, frame, ids::prop};
    let link: &'static DeviceLink<NoopRawMutex> = Box::leak(Box::new(DeviceLink::new()));
    let rt = DeviceRuntime {
        input: Box::leak(Box::new(InputChannel::<NoopRawMutex>::new())),
        radio: Box::leak(Box::new(Channels::<NoopRawMutex, 1, 1>::new())),
        ctl: Box::leak(Box::new(DeviceControl::new())),
        out: Box::leak(Box::new(TransportChannels::new())),
        session_gen: Box::leak(Box::new(AtomicU32::new(0))),
        device: Some(link),
    };
    let input = rt.input;
    let out = rt.out;
    let env = Env {
        events: Rc::new(RefCell::new(std::vec::Vec::new())),
        older: None,
        restore_gate: Box::leak(Box::new(Signal::new())),
        name_gate: None,
        dfu_result: Err(Status::UNIMPLEMENTED),
    };
    let mut session = session("Device link");
    let mut storage = [0; SNAPSHOT_MAX];
    let mut run = core::pin::pin!(run_with_storage(
        &mut session,
        &mut storage,
        None,
        Some([0x55; 32]),
        rt,
        env
    ));
    let mut cx = Context::from_waker(Waker::noop());
    let mut step = |event: Option<InEvent>| {
        if let Some(event) = event {
            assert!(input.try_send(event).is_ok());
        }
        assert!(run.as_mut().poll(&mut cx).is_pending());
    };
    step(None);
    let parse = |bytes: &[u8]| {
        let parsed = Frame::parse(bytes).unwrap();
        let payload = PropPayload::parse(parsed.payload).unwrap();
        (parsed.header.tid(), payload.key, payload.value.to_vec())
    };

    // With no host attached, a publication still reaches the interface.
    step(Some(InEvent::Toggle(Setting::Bluetooth)));
    let Some(DeviceFrame::Frame(published)) = link.try_receive() else {
        panic!("the toggle was not published to the interface");
    };
    assert_eq!(parse(&published), (0, prop::BLE_ENABLED, vec![0]));

    // A host attaches. The interface's request is answered on the link
    // alone, under its own TID.
    step(Some(InEvent::Attached(Transport::Usb)));
    while out.wired.try_receive().is_ok() {}
    let mut buf = [0u8; 64];
    let len = frame::prop_set(&mut buf, 3, prop::DEV_NAME, b"Pocket").unwrap();
    step(Some(InEvent::Device(
        FrameBuf::from_slice(&buf[..len]).unwrap(),
    )));
    let Some(DeviceFrame::Frame(answer)) = link.try_receive() else {
        panic!("the request was not answered");
    };
    assert_eq!(parse(&answer), (3, prop::DEV_NAME, b"Pocket".to_vec()));
    assert!(link.try_receive().is_none(), "exactly one answer");
    assert!(out.wired.try_receive().is_err(), "the host hears no answer");

    // The host's own frames are still the host's: the exchange was not an
    // attach, and displaced nothing.
    let len = frame::prop_get(&mut buf, 2, prop::DEV_NAME).unwrap();
    step(Some(InEvent::Frame(
        Transport::Usb,
        FrameBuf::from_slice(&buf[..len]).unwrap(),
    )));
    let reply = out.wired.try_receive().expect("the host is still served");
    assert_eq!(parse(&reply.frame), (2, prop::DEV_NAME, b"Pocket".to_vec()));
    assert!(
        link.try_receive().is_none(),
        "the host's answer is the host's"
    );

    // A publication reaches both.
    step(Some(InEvent::Toggle(Setting::Bluetooth)));
    let to_host = out.wired.try_receive().expect("published to the host");
    assert_eq!(parse(&to_host.frame), (0, prop::BLE_ENABLED, vec![1]));
    let Some(DeviceFrame::Frame(to_device)) = link.try_receive() else {
        panic!("not published to the interface");
    };
    assert_eq!(parse(&to_device), (0, prop::BLE_ENABLED, vec![1]));

    // An interface that stops reading loses publications, never blocks
    // the driver, and is told after the ones that were kept.
    for _ in 0..DEVICE_LINK_DEPTH + 2 {
        step(Some(InEvent::Toggle(Setting::Bluetooth)));
        while out.wired.try_receive().is_ok() {}
    }
    let mut kept = 0;
    let mut overrun = false;
    while let Some(frame) = link.try_receive() {
        match frame {
            DeviceFrame::Frame(_) => {
                assert!(!overrun, "the overrun follows the frames kept");
                kept += 1;
            }
            DeviceFrame::Overrun => {
                assert!(!overrun, "the overrun is reported once");
                overrun = true;
            }
        }
    }
    assert_eq!(kept, DEVICE_LINK_DEPTH);
    assert!(overrun);
}
