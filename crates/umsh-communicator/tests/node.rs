//! The node's settings, against the soft device's real session through
//! the device's own interface.

use std::cell::RefCell;
use std::rc::Rc;

use umsh_communicator::CommunicatorNode;
use umsh_communicator::settings::SAVE_DELAY_MS;
use umsh_ulcp::frame;
use umsh_ulcp::hdlc;
use umsh_ulcp::ids::prop;
use umsh_ulcp::profiles::VETTED;
use umsh_ulcp_admin::{LocalPort, PortBusy, PortFrame};
use umsh_ulcp_simdev::{DutyLedger, RadioSettings, SessionConfig, SimulatedDevice};
use umsh_ux_communicator::model::{KeyboardSettings, Node, NodeEvent, Profile, Setting, Shareable};

fn config() -> SessionConfig {
    SessionConfig {
        dev_version: "umsh-communicator-test/0.1",
        dev_model: None,
        default_device_name: "Pocket",
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
        default_duty_limit: 0xFFFF,
        duty: Box::leak(Box::new(DutyLedger::new())),
        battery_diagnostics: Default::default(),
        battery: None,
        alert: None,
        time: None,
        gnss: None,
        display_motion_wake: false,
        display_brightness: Some(113),
        keyboard: true,
        illuminance: false,
        temperatures: false,
        ble: true,
        ble_pairing: false,
        ble_pin: false,
        reboot: true,
        dev_identity: true,
        stats: None,
        mac_node: false,
        meshcore_forwarder: false,
        lan_bearer: umsh_ulcp_simdev::LanBearerRole::None,
        wifi: None,
        ip: None,
        bridge_client: false,
        i2c_buses: &[],
        i2c_devices: &[],
    }
}

/// The soft device's own interface. The test keeps a handle to the device
/// too, to act as a phone attached to it would.
#[derive(Clone)]
struct Port(Rc<RefCell<SimulatedDevice>>);

impl LocalPort for Port {
    fn send(&mut self, frame: &[u8]) -> Result<(), PortBusy> {
        self.0.borrow_mut().device_request(frame, 1_000);
        Ok(())
    }

    fn receive(&mut self, out: &mut [u8]) -> Option<PortFrame> {
        let frame = self.0.borrow_mut().take_device_frame()?;
        out[..frame.len()].copy_from_slice(&frame);
        Some(PortFrame::Frame(frame.len()))
    }
}

fn device() -> Rc<RefCell<SimulatedDevice>> {
    let mut sim = SimulatedDevice::new(config());
    sim.open_device_interface();
    Rc::new(RefCell::new(sim))
}

/// A node over `sim`, with its settings read.
fn node(sim: &Rc<RefCell<SimulatedDevice>>) -> CommunicatorNode<Port> {
    let node = CommunicatorNode::new(Port(sim.clone()));
    node.pump(0);
    assert!(node.primed());
    node
}

fn events(node: &CommunicatorNode<Port>) -> Vec<NodeEvent> {
    core::iter::from_fn(|| node.next_event()).collect()
}

#[test]
fn the_first_boot_saves_what_the_device_starts_with() {
    let sim = device();
    assert!(sim.borrow().saved_snapshot().is_none());
    let _node = node(&sim);
    assert!(sim.borrow().saved_snapshot().is_some());

    // With a snapshot in effect, the next boot leaves it alone.
    let before = sim.borrow().saved_snapshot().unwrap().to_vec();
    let _node = node(&sim);
    assert_eq!(sim.borrow().saved_snapshot().unwrap(), &before[..]);
}

/// A change reads at once, and the device keeps the snapshot in step with
/// it: no save is asked for while one is in effect.
#[test]
fn a_change_reads_at_once_and_is_kept() {
    let sim = device();
    let node = node(&sim);
    events(&node);
    let before = sim.borrow().saved_snapshot().unwrap().to_vec();

    node.set_name("Juniper");
    assert_eq!(node.device_name(), "Juniper");
    node.pump(1_000);
    assert_eq!(events(&node), [NodeEvent::SettingsChanged]);
    assert_eq!(node.device_name(), "Juniper");
    let kept = sim.borrow().saved_snapshot().unwrap().to_vec();
    assert_ne!(kept, before, "the snapshot was kept in step");
    node.pump(1_000 + SAVE_DELAY_MS);
    assert_eq!(sim.borrow().saved_snapshot().unwrap(), &kept[..]);
}

#[test]
fn the_profile_picker_reads_vetted_and_custom() {
    let sim = device();
    let node = node(&sim);
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
    node.pump(1_000);
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
    let mut sim = SimulatedDevice::new(SessionConfig {
        freq_khz_max: 900_000,
        ..config()
    });
    sim.open_device_interface();
    let sim = Rc::new(RefCell::new(sim));
    let node = node(&sim);
    events(&node);
    let kept = node.radio().profile;
    let unreachable = VETTED
        .iter()
        .position(|profile| profile.freq_khz > 900_000)
        .unwrap();
    node.set_profile(unreachable);
    assert_eq!(node.radio().profile, Profile::Vetted(unreachable));
    node.pump(1_000);
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
    let node = node(&sim);
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
    node.pump(1_000);
    assert_eq!(node.brightness(), 0);
    assert_eq!(node.keyboard(), keyboard);
    // Saved with the rest, as a switch at the device is.
    assert_ne!(sim.borrow().saved_snapshot().unwrap(), &before[..]);
}

/// What a phone attached to the device changes, the node hears.
#[test]
fn a_change_an_attached_phone_makes_reaches_the_node() {
    let sim = device();
    let node = node(&sim);
    events(&node);
    sim.borrow_mut().attach();
    while sim.borrow_mut().take_outbound().is_some() {}

    let mut request = [0u8; 32];
    let len = frame::prop_set(&mut request, 2, prop::DEV_NAME, b"Willow").unwrap();
    let mut encoded = vec![0u8; hdlc::max_encoded_len(len)];
    let len = hdlc::encode_frame(&request[..len], &mut encoded).unwrap();
    sim.borrow_mut().ingest(&encoded[..len], 2_000).unwrap();

    node.pump(2_000);
    assert_eq!(events(&node), [NodeEvent::SettingsChanged]);
    assert_eq!(node.device_name(), "Willow");
}

#[test]
fn the_address_and_share_link_come_from_the_device_identity() {
    let sim = device();
    {
        let node = node(&sim);
        assert_eq!(node.address(), "");
        assert_eq!(node.share_uri(Shareable::Identity), None);
    }
    // Whoever holds the device may give it an identity.
    let mut request = [0u8; 16];
    let len = frame::prop_set(&mut request, 1, prop::DEV_PRIVATE_KEY, &[]).unwrap();
    sim.borrow_mut().device_request(&request[..len], 0);
    while sim.borrow_mut().take_device_frame().is_some() {}

    let node = node(&sim);
    let address = node.address();
    assert_eq!(address.len(), 44);
    let uri = node.share_uri(Shareable::Identity).unwrap();
    assert_eq!(uri.as_str(), format!("umsh:n:{address}"));
    assert_eq!(node.share_uri(Shareable::Channel(1)), None);
}
