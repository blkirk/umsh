//! The device node behind the window: a MAC of the device identity's own,
//! on the LAN bearer, as the Pager's runs on its radio. It holds the
//! channels and peers the device's settings hold, the chats are fed what
//! it receives, and they send through it. Other simulators, and phones on
//! the network, are on the same group.
//!
//! It runs on a single-threaded runtime that the window's loop drives
//! between frames, so the node, the chats, and the window share a thread.

use std::cell::{Cell, RefCell};
use std::io;
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use tokio::runtime::Runtime;
use tokio::sync::watch;
use tokio::task::LocalSet;
use umsh::core::{ChannelKey, PublicKey};
use umsh::crypto::CryptoEngine;
use umsh::crypto::NodeIdentity;
use umsh::crypto::software::{SoftwareAes, SoftwareIdentity, SoftwareSha256};
use umsh::hal::BearerId;
use umsh::mac::{Mac, MacHandle, OperatingPolicy, RepeaterConfig};
use umsh::node::{Channel, Host, LocalNode};
use umsh::tokio_support::{
    LanConfig, RxMode, StdClock, TokioFileCounterStore, TokioFileKeyValueStore, TokioPlatform,
    UdpMulticastRadio,
};
use umsh_communicator::WellKnownChannels;
use umsh_communicator::task::{self, Inbox};
use umsh_sync::AsyncRefCell;
use umsh_ulcp_simdev::host::HostPlatform;
use umsh_ulcp_simdev::soft::{SoftDevice, SoftPlatform as _};

use crate::node::Node;

type Platform = TokioPlatform<UdpMulticastRadio, TokioFileCounterStore, TokioFileKeyValueStore>;

/// The Pager's device node, in its capacities: one identity, 32 peers, 8
/// channels, 8 pending acknowledgments, 8 transmit slots.
type MeshMac = Mac<Platform, 1, 32, 8, 8, 8, 255, 32, 4, 2, 0>;
type Handle = MacHandle<'static, Platform, 1, 32, 8, 8, 8, 255, 32, 4, 2, 0>;

/// What the device's settings give the node: its channels' keys, and the
/// peers and administrators it is to know.
#[derive(Clone, Default)]
struct Domain {
    channels: Vec<[u8; 32]>,
    peers: Vec<[u8; 32]>,
}

/// The device node, running.
pub struct Mesh {
    runtime: Runtime,
    local: LocalSet,
    device: Rc<RefCell<SoftDevice<HostPlatform>>>,
    /// The device-domain version the node was last given its domain at.
    version: Cell<Option<u32>>,
    domain: watch::Sender<Domain>,
}

impl Mesh {
    /// Brings the node up as `node`'s device identity, with its frame
    /// counters kept under `dir`.
    pub fn start(dir: &Path, node: &'static Node) -> io::Result<Self> {
        let device = node.with_port(|port| port.device().clone());
        let secret = device
            .borrow_mut()
            .platform_mut()
            .load_identity()
            .ok_or_else(|| io::Error::other("the device has no identity"))?;
        let identity = SoftwareIdentity::from_secret_bytes(&secret);
        let key = *identity.public_key();

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let radio = {
            let _context = runtime.enter();
            UdpMulticastRadio::bind(
                LanConfig::lan(),
                RxMode::Bearer(BearerId::LAN),
                UdpMulticastRadio::SIMULATED_T_FRAME_MS,
            )?
        };
        let mac: &'static AsyncRefCell<MeshMac> = Box::leak(Box::new(AsyncRefCell::new(Mac::new(
            radio,
            CryptoEngine::new(SoftwareAes, SoftwareSha256),
            StdClock::new(),
            rand::rng(),
            TokioFileCounterStore::new(dir.join("counters"))?,
            RepeaterConfig::default(),
            OperatingPolicy::default(),
        ))));
        let handle = MacHandle::new(mac);
        let local = LocalSet::new();
        let (host, mesh_node) = local.block_on(&runtime, async {
            let id = handle
                .add_identity(identity)
                .await
                .map_err(|error| io::Error::other(format!("identity: {error:?}")))?;
            handle
                .load_persisted_counter(id)
                .await
                .map_err(|error| io::Error::other(format!("frame counter: {error:?}")))?;
            // A stranger that sends its full key is known for as long as
            // there is room, as on the board.
            handle.set_auto_register_full_key_peers(true).await;
            let mut host = Host::new(handle);
            let mesh_node = host.add_node(id);
            io::Result::Ok((host, mesh_node))
        })?;

        let inbox: &'static Inbox<NoopRawMutex> = Box::leak(Box::new(Inbox::new()));
        // The taps live as long as the node.
        std::mem::forget(task::listen(
            &mesh_node,
            inbox,
            key,
            WellKnownChannels::derive(),
        ));
        let (domain, followed) = watch::channel(Domain::default());
        local.spawn_local(pump(host, handle));
        local.spawn_local(follow(mesh_node.clone(), followed));
        local.spawn_local(chats(node, mesh_node, inbox));
        Ok(Self {
            runtime,
            local,
            device,
            version: Cell::new(None),
            domain,
        })
    }

    /// Lets the node and the chats run for `span`, holding the channels
    /// and peers the device's settings now hold.
    pub fn run_for(&self, span: Duration) {
        {
            let device = self.device.borrow();
            let session = device.session();
            let version = session.dev_domain_version();
            if self.version.replace(Some(version)) != Some(version) {
                self.domain.send_replace(Domain {
                    channels: session.dev_channel_keys().collect(),
                    peers: session.dev_peers().chain(session.dev_admins()).collect(),
                });
            }
        }
        self.runtime
            .block_on(self.local.run_until(tokio::time::sleep(span)));
    }
}

async fn pump(mut host: Host<Handle>, handle: Handle) {
    loop {
        if let Err(error) = host.pump_once().await {
            eprintln!("host-communicator: the device node stopped: {error:?}");
            return;
        }
        if let Err(error) = handle.service_counter_persistence().await {
            eprintln!("host-communicator: frame counters not saved: {error:?}");
        }
    }
}

/// Joins and leaves channels, and knows and forgets peers, as the
/// device's settings do.
async fn follow(node: LocalNode<Handle>, mut followed: watch::Receiver<Domain>) {
    let mut joined: Vec<[u8; 32]> = Vec::new();
    let mut known: Vec<[u8; 32]> = Vec::new();
    while followed.changed().await.is_ok() {
        let domain = followed.borrow_and_update().clone();
        for key in joined.extract_if(.., |key| !domain.channels.contains(key)) {
            let _ = node.leave(&Channel::private(ChannelKey(key), "")).await;
        }
        for key in domain.channels {
            if !joined.contains(&key)
                && node
                    .join(&Channel::private(ChannelKey(key), ""))
                    .await
                    .is_ok()
            {
                joined.push(key);
            }
        }
        for key in known.extract_if(.., |key| !domain.peers.contains(key)) {
            node.remove_peer(&PublicKey(key)).await;
        }
        for key in domain.peers {
            if !known.contains(&key) && node.peer(PublicKey(key)).await.is_ok() {
                known.push(key);
            }
        }
    }
}

async fn chats(node: &'static Node, mesh: LocalNode<Handle>, inbox: &'static Inbox<NoopRawMutex>) {
    task::run(
        node,
        &mesh,
        inbox,
        |at| {
            let now = node.clock().now_ms();
            tokio::time::sleep(Duration::from_millis(at.saturating_sub(now)))
        },
        // The window takes the node's events every frame.
        || {},
    )
    .await
}
