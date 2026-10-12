//! Saved device-domain configuration for the LAN bearer.
use umsh_ulcp::{
    Status,
    ids::prop,
    lan::{DEFAULT_INGRESS_LIMIT, Group},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LanBearerConfig {
    pub enabled: bool,
    pub group: Group,
    /// Datagrams per minute carried from the LAN onto the radio.
    pub ingress_limit: u16,
}

impl Default for LanBearerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            group: Group::default(),
            ingress_limit: DEFAULT_INGRESS_LIMIT,
        }
    }
}

impl LanBearerConfig {
    /// Whether `key` is one of the saved configuration properties.
    pub const fn is_config_prop(key: u32) -> bool {
        matches!(
            key,
            prop::LAN_BEARER_ENABLED | prop::LAN_BEARER_GROUP | prop::LAN_BEARER_INGRESS_LIMIT
        )
    }

    pub fn set(&mut self, key: u32, value: &[u8]) -> Result<(), Status> {
        match key {
            prop::LAN_BEARER_ENABLED => {
                self.enabled = match value {
                    [0] => false,
                    [1] => true,
                    _ => return Err(Status::INVALID_ARGUMENT),
                }
            }
            prop::LAN_BEARER_GROUP => {
                self.group = Group::decode(value).ok_or(Status::INVALID_ARGUMENT)?;
            }
            prop::LAN_BEARER_INGRESS_LIMIT => {
                let limit =
                    u16::from_le_bytes(value.try_into().map_err(|_| Status::INVALID_ARGUMENT)?);
                if limit == 0 {
                    return Err(Status::INVALID_ARGUMENT);
                }
                self.ingress_limit = limit;
            }
            _ => return Err(Status::PROP_NOT_FOUND),
        }
        Ok(())
    }

    /// Encode one configuration property; at most
    /// [`GROUP_LEN`](umsh_ulcp::lan::GROUP_LEN) octets.
    pub fn encode(&self, key: u32, out: &mut [u8]) -> Option<usize> {
        let group;
        let limit;
        let bytes: &[u8] = match key {
            prop::LAN_BEARER_ENABLED => &[self.enabled as u8],
            prop::LAN_BEARER_GROUP => {
                group = self.group.encode();
                &group
            }
            prop::LAN_BEARER_INGRESS_LIMIT => {
                limit = self.ingress_limit.to_le_bytes();
                &limit
            }
            _ => return None,
        };
        out.get_mut(..bytes.len())?.copy_from_slice(bytes);
        Some(bytes.len())
    }
}
