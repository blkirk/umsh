//! Region scoping: transport keys derived from region names, the codes
//! they give a packet, and how the forwarder treats a flood by scope.

use umsh_crypto::Sha256Provider;
use umsh_crypto::software::SoftwareSha256;

/// The secret a region's transport codes are computed under.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TransportKey([u8; 16]);

impl core::fmt::Debug for TransportKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // A public region's key is derivable from its name, but nothing
        // gains from printing it.
        f.write_str("TransportKey(..)")
    }
}

impl TransportKey {
    /// The key for a public region, from its name exactly as stored.
    ///
    /// The name is hashed byte for byte, so spelling and case matter. A
    /// leading `#` is supplied when the name lacks one, which is how the
    /// baseline treats a bare region name. A name beginning with `$` is
    /// a private region, whose key lives in a keystore rather than in
    /// its name; there is none here, so it gives no key at all.
    pub fn for_region_name(name: &[u8]) -> Option<Self> {
        match name.first() {
            Some(b'$') => None,
            Some(b'#') => Some(Self::from_digest(SoftwareSha256.hash(&[name]))),
            _ => Some(Self::from_digest(SoftwareSha256.hash(&[b"#", name]))),
        }
    }

    /// A key given directly, for tests and vectors.
    pub const fn from_bytes(key: [u8; 16]) -> Self {
        Self(key)
    }

    fn from_digest(digest: [u8; 32]) -> Self {
        let mut key = [0; 16];
        key.copy_from_slice(&digest[..16]);
        Self(key)
    }

    /// The transport code this key gives a packet, in wire order.
    ///
    /// HMAC-SHA256 over the payload type and payload, truncated to two
    /// octets. The baseline reads those octets as a little-endian 16-bit
    /// value and reserves 0x0000 and 0xFFFF, moving them one step inward;
    /// the arithmetic here is the same, done on the wire octets.
    pub fn code(&self, payload_type: u8, payload: &[u8]) -> [u8; 2] {
        let mac = SoftwareSha256.hmac(&self.0, &[&[payload_type], payload]);
        let code = match u16::from_le_bytes([mac[0], mac[1]]) {
            0x0000 => 0x0001,
            0xFFFF => 0xFFFE,
            code => code,
        };
        code.to_le_bytes()
    }
}

/// How transport codes gate flood forwarding
/// (`PROP_MESHCORE_REGION_POLICY`).
///
/// Direct traffic is never gated: it follows its route whatever codes it
/// carries.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum RegionPolicy {
    /// Unscoped floods are forwarded, and a scoped flood only when its
    /// code matches a configured region. The baseline repeater's default.
    #[default]
    Open = 0,
    /// Only scoped floods whose code matches a configured region.
    ScopedOnly = 1,
    /// Every flood, whatever its scope.
    Ignore = 2,
}

impl RegionPolicy {
    /// The policy a wire value names, or `None` for an unassigned one.
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Open),
            1 => Some(Self::ScopedOnly),
            2 => Some(Self::Ignore),
            _ => None,
        }
    }

    /// Whether a flood may be forwarded. `matched` is consulted only for
    /// a scoped flood: whether its first code belongs to a configured
    /// region.
    pub fn admits(
        self,
        first_code: Option<[u8; 2]>,
        matched: impl FnOnce([u8; 2]) -> bool,
    ) -> bool {
        match (self, first_code) {
            (Self::Ignore, _) => true,
            (Self::Open, None) => true,
            (Self::ScopedOnly, None) => false,
            (_, Some(code)) => matched(code),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_name_derives_as_its_hashtag() {
        assert_eq!(
            TransportKey::for_region_name(b"sf-bay"),
            TransportKey::for_region_name(b"#sf-bay")
        );
        // Case is significant, unlike UMSH's own derivation.
        assert_ne!(
            TransportKey::for_region_name(b"sf-bay"),
            TransportKey::for_region_name(b"SF-Bay")
        );
        // A UMSH code literal is just another MeshCore name.
        assert_eq!(
            TransportKey::for_region_name(b"0x1234"),
            TransportKey::for_region_name(b"#0x1234")
        );
    }

    #[test]
    fn a_private_region_has_no_key() {
        assert_eq!(TransportKey::for_region_name(b"$ops"), None);
        assert_ne!(TransportKey::for_region_name(b"#$ops"), None);
    }

    #[test]
    fn the_policy_table() {
        let all = |_| true;
        let none = |_| false;
        let code = Some([0x12, 0x34]);
        assert!(RegionPolicy::Open.admits(None, none));
        assert!(RegionPolicy::Open.admits(code, all));
        assert!(!RegionPolicy::Open.admits(code, none));
        assert!(!RegionPolicy::ScopedOnly.admits(None, all));
        assert!(RegionPolicy::ScopedOnly.admits(code, all));
        assert!(!RegionPolicy::ScopedOnly.admits(code, none));
        assert!(RegionPolicy::Ignore.admits(None, none));
        assert!(RegionPolicy::Ignore.admits(code, all));
        assert!(RegionPolicy::Ignore.admits(code, none));
    }

    #[test]
    fn policy_values() {
        for value in 0..=255u8 {
            match RegionPolicy::from_u8(value) {
                Some(policy) => assert_eq!(policy as u8, value),
                None => assert!(value > 2),
            }
        }
    }
}
