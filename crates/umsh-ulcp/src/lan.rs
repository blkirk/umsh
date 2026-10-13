//! LAN bearer configuration and state (device domain).
//!
//! Spec: `docs/protocol/src/ulcp-lan-bearer.md`; the bearer itself is
//! `docs/protocol/src/lan-bearer.md`.

/// The default multicast group.
pub const DEFAULT_GROUP: [u8; 4] = [239, 255, 134, 121];
/// The default UDP port.
pub const DEFAULT_PORT: u16 = 15056;
/// The default ingress limit, in datagrams per minute.
pub const DEFAULT_INGRESS_LIMIT: u16 = 120;
/// Length of a `PROP_LAN_BEARER_GROUP` value.
pub const GROUP_LEN: usize = 6;

/// `PROP_LAN_BEARER_STATE`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum State {
    /// The bearer is switched off.
    #[default]
    Disabled = 0,
    /// The bearer is joined to its group and carrying frames.
    Active = 1,
    /// The bearer is switched on and the device has no IPv4 network to
    /// join the group on.
    NoNetwork = 2,
}

impl State {
    pub const fn decode(value: u8) -> Option<Self> {
        Some(match value {
            0 => Self::Disabled,
            1 => Self::Active,
            2 => Self::NoNetwork,
            _ => return None,
        })
    }
}

/// A multicast group and port, as `PROP_LAN_BEARER_GROUP` carries them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Group {
    pub address: [u8; 4],
    pub port: u16,
}

impl Default for Group {
    fn default() -> Self {
        Self {
            address: DEFAULT_GROUP,
            port: DEFAULT_PORT,
        }
    }
}

impl Group {
    pub fn encode(self) -> [u8; GROUP_LEN] {
        let [a, b, c, d] = self.address;
        let [lo, hi] = self.port.to_le_bytes();
        [a, b, c, d, lo, hi]
    }

    /// Decode a group, refusing anything but an IPv4 multicast address
    /// (224.0.0.0/4) and a nonzero port.
    pub fn decode(value: &[u8]) -> Option<Self> {
        let &[a, b, c, d, lo, hi] = value else {
            return None;
        };
        let port = u16::from_le_bytes([lo, hi]);
        (a & 0xF0 == 0xE0 && port != 0).then_some(Self {
            address: [a, b, c, d],
            port,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_group_round_trips_and_only_a_multicast_one() {
        let group = Group::default();
        assert_eq!(group.encode(), [239, 255, 134, 121, 0xD0, 0x3A]);
        assert_eq!(Group::decode(&group.encode()), Some(group));
        assert_eq!(Group::decode(&[192, 168, 1, 1, 0xD0, 0x3A]), None);
        assert_eq!(Group::decode(&[239, 255, 134, 121, 0, 0]), None);
        assert_eq!(Group::decode(&[239, 255, 134, 121, 1]), None);
    }
}
