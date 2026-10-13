//! UMSH frames over IP multicast on a local network.
//!
//! The LAN bearer carries one frame per UDP datagram, with no header, to
//! an IPv4 multicast group every participant joins. Radios on the LAN
//! forward between the group and their LoRa meshes; desktops and phones
//! reach the mesh through them. Participants advertise themselves, and
//! the LoRa settings they use, over DNS-SD.
//!
//! - [`classify`] and the constants: the datagram rules every participant
//!   applies.
//! - [`txt`]: the DNS-SD TXT schema, `no_std` so a radio's responder and a
//!   host's browser share one codec.
//! - `socket` and `radio` (feature `tokio`): the group membership with
//!   echo suppression, and a [`umsh_hal::Radio`] over it.
//! - `dnssd` (feature `dnssd`): advertising and browsing.
//!
//! Spec: [LAN Bearer](https://darconeous.github.io/umsh/docs/protocol/lan-bearer.html).

#![cfg_attr(not(feature = "std"), no_std)]

pub mod txt;

#[cfg(feature = "dnssd")]
pub mod dnssd;
#[cfg(feature = "tokio")]
pub mod radio;
#[cfg(feature = "tokio")]
pub mod socket;

use umsh_core::{Fcf, UMSH_VERSION};

/// The default multicast group.
pub const DEFAULT_GROUP: [u8; 4] = [239, 255, 134, 121];
/// The default UDP port.
pub const DEFAULT_PORT: u16 = 15056;
/// The multicast TTL senders use. Large enough to cross whatever multicast
/// routing a network is configured with; where the group stops is the
/// network's decision, made by its routers and the group's scope.
pub const MULTICAST_TTL: u32 = 16;
/// The largest frame a datagram may carry.
pub const MAX_FRAME: usize = 255;
/// The DNS-SD service type participants advertise under.
pub const SERVICE_TYPE: &str = "_umsh._udp";

/// The MeshCore v1 version bits, which share the first octet's top two
/// bits with UMSH's.
const MESHCORE_V1_VERSION: u8 = 0b00;

/// What a datagram carries, judged by its length and its first octet
/// alone—as a radio judges a reception, before parsing anything.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Datagram {
    /// An UMSH frame.
    Umsh,
    /// A MeshCore v1 frame.
    MeshCoreV1,
    /// Empty, larger than [`MAX_FRAME`], or of an unknown protocol
    /// version: dropped with accounting.
    Malformed,
}

/// Classify one datagram.
pub fn classify(datagram: &[u8]) -> Datagram {
    let Some(&first) = datagram.first() else {
        return Datagram::Malformed;
    };
    if datagram.len() > MAX_FRAME {
        return Datagram::Malformed;
    }
    let fcf = Fcf(first);
    if fcf.version() == UMSH_VERSION && fcf.reserved_valid() {
        Datagram::Umsh
    } else if fcf.version() == MESHCORE_V1_VERSION {
        Datagram::MeshCoreV1
    } else {
        Datagram::Malformed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UMSH: u8 = 0b1100_0000;

    #[test]
    fn datagrams_are_judged_by_their_first_octet() {
        assert_eq!(classify(&[]), Datagram::Malformed);
        assert_eq!(classify(&[UMSH, 1, 2]), Datagram::Umsh);
        assert_eq!(classify(&[0b0000_0000, 1]), Datagram::MeshCoreV1);
        assert_eq!(classify(&[0b0100_0000, 1]), Datagram::Malformed);
        assert_eq!(classify(&[UMSH; MAX_FRAME + 1]), Datagram::Malformed);
        assert_eq!(classify(&[UMSH; MAX_FRAME]), Datagram::Umsh);
    }
}
