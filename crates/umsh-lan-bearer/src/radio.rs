//! A [`Radio`] whose medium is the group.
//!
//! Two uses, told apart by [`RxMode`]:
//!
//! - **A bearer.** Receptions are what they are: unmeasured, from
//!   [`RxOrigin::Bearer`]. This is what a participant on a real LAN uses.
//! - **A simulated radio channel.** Receptions claim to have come off the
//!   air, with fixed signal values, so a desktop simulation exercises the
//!   paths a LoRa reception takes—contention ranked by signal, route
//!   scoring, the signal gates. Nothing on a real LAN should use this.

use std::collections::VecDeque;
use std::io;
use std::net::Ipv4Addr;
use std::task::{Context, Poll};

use umsh_hal::{BearerId, Radio, RxInfo, RxOrigin, Snr, TxError, TxOptions};

use crate::MAX_FRAME;
use crate::socket::{LanConfig, LanSocket};

/// How receptions are reported.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RxMode {
    /// Unmeasured receptions from the bearer.
    Bearer(BearerId),
    /// Receptions off a simulated air, with these signal values.
    SimulatedAir { rssi: i16, snr: Snr },
}

/// Errors from [`UdpMulticastRadio`].
#[derive(Debug)]
pub enum UdpMulticastRadioError {
    Io(io::Error),
    FrameTooLarge(usize),
}

impl From<io::Error> for UdpMulticastRadioError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// A [`Radio`] over a [`LanSocket`].
pub struct UdpMulticastRadio {
    socket: LanSocket,
    mode: RxMode,
    t_frame_ms: u32,
    recv_buf: [u8; MAX_FRAME + 1],
    /// Frames this radio sent whose echoes have not yet come back.
    unheard: VecDeque<Vec<u8>>,
}

/// How many sent frames are remembered for their echoes.
const UNHEARD_CAP: usize = 16;

impl UdpMulticastRadio {
    /// The frame time a simulated channel reports: LoRa-like, so retry
    /// and backoff timing on a desktop resemble a real deployment.
    pub const SIMULATED_T_FRAME_MS: u32 = 800;

    /// Join the group as described, reporting receptions as `mode`, with
    /// `t_frame_ms` as the frame time the MAC times against.
    pub fn bind(config: LanConfig, mode: RxMode, t_frame_ms: u32) -> io::Result<Self> {
        Ok(Self {
            socket: LanSocket::bind(config)?,
            mode,
            t_frame_ms,
            recv_buf: [0; MAX_FRAME + 1],
            unheard: VecDeque::new(),
        })
    }

    /// A simulated radio channel confined to this host: the desktop
    /// examples' and the tests' shared medium.
    pub async fn bind_v4(group: Ipv4Addr, port: u16) -> Result<Self, UdpMulticastRadioError> {
        Ok(Self::bind(
            LanConfig::host_local(group, port),
            RxMode::SimulatedAir {
                rssi: -40,
                snr: Snr::from_decibels(10),
            },
            Self::SIMULATED_T_FRAME_MS,
        )?)
    }

    /// The membership underneath.
    pub fn socket(&self) -> &LanSocket {
        &self.socket
    }
}

impl Radio for UdpMulticastRadio {
    type Error = UdpMulticastRadioError;

    async fn transmit(
        &mut self,
        data: &[u8],
        _options: TxOptions,
    ) -> Result<(), TxError<Self::Error>> {
        if data.len() > MAX_FRAME {
            return Err(TxError::Io(UdpMulticastRadioError::FrameTooLarge(
                data.len(),
            )));
        }
        self.socket
            .send(data)
            .await
            .map_err(|error| TxError::Io(UdpMulticastRadioError::Io(error)))?;
        if self.unheard.len() == UNHEARD_CAP {
            self.unheard.pop_front();
        }
        self.unheard.push_back(data.to_vec());
        Ok(())
    }

    fn poll_receive(
        &mut self,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<RxInfo, Self::Error>> {
        loop {
            let len = match self.socket.poll_recv(cx, &mut self.recv_buf) {
                Poll::Ready(Ok((len, _source))) => len,
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error.into())),
                Poll::Pending => return Poll::Pending,
            };
            // Empty and oversized datagrams are nobody's frame. The
            // receive buffer is one larger than a frame so that an
            // oversized datagram is seen as one rather than truncated
            // into something plausible.
            if len == 0 || len > MAX_FRAME || len > buf.len() {
                continue;
            }
            // A radio does not hear itself. Every participant shares the
            // group's port, so its echo is known by content, not source.
            let frame = &self.recv_buf[..len];
            if let Some(index) = self.unheard.iter().position(|sent| sent == frame) {
                self.unheard.remove(index);
                continue;
            }
            buf[..len].copy_from_slice(frame);
            let (rssi, snr, origin) = match self.mode {
                RxMode::Bearer(bearer) => (0, Snr::from_centibels(0), RxOrigin::Bearer(bearer)),
                RxMode::SimulatedAir { rssi, snr } => (rssi, snr, RxOrigin::Air),
            };
            return Poll::Ready(Ok(RxInfo {
                len,
                rssi,
                snr,
                lqi: None,
                origin,
                buffered: None,
            }));
        }
    }

    fn max_frame_size(&self) -> usize {
        MAX_FRAME
    }

    fn t_frame_ms(&self) -> u32 {
        self.t_frame_ms
    }
}
