//! The LAN virtual radio: a soft ULCP device in this process whose PHY is
//! the LAN bearer's multicast group.
//!
//! The device is the production session on a file-backed platform, so it
//! has an identity and saved configuration of its own that persist from
//! one invocation to the next. The tool reaches it over an in-memory byte
//! pipe carrying the same HDLC-Lite framing a wire does, and everything
//! above that—attach, the mesh session, every command—is unchanged.
//!
//! What the device transmits is sent to the group, where every radio with
//! its bearer enabled hears it; what any participant sends to the group
//! is received unmeasured, since nothing here measured it.

use std::fs::File;
use std::net::SocketAddrV4;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context as _, Result, anyhow, bail};
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};
use umsh::tokio_support::{LanConfig, LanSocket};
use umsh::ulcp_wire::profiles;
use umsh_lan_bearer::{Datagram, MAX_FRAME};
use umsh_ulcp_simdev::host::HostPlatform;
use umsh_ulcp_simdev::soft::SoftDevice;
use umsh_ulcp_simdev::{DutyLedger, RadioRxInfo, RadioSettings, SessionConfig};

/// How much of the pipe to take in one read. ULCP frames are small; this
/// only bounds a burst.
const READ_CHUNK: usize = 4096;

/// How long an open waits for a previous virtual radio in this process
/// to let go of its state.
const LOCK_PATIENCE: Duration = Duration::from_secs(2);

/// The shape of the virtual radio.
///
/// Spare, like a bridge's host interface: no hardware to describe and no
/// node behind it. The PHY settings it reports are the default profile's,
/// fixed; the frames it sends are transmitted later by whichever LoRa
/// radios forward them, so the frame time a host's MAC derives from these
/// is an estimate either way. The duty limit is left open—sending to the
/// group spends no airtime—and the radios' ingress limits are the
/// control that means something.
fn session_config() -> SessionConfig {
    SessionConfig {
        dev_version: concat!("umshctl-lan/", env!("CARGO_PKG_VERSION")),
        dev_model: None,
        default_device_name: "LAN virtual radio",
        mtu: MAX_FRAME as u16,
        tx_preamble_symbols: 32,
        sync_word: profiles::DEFAULT.sync_word,
        min_tx_power_dbm: -9,
        max_tx_power_dbm: 22,
        freq_khz_min: 150_000,
        freq_khz_max: 960_000,
        defaults: RadioSettings {
            enabled: false,
            freq_khz: profiles::DEFAULT.freq_khz,
            bw_hz: profiles::DEFAULT.bw_hz,
            sf: profiles::DEFAULT.sf,
            cr_denom: profiles::DEFAULT.cr_denom,
            tx_power_dbm: profiles::DEFAULT_TX_POWER_DBM,
        },
        default_duty_limit: 0xFFFF,
        duty: Box::leak(Box::new(DutyLedger::new())),
        battery_diagnostics: Default::default(),
        battery: None,
        alert: None,
        time: None,
        gnss: None,
        display_motion_wake: false,
        illuminance: false,
        temperatures: false,
        ble: false,
        ble_pairing: false,
        ble_pin: false,
        // The device is a task in this process; there is nothing for
        // `CMD_REBOOT` to restart.
        reboot: false,
        stats: None,
        mac_node: false,
        meshcore_forwarder: false,
        lan_bearer: false,
        wifi: None,
        ip: None,
        bridge_client: false,
        i2c_buses: &[],
        i2c_devices: &[],
    }
}

/// Where the virtual radio keeps its identity and saved configuration.
fn device_dir() -> Result<PathBuf> {
    crate::connection::state_dir()
        .map(|dir| dir.join("lan-device"))
        .ok_or_else(|| anyhow!("no HOME directory to keep the LAN device's state in"))
}

/// Hold the state directory for one virtual radio at a time. Two
/// devices sharing an identity would send under one key with two frame
/// counters, and each would see the other's frames as replays.
fn lock(dir: &std::path::Path) -> Result<Option<File>> {
    use std::os::fd::AsRawFd;
    let file = File::create(dir.join("lock"))
        .with_context(|| format!("opening {}", dir.join("lock").display()))?;
    // SAFETY: `flock` on a descriptor this function owns.
    let held = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0;
    Ok(held.then_some(file))
}

/// Start a virtual radio on `group` and return the host's end of the
/// link to it.
///
/// The device runs on this thread's `LocalSet` until the host's end is
/// dropped.
pub async fn open(group: SocketAddrV4) -> Result<DuplexStream> {
    let dir = device_dir()?;
    let platform =
        HostPlatform::new(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let deadline = tokio::time::Instant::now() + LOCK_PATIENCE;
    let lock = loop {
        if let Some(lock) = lock(&dir)? {
            break lock;
        }
        if tokio::time::Instant::now() >= deadline {
            bail!(
                "another umshctl is already the LAN virtual radio ({} is locked)",
                dir.display()
            );
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    let config = LanConfig {
        group,
        ..LanConfig::lan()
    };
    let socket = LanSocket::bind(config).with_context(|| format!("joining {group}"))?;
    let (host, device_end) = tokio::io::duplex(READ_CHUNK);
    let device = SoftDevice::new(session_config(), platform);
    tokio::task::spawn_local(async move {
        drive(device, socket, device_end).await;
        drop(lock);
    });
    Ok(host)
}

/// Relay between the pipe, the device, and the group until the host
/// lets go.
async fn drive(mut device: SoftDevice<HostPlatform>, socket: LanSocket, mut pipe: DuplexStream) {
    let started = tokio::time::Instant::now();
    let now_ms = || started.elapsed().as_millis() as u64;
    let mut buf = vec![0u8; READ_CHUNK];
    let mut datagram = [0u8; MAX_FRAME + 1];
    device.attach();
    loop {
        // Whatever the device has to say goes out first: the attach's
        // own notice, then each reply in turn.
        while let Some(wire) = device.take_outbound() {
            if pipe.write_all(&wire).await.is_err() {
                return;
            }
        }
        for frame in device.take_transmitted() {
            // A send that fails is a frame lost on the medium, which
            // the protocol above already tolerates.
            let _ = socket.send(&frame).await;
        }
        tokio::select! {
            read = pipe.read(&mut buf) => match read {
                Ok(0) | Err(_) => break,
                // A garbled frame costs one frame: the decoder
                // resynchronizes on the next flag.
                Ok(len) => {
                    let _ = device.ingest(&buf[..len], now_ms());
                }
            },
            received = socket.recv(&mut datagram) => {
                if let Ok((len, _)) = received
                    && umsh_lan_bearer::classify(&datagram[..len]) == Datagram::Umsh
                {
                    device.inject_radio_rx(&datagram[..len], &RadioRxInfo::default(), now_ms());
                }
            }
        }
    }
    device.detach();
}
