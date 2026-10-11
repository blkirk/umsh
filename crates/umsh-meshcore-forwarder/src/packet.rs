//! The MeshCore v1 frame: parsing, the duplicate identity, and building
//! the frames a forwarder emits.
//!
//! A frame is a header octet, two transport codes when the route type
//! carries them, a path-length octet, the path, and the payload:
//!
//! ~~~text
//! header | [code0 code1] | path_len | path[count × size] | payload
//! ~~~
//!
//! The header packs the route type (bits 0–1), the payload type (bits
//! 2–5), and the version (bits 6–7). The path-length octet packs the
//! per-entry hash size less one (bits 6–7; 3 is reserved) and the entry
//! count (bits 0–5).
//!
//! Parsing follows the baseline dispatcher's acceptance rules exactly,
//! except that every read is bounds-checked: a frame too short for the
//! fields its header promises is rejected rather than read past its end.

use heapless::Vec;
use umsh_crypto::Sha256Provider;
use umsh_crypto::software::SoftwareSha256;

/// The largest radio frame.
pub const MAX_FRAME_LEN: usize = 255;
/// The largest payload the baseline accepts.
pub const MAX_PAYLOAD_LEN: usize = 184;
/// The largest encoded path, in octets.
pub const MAX_PATH_LEN: usize = 64;
/// Octets of a packet's duplicate identity.
pub const PACKET_HASH_LEN: usize = 8;

/// A packet's duplicate identity: the leading octets of a SHA-256 over
/// its payload type and payload.
pub type PacketHash = [u8; PACKET_HASH_LEN];

/// A frame buffer sized for the radio.
pub type Frame = Vec<u8, MAX_FRAME_LEN>;

/// Payload types, from the header's bits 2–5.
pub mod payload_type {
    pub const REQ: u8 = 0x00;
    pub const RESPONSE: u8 = 0x01;
    pub const TXT_MSG: u8 = 0x02;
    pub const ACK: u8 = 0x03;
    pub const ADVERT: u8 = 0x04;
    pub const GRP_TXT: u8 = 0x05;
    pub const GRP_DATA: u8 = 0x06;
    pub const ANON_REQ: u8 = 0x07;
    pub const PATH: u8 = 0x08;
    pub const TRACE: u8 = 0x09;
    pub const MULTIPART: u8 = 0x0A;
    pub const CONTROL: u8 = 0x0B;
    pub const RAW_CUSTOM: u8 = 0x0F;
}

/// The one protocol version this engine speaks, from the header's bits
/// 6–7. UMSH frames carry `0b11` there, so the two never collide.
pub const VERSION_1: u8 = 0;

/// How a packet is routed, from the header's bits 0–1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteType {
    /// Flooded, scoped by transport codes.
    TransportFlood = 0,
    /// Flooded, unscoped.
    Flood = 1,
    /// Source-routed along the path.
    Direct = 2,
    /// Source-routed, carrying transport codes.
    TransportDirect = 3,
}

impl RouteType {
    /// The route type a header octet carries, whatever follows it.
    pub const fn from_header(header: u8) -> Self {
        match header & 0x03 {
            0 => Self::TransportFlood,
            1 => Self::Flood,
            2 => Self::Direct,
            _ => Self::TransportDirect,
        }
    }

    /// Whether the packet is flooded rather than source-routed.
    pub const fn is_flood(self) -> bool {
        matches!(self, Self::TransportFlood | Self::Flood)
    }

    /// Whether two transport codes follow the header.
    pub const fn has_transport_codes(self) -> bool {
        matches!(self, Self::TransportFlood | Self::TransportDirect)
    }
}

/// Why a frame is not a MeshCore v1 packet this engine accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseError {
    /// Nothing was received.
    Empty,
    /// Another protocol, or a MeshCore version this engine does not
    /// speak. Every UMSH frame lands here.
    Version,
    /// The frame ends inside its header, codes, path length, or path.
    Truncated,
    /// The path-length octet selects the reserved hash size.
    ReservedPathMode,
    /// The encoded path exceeds [`MAX_PATH_LEN`].
    PathTooLong,
    /// The payload exceeds [`MAX_PAYLOAD_LEN`].
    PayloadTooLong,
}

/// A parsed MeshCore v1 packet, borrowing the frame it came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Packet<'a> {
    header: u8,
    /// Both codes in wire order, or `None` for a route type without them.
    transport_codes: Option<[u8; 4]>,
    path_len: u8,
    path: &'a [u8],
    payload: &'a [u8],
}

impl<'a> Packet<'a> {
    /// Parse one received frame.
    pub fn parse(frame: &'a [u8]) -> Result<Self, ParseError> {
        let (&header, rest) = frame.split_first().ok_or(ParseError::Empty)?;
        if header >> 6 != VERSION_1 {
            return Err(ParseError::Version);
        }
        let route = RouteType::from_header(header);
        let (transport_codes, rest) = if route.has_transport_codes() {
            let (codes, rest) = rest.split_at_checked(4).ok_or(ParseError::Truncated)?;
            (Some(codes.try_into().expect("four octets")), rest)
        } else {
            (None, rest)
        };
        let (&path_len, rest) = rest.split_first().ok_or(ParseError::Truncated)?;
        if path_len >> 6 == 3 {
            return Err(ParseError::ReservedPathMode);
        }
        let path_bytes = path_byte_len(path_len);
        if path_bytes > MAX_PATH_LEN {
            return Err(ParseError::PathTooLong);
        }
        let (path, payload) = rest
            .split_at_checked(path_bytes)
            .ok_or(ParseError::Truncated)?;
        if payload.len() > MAX_PAYLOAD_LEN {
            return Err(ParseError::PayloadTooLong);
        }
        Ok(Self {
            header,
            transport_codes,
            path_len,
            path,
            payload,
        })
    }

    /// The header octet.
    pub const fn header(&self) -> u8 {
        self.header
    }

    pub const fn route_type(&self) -> RouteType {
        RouteType::from_header(self.header)
    }

    /// The payload type, one of [`payload_type`]'s values or another.
    pub const fn payload_type(&self) -> u8 {
        (self.header >> 2) & 0x0F
    }

    /// The encoded path-length octet.
    pub const fn path_len(&self) -> u8 {
        self.path_len
    }

    /// Octets per path entry: one, two, or three.
    pub const fn path_hash_size(&self) -> usize {
        (self.path_len >> 6) as usize + 1
    }

    /// Entries in the path.
    pub const fn path_hash_count(&self) -> usize {
        (self.path_len & 0x3F) as usize
    }

    pub const fn path(&self) -> &'a [u8] {
        self.path
    }

    pub const fn payload(&self) -> &'a [u8] {
        self.payload
    }

    /// Both transport codes in wire order, when the route type has them.
    pub const fn transport_codes(&self) -> Option<[u8; 4]> {
        self.transport_codes
    }

    /// The first transport code in wire order: the one a repeater
    /// matches against its regions.
    pub const fn first_transport_code(&self) -> Option<[u8; 2]> {
        match self.transport_codes {
            Some([a, b, _, _]) => Some([a, b]),
            None => None,
        }
    }

    /// This packet's duplicate identity.
    pub fn hash(&self) -> PacketHash {
        packet_hash(self.payload_type(), self.path_len, self.payload)
    }
}

/// Octets a path-length octet claims, which may exceed what is allowed.
const fn path_byte_len(path_len: u8) -> usize {
    (path_len & 0x3F) as usize * ((path_len >> 6) as usize + 1)
}

/// The duplicate identity of a packet.
///
/// SHA-256 over the payload type and the payload, truncated. A TRACE also
/// hashes its path length—because a trace legitimately revisits a node
/// on its way back—and does so as the baseline does: both in-memory
/// octets of a little-endian 16-bit field, so the encoded path-length
/// octet followed by a zero.
pub fn packet_hash(payload_type: u8, path_len: u8, payload: &[u8]) -> PacketHash {
    let digest = if payload_type == payload_type::TRACE {
        SoftwareSha256.hash(&[&[payload_type], &[path_len, 0], payload])
    } else {
        SoftwareSha256.hash(&[&[payload_type], payload])
    };
    let mut hash = [0; PACKET_HASH_LEN];
    hash.copy_from_slice(&digest[..PACKET_HASH_LEN]);
    hash
}

/// The header octet for a payload type and route type, at version 1.
pub const fn header(payload_type: u8, route: RouteType) -> u8 {
    (VERSION_1 << 6) | ((payload_type & 0x0F) << 2) | route as u8
}

/// Encode a frame. `None` when it would not fit a radio frame, which no
/// frame built from a valid reception can reach.
pub fn encode(
    header: u8,
    transport_codes: Option<[u8; 4]>,
    path_len: u8,
    path: &[u8],
    payload: &[u8],
) -> Option<Frame> {
    let mut frame = Frame::new();
    frame.push(header).ok()?;
    if let Some(codes) = transport_codes {
        frame.extend_from_slice(&codes).ok()?;
    }
    frame.push(path_len).ok()?;
    frame.extend_from_slice(path).ok()?;
    frame.extend_from_slice(payload).ok()?;
    Some(frame)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_every_other_version() {
        for version in 1..4u8 {
            let frame = [
                (version << 6) | header(payload_type::TXT_MSG, RouteType::Flood),
                0,
                1,
            ];
            assert_eq!(Packet::parse(&frame), Err(ParseError::Version));
        }
    }

    #[test]
    fn rejects_the_reserved_path_mode() {
        let frame = [header(payload_type::TXT_MSG, RouteType::Flood), 0xC0, 1];
        assert_eq!(Packet::parse(&frame), Err(ParseError::ReservedPathMode));
    }

    #[test]
    fn the_path_limit_counts_octets_not_entries() {
        // 22 three-octet entries is 66 octets.
        let mut frame = std::vec![header(payload_type::TXT_MSG, RouteType::Flood), 0x80 | 22];
        frame.resize(frame.len() + 66 + 5, 0);
        assert_eq!(Packet::parse(&frame), Err(ParseError::PathTooLong));
        // 21 of them is 63, inside the limit.
        let mut frame = std::vec![header(payload_type::TXT_MSG, RouteType::Flood), 0x80 | 21];
        frame.resize(frame.len() + 63 + 5, 0);
        let packet = Packet::parse(&frame).unwrap();
        assert_eq!(packet.path_hash_size(), 3);
        assert_eq!(packet.path_hash_count(), 21);
        assert_eq!(packet.path().len(), 63);
    }

    #[test]
    fn truncation_is_rejected_at_every_field() {
        let route = header(payload_type::TXT_MSG, RouteType::TransportFlood);
        assert_eq!(Packet::parse(&[]), Err(ParseError::Empty));
        assert_eq!(Packet::parse(&[route, 1, 2, 3]), Err(ParseError::Truncated));
        assert_eq!(
            Packet::parse(&[route, 1, 2, 3, 4]),
            Err(ParseError::Truncated)
        );
        assert_eq!(
            Packet::parse(&[route, 1, 2, 3, 4, 2, 0xAA]),
            Err(ParseError::Truncated)
        );
        // A path that exactly fills the frame leaves an empty payload,
        // which the dispatcher accepts and each payload type then judges.
        let frame = [route, 1, 2, 3, 4, 2, 0xAA, 0xBB];
        let packet = Packet::parse(&frame).unwrap();
        assert!(packet.payload().is_empty());
        assert_eq!(packet.first_transport_code(), Some([1, 2]));
    }

    #[test]
    fn payload_limit() {
        let mut frame = std::vec![header(payload_type::GRP_TXT, RouteType::Flood), 0];
        frame.resize(2 + MAX_PAYLOAD_LEN, 0x55);
        assert!(Packet::parse(&frame).is_ok());
        frame.push(0x55);
        assert_eq!(Packet::parse(&frame), Err(ParseError::PayloadTooLong));
    }

    #[test]
    fn route_type_does_not_change_the_identity() {
        let flood = [
            header(payload_type::TXT_MSG, RouteType::Flood),
            0x01,
            0xAA,
            1,
            2,
            3,
        ];
        let direct = [
            header(payload_type::TXT_MSG, RouteType::Direct),
            0x00,
            1,
            2,
            3,
        ];
        assert_eq!(
            Packet::parse(&flood).unwrap().hash(),
            Packet::parse(&direct).unwrap().hash()
        );
    }

    #[test]
    fn a_trace_identity_includes_its_path_length() {
        let a = [
            header(payload_type::TRACE, RouteType::Direct),
            0x00,
            1,
            2,
            3,
        ];
        let b = [
            header(payload_type::TRACE, RouteType::Direct),
            0x01,
            9,
            1,
            2,
            3,
        ];
        assert_ne!(
            Packet::parse(&a).unwrap().hash(),
            Packet::parse(&b).unwrap().hash()
        );
    }

    #[test]
    fn encode_round_trips() {
        let frame = encode(
            header(payload_type::ACK, RouteType::TransportDirect),
            Some([1, 2, 3, 4]),
            0x42,
            &[0xAA, 0xBB, 0xCC, 0xDD],
            &[9, 8, 7, 6],
        )
        .unwrap();
        let packet = Packet::parse(&frame).unwrap();
        assert_eq!(packet.route_type(), RouteType::TransportDirect);
        assert_eq!(packet.payload_type(), payload_type::ACK);
        assert_eq!(packet.path_hash_size(), 2);
        assert_eq!(packet.path_hash_count(), 2);
        assert_eq!(packet.payload(), &[9, 8, 7, 6]);
    }
}
