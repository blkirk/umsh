//! Group membership with echo suppression.
//!
//! Two sockets. The receiving one binds the group's port, shared with
//! every other participant on the host, and joins the group. The sending
//! one binds an ephemeral port of its own, so the source address every
//! receiver sees is unique to this participant—which is what lets this
//! participant recognize and drop its own datagrams when multicast
//! loopback hands them back, however many other participants share the
//! host and however many frames are in flight.

use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket as StdUdpSocket};
use std::task::{Context, Poll};

use socket2::{Domain, Protocol, Socket, Type};
use tokio::io::ReadBuf;
use tokio::net::UdpSocket;

use crate::{DEFAULT_GROUP, DEFAULT_PORT, MAX_FRAME, MULTICAST_TTL};

/// Where and how to join the group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LanConfig {
    /// The group address and port.
    pub group: SocketAddrV4,
    /// The interface to join on, by its address. Unspecified picks the
    /// interface the host routes the group through.
    pub interface: Ipv4Addr,
    /// Whether this host's other participants hear what this one sends.
    /// Off for a host with one participant, such as a radio; on for a
    /// desktop that may run several.
    pub loopback: bool,
}

impl LanConfig {
    /// The default group on the routed interface, with loopback on.
    pub fn lan() -> Self {
        Self {
            group: SocketAddrV4::new(Ipv4Addr::from(DEFAULT_GROUP), DEFAULT_PORT),
            interface: Ipv4Addr::UNSPECIFIED,
            loopback: true,
        }
    }

    /// A group confined to this host: joined on the loopback interface,
    /// where only this host's own participants hear it. What the desktop
    /// simulations and the tests use.
    pub fn host_local(group: Ipv4Addr, port: u16) -> Self {
        Self {
            group: SocketAddrV4::new(group, port),
            interface: Ipv4Addr::LOCALHOST,
            loopback: true,
        }
    }
}

/// A participant's membership in the group.
pub struct LanSocket {
    recv: UdpSocket,
    send: UdpSocket,
    /// The address every datagram this participant sends comes from.
    own: SocketAddr,
    group: SocketAddrV4,
}

impl LanSocket {
    /// Join the group described by `config`.
    pub fn bind(config: LanConfig) -> io::Result<Self> {
        if !config.group.ip().is_multicast() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the group must be an IPv4 multicast address",
            ));
        }
        let interface = match config.interface {
            Ipv4Addr::UNSPECIFIED => routed_interface(config.group)?,
            interface => interface,
        };

        // Sending: an address of its own, on the interface the group is
        // joined on, so the source every receiver sees is exactly the
        // one recorded here.
        let send = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
        send.set_multicast_if_v4(&interface)?;
        send.set_multicast_ttl_v4(MULTICAST_TTL)?;
        send.set_multicast_loop_v4(config.loopback)?;
        send.bind(&SocketAddrV4::new(interface, 0).into())?;
        send.set_nonblocking(true)?;
        let send: StdUdpSocket = send.into();
        let own = send.local_addr()?;
        let send = UdpSocket::from_std(send)?;

        // Receiving: the group's port, shared with every participant on
        // the host.
        let recv = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
        recv.set_reuse_address(true)?;
        #[cfg(unix)]
        recv.set_reuse_port(true)?;
        recv.bind(&SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, config.group.port()).into())?;
        recv.join_multicast_v4(config.group.ip(), &interface)?;
        recv.set_nonblocking(true)?;
        let recv = UdpSocket::from_std(recv.into())?;

        Ok(Self {
            recv,
            send,
            own,
            group: config.group,
        })
    }

    /// The source address of everything this participant sends.
    pub fn local_addr(&self) -> SocketAddr {
        self.own
    }

    /// The group this participant joined.
    pub fn group(&self) -> SocketAddrV4 {
        self.group
    }

    /// Send one frame to the group.
    pub async fn send(&self, frame: &[u8]) -> io::Result<()> {
        if frame.len() > MAX_FRAME {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a frame larger than 255 octets cannot be sent",
            ));
        }
        self.send.send_to(frame, self.group).await?;
        Ok(())
    }

    /// Poll for the next datagram another participant sent, copying it
    /// into `buf` and returning its length and source. This
    /// participant's own datagrams are dropped here.
    pub fn poll_recv(
        &self,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<(usize, SocketAddr)>> {
        loop {
            let mut read = ReadBuf::new(buf);
            let source = match self.recv.poll_recv_from(cx, &mut read) {
                Poll::Ready(Ok(source)) => source,
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                Poll::Pending => return Poll::Pending,
            };
            if source == self.own {
                continue;
            }
            return Poll::Ready(Ok((read.filled().len(), source)));
        }
    }

    /// Receive the next datagram another participant sent.
    pub async fn recv(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        std::future::poll_fn(|cx| self.poll_recv(cx, buf)).await
    }
}

/// The address of the interface the host would send to `group` through.
///
/// Connecting a UDP socket sends nothing; it only asks the routing table.
fn routed_interface(group: SocketAddrV4) -> io::Result<Ipv4Addr> {
    let probe = StdUdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))?;
    probe.connect(group)?;
    match probe.local_addr()? {
        SocketAddr::V4(local) if !local.ip().is_unspecified() => Ok(*local.ip()),
        _ => Err(io::Error::new(
            io::ErrorKind::AddrNotAvailable,
            "no IPv4 interface routes to the group",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const GROUP: Ipv4Addr = Ipv4Addr::new(239, 255, 42, 101);

    async fn recv_within(socket: &LanSocket, ms: u64) -> Option<Vec<u8>> {
        let mut buf = [0u8; 512];
        tokio::time::timeout(Duration::from_millis(ms), socket.recv(&mut buf))
            .await
            .ok()
            .map(|result| buf[..result.unwrap().0].to_vec())
    }

    /// Several participants on one host each hear the others and never
    /// themselves, however many frames are in flight.
    #[tokio::test]
    async fn every_participant_hears_the_others_and_never_itself() {
        let config = LanConfig::host_local(GROUP, 47_101);
        let a = LanSocket::bind(config).unwrap();
        let b = LanSocket::bind(config).unwrap();
        let c = LanSocket::bind(config).unwrap();
        assert_ne!(a.local_addr(), b.local_addr());

        for tag in 0..3u8 {
            a.send(&[0xC0, tag]).await.unwrap();
        }
        b.send(&[0xC0, 0xBB]).await.unwrap();

        let mut heard_by_c = Vec::new();
        while let Some(frame) = recv_within(&c, 200).await {
            heard_by_c.push(frame);
        }
        assert_eq!(heard_by_c.len(), 4);

        let mut heard_by_a = Vec::new();
        while let Some(frame) = recv_within(&a, 200).await {
            heard_by_a.push(frame);
        }
        assert_eq!(heard_by_a, [vec![0xC0, 0xBB]]);
    }

    #[tokio::test]
    async fn only_a_multicast_group_and_a_fitting_frame() {
        assert!(
            LanSocket::bind(LanConfig::host_local(Ipv4Addr::new(10, 0, 0, 1), 47_102)).is_err()
        );
        let socket = LanSocket::bind(LanConfig::host_local(GROUP, 47_103)).unwrap();
        assert!(socket.send(&[0xC0; MAX_FRAME + 1]).await.is_err());
    }
}
