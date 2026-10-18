//! Engine transmissions to `LocalNode` sends, and their delivery back to the
//! engine.

use alloc::collections::{BTreeSet, VecDeque};
use alloc::vec::Vec;

use umsh_core::{ChannelKey, ChannelTag, PayloadType, PublicKey};
use umsh_mac::{SendOptions, SendReceipt};
use umsh_node::{BoundChannel, LocalNode, MacBackend, SendProgressTicket, Transport};

use crate::chat::{ChannelRegistry, ChatFacade, Outbound, TransmitUpdate};
use crate::engine::{Destination, Transmission};
use crate::node_adapter::WellKnownChannels;
use crate::validate::TextProfile;

/// The flood-hop ceiling the protocol fixes for `public` and `EMERGENCY`.
pub const WELL_KNOWN_CHANNEL_MAX_FLOOD_HOPS: u8 = 10;

/// How long an in-flight transmission may go unresolved before it is called
/// out. Longer than any ordinary acknowledgment wait, so this fires on
/// trouble rather than on a slow link.
pub const TRANSMISSION_STALL_MS: u64 = 60_000;

/// What a send needs to know beyond the frame itself.
#[derive(Clone, Copy)]
pub struct TransmitPolicy {
    /// Ceiling on how far a send travels when nothing narrower applies. Zero
    /// sends no flood at all.
    pub default_flood_hops: u8,
    pub well_known: WellKnownChannels,
}

/// Spend `hops` on `options`: a budget for a flood, or no flood at all for
/// zero.
pub fn with_flood_reach(options: SendOptions, hops: u8) -> SendOptions {
    if hops == 0 {
        options.no_flood()
    } else {
        options.with_flood_hops(hops)
    }
}

/// How far traffic on a channel may travel: the ceiling the user registered
/// for it, otherwise `default_hops`—and never past the ceiling the protocol
/// fixes for the well-known `public` and `EMERGENCY` channels.
pub fn channel_reach(
    channels: &ChannelRegistry,
    well_known: &WellKnownChannels,
    channel: &ChannelTag,
    default_hops: u8,
) -> u8 {
    let asked = channels.max_flood_hops(channel).unwrap_or(default_hops);
    match channels.key(channel) {
        Some(key) if well_known.contains(&key) => asked.min(WELL_KNOWN_CHANNEL_MAX_FLOOD_HOPS),
        _ => asked,
    }
}

/// Bind a held channel, so it can be sent over.
///
/// Looked up per send rather than cached: leaving and rejoining a channel
/// invalidates the binding, and the registry is the one place that knows.
pub fn bound_channel<M: MacBackend>(
    node: &LocalNode<M>,
    channels: &ChannelRegistry,
    channel: &ChannelTag,
) -> Option<BoundChannel<M>> {
    let key = channels.key(channel)?;
    node.bound_channel(&umsh_node::Channel::private(key, ""))
}

/// A transmission that has stayed in flight past [`TRANSMISSION_STALL_MS`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stall {
    pub transmission_id: u32,
    pub waited_ms: u64,
    /// Window slots in use, of `window`.
    pub occupied: usize,
    pub window: usize,
    /// Transmissions waiting behind the window.
    pub pending: usize,
}

/// The chat transmit queue.
///
/// Frames go to the node through a bounded window, so a message larger than
/// the window waits here rather than in the MAC's own queue. A frame leaves
/// the window when nothing more can happen to it: acknowledged or failed, or
/// transmitted when no acknowledgment will follow.
pub struct TransmitQueue {
    window: usize,
    pending: VecDeque<Queued>,
    in_flight: Vec<InFlight>,
    /// Peers whose first frame has been acknowledged.
    pipeline_ready: BTreeSet<[u8; 32]>,
}

struct Queued {
    session_id: u64,
    transmission: Transmission,
}

struct InFlight {
    session_id: u64,
    transmission_id: u32,
    /// The peer whose first-contact pipeline this send gates on. Channel
    /// sends have no such peer: multicast is unaddressed and blind unicast
    /// carries no acknowledgment to confirm with.
    gate_peer: Option<PublicKey>,
    ticket: SendProgressTicket,
    sent_reported: bool,
    /// No acknowledgment will ever arrive, so transmission is the terminal
    /// success state rather than a step toward one.
    non_ack: bool,
    /// When this entry entered the window, and whether it has already been
    /// reported as overdue. A frame that never resolves holds a slot; a full
    /// window of them stops chat entirely, which from the outside looks like
    /// messages hanging on "Sending" for no reason.
    queued_at_ms: u64,
    stall_reported: bool,
}

impl TransmitQueue {
    /// A queue keeping at most `window` frames in flight, aligned with the
    /// MAC's transmit queue.
    pub fn new(window: usize) -> Self {
        Self {
            window: window.max(1),
            pending: VecDeque::new(),
            in_flight: Vec::new(),
            pipeline_ready: BTreeSet::new(),
        }
    }

    pub fn enqueue(&mut self, outbound: Outbound) {
        let session_id = outbound.session_id();
        self.pending.extend(
            outbound
                .into_transmissions()
                .into_iter()
                .map(|transmission| Queued {
                    session_id,
                    transmission,
                }),
        );
    }

    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    pub fn in_flight_len(&self) -> usize {
        self.in_flight.len()
    }

    /// Hand waiting frames to the node while the window has room. Returns
    /// how many were sent: each is an authenticated send that advanced the
    /// frame counter.
    ///
    /// - A unicast asks for an acknowledgment, and a peer gets one frame in
    ///   flight until its first is acknowledged: first contact may need
    ///   counter synchronization.
    /// - A channel multicast carries the full source key, so a member who
    ///   misses a fragment can address a resend request to us.
    /// - A channel-direct frame is a blind unicast, registering the peer on
    ///   the way out.
    /// - `EMERGENCY` goes out unencrypted, so any node in range can read it.
    ///
    /// A frame that cannot be sent—an unregistrable peer, a channel no longer
    /// held—fails at once. One the node refuses for now, which with a
    /// registered peer and an engine-bounded payload means its queues are
    /// full, waits for tickets to advance.
    pub async fn pump<M, P, const SLOTS: usize, const PAGES: usize>(
        &mut self,
        node: &LocalNode<M>,
        chat: &mut ChatFacade<P, SLOTS, PAGES>,
        policy: &TransmitPolicy,
        now_ms: u64,
    ) -> usize
    where
        M: MacBackend,
        P: TextProfile + Clone,
    {
        let mut sent_count = 0;
        while self.in_flight.len() < self.window {
            let Some(queued) = self.pending.pop_front() else {
                break;
            };
            let transmission = &queued.transmission;
            let gate_peer = match transmission.destination {
                Destination::Peer(peer) => Some(peer),
                Destination::Channel(_) | Destination::ChannelPeer { .. } => None,
            };
            if let Some(peer) = gate_peer
                && !self.pipeline_ready.contains(&peer.0)
                && self
                    .in_flight
                    .iter()
                    .any(|entry| entry.gate_peer == Some(peer))
            {
                self.pending.push_front(queued);
                break;
            }
            let mut payload = Vec::with_capacity(transmission.payload.len() + 1);
            payload.push(PayloadType::TextMessage as u8);
            payload.extend_from_slice(transmission.payload.as_slice());
            let sent = match send(node, chat.channels(), policy, transmission, &payload).await {
                Sent::Ticket(ticket) => ticket,
                Sent::Unsendable => {
                    chat.transmit_update(
                        queued.session_id,
                        transmission.transmission_id,
                        TransmitUpdate::Failed,
                        now_ms,
                    );
                    continue;
                }
                Sent::Busy => {
                    self.pending.push_front(queued);
                    break;
                }
            };
            self.in_flight.push(InFlight {
                session_id: queued.session_id,
                transmission_id: transmission.transmission_id,
                gate_peer,
                ticket: sent,
                sent_reported: false,
                non_ack: gate_peer.is_none(),
                queued_at_ms: now_ms,
                stall_reported: false,
            });
            sent_count += 1;
        }
        sent_count
    }

    /// Report what the tickets say to the engine: transmitted, acknowledged,
    /// or failed. Returns the frames newly found overdue.
    pub fn service<P, const SLOTS: usize, const PAGES: usize>(
        &mut self,
        chat: &mut ChatFacade<P, SLOTS, PAGES>,
        now_ms: u64,
    ) -> Vec<Stall>
    where
        P: TextProfile + Clone,
    {
        let occupied = self.in_flight.len();
        let pending = self.pending.len();
        let mut stalls = Vec::new();
        let mut index = 0;
        while index < self.in_flight.len() {
            let entry = &mut self.in_flight[index];
            // Checked before the state transitions below, so an entry about
            // to retire this pass is not accused on its way out.
            let waited_ms = now_ms.saturating_sub(entry.queued_at_ms);
            if !entry.stall_reported
                && !entry.ticket.was_transmitted()
                && waited_ms >= TRANSMISSION_STALL_MS
            {
                entry.stall_reported = true;
                stalls.push(Stall {
                    transmission_id: entry.transmission_id,
                    waited_ms,
                    occupied,
                    window: self.window,
                    pending,
                });
            }
            if entry.ticket.was_transmitted() && !entry.sent_reported {
                entry.sent_reported = true;
                chat.transmit_update(
                    entry.session_id,
                    entry.transmission_id,
                    TransmitUpdate::Sent,
                    now_ms,
                );
            }
            let entry = &self.in_flight[index];
            if entry.non_ack && entry.sent_reported {
                // Nothing further can happen to this one; retiring it here is
                // what keeps channel sends from accumulating in flight.
                self.in_flight.swap_remove(index);
            } else if entry.ticket.was_acked() {
                if let Some(peer) = entry.gate_peer {
                    self.pipeline_ready.insert(peer.0);
                }
                chat.transmit_update(
                    entry.session_id,
                    entry.transmission_id,
                    TransmitUpdate::Acked {
                        via_flood: entry.ticket.was_acked_via_route_retry(),
                    },
                    now_ms,
                );
                self.in_flight.swap_remove(index);
            } else if entry.ticket.has_failed() {
                chat.transmit_update(
                    entry.session_id,
                    entry.transmission_id,
                    TransmitUpdate::Failed,
                    now_ms,
                );
                self.in_flight.swap_remove(index);
            } else {
                index += 1;
            }
        }
        stalls
    }

    /// Fail every frame the queue holds, waiting or in flight. Returns the
    /// receipts of in-flight sends still awaiting an acknowledgment, for the
    /// caller to cancel at the MAC.
    pub fn fail_all<P, const SLOTS: usize, const PAGES: usize>(
        &mut self,
        chat: &mut ChatFacade<P, SLOTS, PAGES>,
        now_ms: u64,
    ) -> Vec<SendReceipt>
    where
        P: TextProfile + Clone,
    {
        for queued in self.pending.drain(..) {
            chat.transmit_update(
                queued.session_id,
                queued.transmission.transmission_id,
                TransmitUpdate::Failed,
                now_ms,
            );
        }
        let mut receipts = Vec::new();
        for entry in self.in_flight.drain(..) {
            if let Some(receipt) = entry.ticket.receipt() {
                receipts.push(receipt);
            }
            chat.transmit_update(
                entry.session_id,
                entry.transmission_id,
                TransmitUpdate::Failed,
                now_ms,
            );
        }
        receipts
    }
}

enum Sent {
    Ticket(SendProgressTicket),
    /// Nothing will make this frame sendable; fail it.
    Unsendable,
    /// The node refused it for now; try again later.
    Busy,
}

async fn send<M: MacBackend>(
    node: &LocalNode<M>,
    channels: &ChannelRegistry,
    policy: &TransmitPolicy,
    transmission: &Transmission,
    payload: &[u8],
) -> Sent {
    let result = match transmission.destination {
        Destination::Peer(peer) => {
            let Ok(connection) = node.peer(peer).await else {
                return Sent::Unsendable;
            };
            let options = with_flood_reach(SendOptions::default(), policy.default_flood_hops)
                .with_ack_requested(true);
            connection.send(payload, &options).await.map_err(|_| ())
        }
        Destination::Channel(channel) => {
            let (Some(key), Some(bound)) = (
                channels.key(&channel),
                bound_channel(node, channels, &channel),
            ) else {
                return Sent::Unsendable;
            };
            let options = channel_options(channels, policy, &channel, &key);
            bound.send_all(payload, &options).await.map_err(|_| ())
        }
        Destination::ChannelPeer { channel, peer } => {
            let (Some(key), Some(bound)) = (
                channels.key(&channel),
                bound_channel(node, channels, &channel),
            ) else {
                return Sent::Unsendable;
            };
            // The MAC will only address a registered peer, and a channel
            // member is not one: nothing about being in a channel together
            // registers anybody. Registering on the way out spends a peer
            // slot only on the members we have to ask something of.
            if node.peer(peer).await.is_err() {
                return Sent::Unsendable;
            }
            // A repair request; the engine owns retrying it, so no
            // acknowledgment is asked for. Repairs carry the same message the
            // multicast did, so they are held to the same rules.
            let options = channel_options(channels, policy, &channel, &key);
            bound.send(&peer, payload, &options).await.map_err(|_| ())
        }
    };
    match result {
        Ok(ticket) => Sent::Ticket(ticket),
        Err(()) => Sent::Busy,
    }
}

/// Options for a frame on a channel: as far as the channel reaches, naming
/// us by full key, and on `EMERGENCY` unencrypted.
///
/// An emergency message only members can read is not an emergency message.
/// The full source key keeps it attributable without encryption. An
/// unencrypted blind unicast still names its channel, which is what a
/// repeater decides to forward on. The spec also asks `EMERGENCY` chat to be
/// signed, but the text-message format has no signature yet, so none is sent.
fn channel_options(
    channels: &ChannelRegistry,
    policy: &TransmitPolicy,
    channel: &ChannelTag,
    key: &ChannelKey,
) -> SendOptions {
    let reach = channel_reach(
        channels,
        &policy.well_known,
        channel,
        policy.default_flood_hops,
    );
    let options = with_flood_reach(SendOptions::default(), reach).with_full_source();
    if policy.well_known.is_emergency(key) {
        options.unencrypted()
    } else {
        options
    }
}
