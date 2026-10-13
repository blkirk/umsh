//! Saved device-domain configuration for the LAN bearer.
use umsh_ulcp::{
    Status,
    ids::prop,
    lan::{DEFAULT_INGRESS_LIMIT, Group},
};

/// The bearer's settings. The group and the ingress limit are held as
/// what an operator chose, if anything: an unchosen one follows this
/// firmware's default, and is saved as unchosen so that a later firmware
/// with a different default brings it along.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LanBearerConfig {
    pub enabled: bool,
    group: Option<Group>,
    ingress_limit: Option<u16>,
}

impl LanBearerConfig {
    /// The group in effect.
    pub fn group(&self) -> Group {
        self.group.unwrap_or_default()
    }

    /// Datagrams per minute carried from the LAN onto the radio.
    pub fn ingress_limit(&self) -> u16 {
        self.ingress_limit.unwrap_or(DEFAULT_INGRESS_LIMIT)
    }

    /// Whether `key` is one of the saved configuration properties.
    pub const fn is_config_prop(key: u32) -> bool {
        matches!(
            key,
            prop::LAN_BEARER_ENABLED | prop::LAN_BEARER_GROUP | prop::LAN_BEARER_INGRESS_LIMIT
        )
    }

    /// Whether `key` holds no choice of its own and follows the default.
    pub fn is_default(&self, key: u32) -> bool {
        match key {
            prop::LAN_BEARER_GROUP => self.group.is_none(),
            prop::LAN_BEARER_INGRESS_LIMIT => self.ingress_limit.is_none(),
            _ => false,
        }
    }

    /// Take one property from `saved`, choice and all.
    pub fn restore(&mut self, key: u32, saved: &Self) {
        match key {
            prop::LAN_BEARER_ENABLED => self.enabled = saved.enabled,
            prop::LAN_BEARER_GROUP => self.group = saved.group,
            prop::LAN_BEARER_INGRESS_LIMIT => self.ingress_limit = saved.ingress_limit,
            _ => {}
        }
    }

    /// Apply a written value. An empty group or limit returns it to the
    /// default.
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
                self.group = match value {
                    [] => None,
                    _ => Some(Group::decode(value).ok_or(Status::INVALID_ARGUMENT)?),
                }
            }
            prop::LAN_BEARER_INGRESS_LIMIT => {
                self.ingress_limit = match value {
                    [] => None,
                    _ => {
                        let limit = u16::from_le_bytes(
                            value.try_into().map_err(|_| Status::INVALID_ARGUMENT)?,
                        );
                        if limit == 0 {
                            return Err(Status::INVALID_ARGUMENT);
                        }
                        Some(limit)
                    }
                }
            }
            _ => return Err(Status::PROP_NOT_FOUND),
        }
        Ok(())
    }

    /// Encode the value in effect for one configuration property; at most
    /// [`GROUP_LEN`](umsh_ulcp::lan::GROUP_LEN) octets.
    pub fn encode(&self, key: u32, out: &mut [u8]) -> Option<usize> {
        let group;
        let limit;
        let bytes: &[u8] = match key {
            prop::LAN_BEARER_ENABLED => &[self.enabled as u8],
            prop::LAN_BEARER_GROUP => {
                group = self.group().encode();
                &group
            }
            prop::LAN_BEARER_INGRESS_LIMIT => {
                limit = self.ingress_limit().to_le_bytes();
                &limit
            }
            _ => return None,
        };
        out.get_mut(..bytes.len())?.copy_from_slice(bytes);
        Some(bytes.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_write_returns_a_setting_to_the_default() {
        let mut config = LanBearerConfig::default();
        assert!(config.is_default(prop::LAN_BEARER_GROUP));
        config
            .set(prop::LAN_BEARER_GROUP, &[239, 1, 2, 3, 0x10, 0x27])
            .unwrap();
        config
            .set(prop::LAN_BEARER_INGRESS_LIMIT, &30u16.to_le_bytes())
            .unwrap();
        assert_eq!(config.group().address, [239, 1, 2, 3]);
        assert_eq!(config.ingress_limit(), 30);
        assert!(!config.is_default(prop::LAN_BEARER_GROUP));

        config.set(prop::LAN_BEARER_GROUP, &[]).unwrap();
        config.set(prop::LAN_BEARER_INGRESS_LIMIT, &[]).unwrap();
        assert_eq!(config, LanBearerConfig::default());
        assert_eq!(config.group(), Group::default());
        assert_eq!(config.ingress_limit(), DEFAULT_INGRESS_LIMIT);
        assert_eq!(
            config.set(prop::LAN_BEARER_ENABLED, &[]),
            Err(Status::INVALID_ARGUMENT)
        );
    }
}
