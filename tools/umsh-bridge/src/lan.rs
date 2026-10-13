//! A participant's membership in a LAN bearer group, and the loop that
//! relays frames between it and one queue in each direction.
//!
//! Every datagram is a frame, with no header and no measurements: what
//! arrives from the group crosses as an unmeasured reception, and what
//! leaves for it is the frame alone.
//!
//! No repeater stands between the group and the bridge, so nothing
//! else stops a frame circulating between two bridge participants on
//! the same LAN, or between this one and its own multicast echo. The
//! relay therefore carries a frame across at most once, in either
//! direction, within [`CROSSING_WINDOW`]. That drops nothing a
//! repeater would have carried: every frame it suppresses is one the
//! far side's duplicate caches have already seen.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, Result};
use tokio::time::Instant;
use umsh_lan_bearer::socket::{LanConfig, LanSocket};
use umsh_lan_bearer::{Datagram, MAX_FRAME, classify};
use umsh_ulcp::meta::RxMeta;

use crate::tunnel::{Dequeued, TunnelFrame, TunnelQueue};

/// How long a frame that crossed in either direction is kept from
/// crossing again.
pub const CROSSING_WINDOW: Duration = Duration::from_secs(30);

/// The most crossings remembered. A group busy enough to exceed this
/// within the window shortens the window rather than growing memory.
const CROSSINGS_CAP: usize = 256;

/// How long to wait before rejoining a group that failed—a network
/// not up yet, or an interface that went away.
const REJOIN_DELAY: Duration = Duration::from_secs(3);

pub struct LanRelay {
    config: LanConfig,
    /// Frames heard from the group, for whoever consumes this interface.
    inbound: Arc<TunnelQueue>,
    /// Frames to send to the group.
    outbound: Arc<TunnelQueue>,
}

impl LanRelay {
    pub fn new(config: LanConfig, inbound: Arc<TunnelQueue>, outbound: Arc<TunnelQueue>) -> Self {
        Self {
            config,
            inbound,
            outbound,
        }
    }

    /// Stay joined and relaying for as long as the process runs.
    pub async fn run(&self) -> ! {
        loop {
            if let Err(error) = self.session().await {
                tracing::warn!(group = %self.config.group, "lan: {error:#}");
            }
            self.outbound.clear();
            tokio::time::sleep(REJOIN_DELAY).await;
        }
    }

    async fn session(&self) -> Result<()> {
        let socket = LanSocket::bind(self.config)
            .with_context(|| format!("joining {}", self.config.group))?;
        tracing::info!(group = %self.config.group, "lan joined");

        let metadata = unmeasured();
        let mut crossings = Crossings::default();
        let mut datagram = [0u8; MAX_FRAME + 1];
        let generation = self.outbound.generation();
        loop {
            tokio::select! {
                received = socket.recv(&mut datagram) => {
                    let (len, source) = received.context("receiving from the group")?;
                    let frame = &datagram[..len];
                    if classify(frame) == Datagram::Malformed {
                        tracing::debug!(%source, len, "dropping a datagram that is no frame");
                        continue;
                    }
                    if !crossings.admit(frame, Instant::now()) {
                        continue;
                    }
                    tracing::trace!(%source, len, "lan rx");
                    if self.inbound.push(TunnelFrame::new(frame.to_vec(), metadata.clone())) {
                        tracing::warn!("inbound queue full; dropped the oldest frame");
                    }
                }
                dequeued = core::future::poll_fn(|cx| self.outbound.poll_pop(generation, cx)) => {
                    let Dequeued::Frame(frame) = dequeued else {
                        return Ok(());
                    };
                    if frame.data.is_empty() || frame.data.len() > MAX_FRAME {
                        continue;
                    }
                    if !crossings.admit(&frame.data, Instant::now()) {
                        continue;
                    }
                    // A send that fails is lost the way a collided
                    // transmission is; a network that is gone shows up
                    // on the receive side.
                    if let Err(error) = socket.send(&frame.data).await {
                        tracing::debug!("lan send failed: {error}");
                    }
                }
            }
        }
    }
}

/// Receive metadata for a frame from the group: nothing was measured.
fn unmeasured() -> Vec<u8> {
    let mut metadata = [0u8; RxMeta::WIRE_LEN];
    RxMeta {
        rssi_dbm: None,
        lqi: None,
        snr_cb: None,
    }
    .encode(&mut metadata)
    .expect("buffer sized with WIRE_LEN");
    metadata.to_vec()
}

/// The frames that crossed recently, in either direction.
#[derive(Default)]
struct Crossings {
    recent: VecDeque<(Instant, Vec<u8>)>,
}

impl Crossings {
    /// Whether `frame` may cross now, recording that it did.
    fn admit(&mut self, frame: &[u8], now: Instant) -> bool {
        while let Some((at, _)) = self.recent.front()
            && now.saturating_duration_since(*at) > CROSSING_WINDOW
        {
            self.recent.pop_front();
        }
        if self.recent.iter().any(|(_, seen)| seen == frame) {
            return false;
        }
        if self.recent.len() == CROSSINGS_CAP {
            self.recent.pop_front();
        }
        self.recent.push_back((now, frame.to_vec()));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_crosses_once_in_either_direction_until_the_window_passes() {
        let mut crossings = Crossings::default();
        let start = Instant::now();
        assert!(crossings.admit(&[0xC0, 1], start));
        // Its own echo, or the same frame back from another bridge.
        assert!(!crossings.admit(&[0xC0, 1], start + Duration::from_secs(1)));
        assert!(crossings.admit(&[0xC0, 2], start + Duration::from_secs(1)));
        assert!(crossings.admit(&[0xC0, 1], start + CROSSING_WINDOW + Duration::from_secs(1)));
    }

    #[test]
    fn the_memory_is_bounded() {
        let mut crossings = Crossings::default();
        let now = Instant::now();
        for tag in 0..=CROSSINGS_CAP as u16 {
            assert!(crossings.admit(&tag.to_be_bytes(), now));
        }
        assert_eq!(crossings.recent.len(), CROSSINGS_CAP);
        // The oldest made room.
        assert!(crossings.admit(&0u16.to_be_bytes(), now));
    }
}
