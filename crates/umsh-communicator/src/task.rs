//! Carrying frames between the device node and the chats.
//!
//! The device node's callbacks run inside its pump, where the chats are
//! not at hand: [`listen`] copies what the chats need into an [`Inbox`],
//! and wakes it on word of a frame sent. [`run`] feeds the inbox to the
//! communicator's node, sends the chats' frames, carries out what else
//! they ask of the device node, and keeps their timers. Nothing here
//! names an executor, so a board runs it on whichever it has.

use alloc::collections::VecDeque;
use alloc::vec;
use alloc::vec::Vec;
use core::cell::RefCell;
use core::future::Future;

use embassy_futures::select::select3;
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::RawMutex;
use embassy_sync::signal::Signal;
use umsh_core::{ChannelKey, NodeHint, PayloadType, PublicKey};
use umsh_mac::SendOptions;
use umsh_node::{
    LocalNode, MacBackend, NodeIdentityPayload, Subscription, identity_is_trustworthy,
};
use umsh_text::node_adapter::{
    Inbound, InboundText, RxSummary, WellKnownChannels, inbound_text, transmit,
};
use umsh_ulcp_admin::LocalPort;

use crate::node::CommunicatorNode;
use crate::store::ChatStore;

/// How many arrivals wait for the chats. Past it, the oldest identity or
/// answer goes first, then the oldest text frame.
pub const INBOX_FRAMES: usize = 16;

/// How often the chats' timers are advanced: reassembly, notification,
/// and repair all run on the scale of seconds.
pub const TICK_MS: u64 = 1_000;

/// How long a ping waits for its answer.
const PING_TIMEOUT_MS: u64 = 30_000;

/// What a ping carries beyond its nonce, so it measures a link as a
/// message would use it.
const PING_PADDING: usize = 6;

/// What the device node received for the chats.
pub enum Arrival {
    /// A text frame.
    Text(InboundText),
    /// A node's identity, as the node itself claims it.
    Identity {
        key: PublicKey,
        identity: NodeIdentityPayload,
        rx: RxSummary,
    },
    /// A peer answered a ping.
    Pong {
        peer: PublicKey,
        round_trip_ms: u64,
        rx: RxSummary,
    },
}

/// What the chats ask of the device node besides sending their frames.
pub enum Command {
    /// Ping a peer.
    Ping {
        peer: PublicKey,
        options: SendOptions,
    },
    /// Ask a channel for the identity of whoever holds a hint.
    AskChannel {
        channel: ChannelKey,
        hint: NodeHint,
        options: SendOptions,
    },
}

/// What the device node received, waiting for the chats.
pub struct Inbox<R: RawMutex> {
    arrivals: Mutex<R, RefCell<VecDeque<Arrival>>>,
    ready: Signal<R, ()>,
}

impl<R: RawMutex> Inbox<R> {
    pub const fn new() -> Self {
        Self {
            arrivals: Mutex::new(RefCell::new(VecDeque::new())),
            ready: Signal::new(),
        }
    }

    pub fn push(&self, arrival: Arrival) {
        self.arrivals.lock(|arrivals| {
            let mut arrivals = arrivals.borrow_mut();
            if arrivals.len() >= INBOX_FRAMES {
                // A message outweighs word of who is around.
                let oldest = arrivals
                    .iter()
                    .position(|arrival| !matches!(arrival, Arrival::Text(_)))
                    .unwrap_or(0);
                arrivals.remove(oldest);
            }
            arrivals.push_back(arrival);
        });
        self.ready.signal(());
    }

    /// Wakes the chat task without an arrival: there is word of a frame
    /// sent.
    pub fn wake(&self) {
        self.ready.signal(());
    }

    fn take(&self) -> Option<Arrival> {
        self.arrivals
            .lock(|arrivals| arrivals.borrow_mut().pop_front())
    }
}

impl<R: RawMutex> Default for Inbox<R> {
    fn default() -> Self {
        Self::new()
    }
}

/// Copies what `node`, the device identity `local_key`'s node, receives
/// for the chats into `inbox`: text frames, other nodes' identities, and
/// answers to pings. It wakes the inbox on word of a frame sent. The
/// channel rules for `public` and `EMERGENCY` are applied here: a frame
/// they refuse is left for any other listener.
///
/// The subscriptions end when they are dropped.
pub fn listen<M, R>(
    node: &LocalNode<M>,
    inbox: &'static Inbox<R>,
    local_key: PublicKey,
    well_known: WellKnownChannels,
) -> Vec<Subscription>
where
    M: MacBackend + 'static,
    R: RawMutex + 'static,
{
    vec![
        node.on_receive(
            move |packet| match inbound_text(packet, &local_key, &well_known) {
                Some(Inbound::Text(text)) => {
                    inbox.push(Arrival::Text(text));
                    true
                }
                // Our own channel message, relayed back: the message is
                // already kept.
                Some(Inbound::Echo { .. }) => true,
                Some(Inbound::Refused(_)) | None => false,
            },
        ),
        // Left for the node's own handling of identities, too.
        node.on_receive(move |packet| {
            if packet.payload_type() == PayloadType::NodeIdentity
                && let Some(key) = packet.from_key()
                && key != local_key
                && let Ok(identity) = NodeIdentityPayload::from_bytes(packet.payload())
                && identity_is_trustworthy(packet, &key, &identity)
            {
                inbox.push(Arrival::Identity {
                    key,
                    identity,
                    rx: RxSummary::from_packet(packet),
                });
            }
            false
        }),
        node.on_pong_with_metadata(move |peer, pong| {
            inbox.push(Arrival::Pong {
                peer,
                round_trip_ms: pong.round_trip_ms,
                rx: RxSummary {
                    rssi_dbm: pong.rssi_dbm,
                    snr_centibels: pong.snr_centibels,
                    lqi: pong.lqi,
                    hop_count: pong.hop_count,
                    route_hints: pong.route_hints.clone(),
                    source_authenticated: true,
                    buffered_age_s: 0,
                },
            });
        }),
        node.on_transmitted(|_| inbox.wake()),
        node.on_ack_received(|_, _| inbox.wake()),
        node.on_ack_timeout(|_, _| inbox.wake()),
    ]
}

/// Feeds what reaches `inbox` to `node`, sends its frames through `mesh`,
/// the device node, and keeps its timers, for as long as the board runs.
/// `sleep_until` waits for a time on the node's clock, and `notify` tells
/// the interface that events wait for it.
pub async fn run<P, S, M, R, F>(
    node: &CommunicatorNode<P, S>,
    mesh: &LocalNode<M>,
    inbox: &Inbox<R>,
    mut sleep_until: impl FnMut(u64) -> F,
    mut notify: impl FnMut(),
) -> !
where
    P: LocalPort,
    S: ChatStore,
    M: MacBackend,
    R: RawMutex,
    F: Future<Output = ()>,
{
    let mut tick_at = node.clock().now_ms();
    loop {
        while let Some(arrival) = inbox.take() {
            node.arrive(arrival);
        }
        node.service();
        if node.clock().now_ms() >= tick_at {
            node.tick();
            tick_at = node.clock().now_ms() + TICK_MS;
        }
        if node.has_events() {
            notify();
        }
        while let Some(command) = node.command() {
            execute(mesh, command).await;
        }
        // Until the device node is full; word of a frame sent makes room.
        while let Some(departure) = node.depart() {
            let sent = transmit(mesh, &departure).await;
            if !node.departed(departure, sent) {
                break;
            }
        }
        if node.has_events() {
            notify();
        }
        select3(inbox.ready.wait(), node.work(), sleep_until(tick_at)).await;
    }
}

/// Carries out `command`. One the device node cannot take now is dropped:
/// each is asked for again as easily as it was asked for.
async fn execute<M: MacBackend>(mesh: &LocalNode<M>, command: Command) {
    match command {
        Command::Ping { peer, options } => {
            if let Ok(connection) = mesh.peer(peer).await {
                _ = connection
                    .ping(PING_PADDING, &options, PING_TIMEOUT_MS)
                    .await;
            }
        }
        Command::AskChannel {
            channel,
            hint,
            options,
        } => ask_channel(mesh, &channel, &hint, &options).await,
    }
}

#[cfg(feature = "software-crypto")]
async fn ask_channel<M: MacBackend>(
    mesh: &LocalNode<M>,
    channel: &ChannelKey,
    hint: &NodeHint,
    options: &SendOptions,
) {
    if let Some(bound) = mesh.bound_channel(&umsh_node::Channel::private(*channel, "")) {
        _ = bound.request_identity(hint, options).await;
    }
}

/// Channels are sealed by the software crypto back end alone.
#[cfg(not(feature = "software-crypto"))]
async fn ask_channel<M: MacBackend>(
    _mesh: &LocalNode<M>,
    _channel: &ChannelKey,
    _hint: &NodeHint,
    _options: &SendOptions,
) {
}
