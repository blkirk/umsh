//! What the communicator's tests share: a soft device, and the device's
//! own interface into it.

#![allow(dead_code)]

use std::cell::RefCell;
use std::rc::Rc;

use umsh_ulcp::frame;
use umsh_ulcp::ids::prop;
use umsh_ulcp_admin::{LocalPort, PortBusy, PortFrame};
use umsh_ulcp_simdev::{DutyLedger, RadioSettings, SessionConfig, SimulatedDevice};

pub fn config() -> SessionConfig {
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
pub struct Port(pub Rc<RefCell<SimulatedDevice>>);

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

pub fn device() -> Rc<RefCell<SimulatedDevice>> {
    let mut sim = SimulatedDevice::new(config());
    sim.open_device_interface();
    Rc::new(RefCell::new(sim))
}

/// Gives `sim` the identity `secret` makes, or one it makes itself, as
/// whoever holds the device may.
pub fn provision(sim: &Rc<RefCell<SimulatedDevice>>, secret: Option<&[u8; 32]>) {
    let mut request = [0u8; 48];
    let value: &[u8] = secret.map_or(&[], |secret| &secret[..]);
    let len = frame::prop_set(&mut request, 1, prop::DEV_PRIVATE_KEY, value).unwrap();
    sim.borrow_mut().device_request(&request[..len], 0);
    while sim.borrow_mut().take_device_frame().is_some() {}
}
