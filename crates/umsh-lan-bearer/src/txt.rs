//! The DNS-SD TXT schema (`lan-bearer.md` § Discovery).
//!
//! A participant's record says which group it is on, which node it is,
//! and—if it has a LoRa radio—the settings that radio uses, so a client
//! knows before sending anything which meshes the LAN reaches.
//!
//! Allocation-free: a radio's responder formats each value into a stack
//! buffer as it goes, and a browser parses with whatever lookup its mDNS
//! library offers.

use core::fmt::Write as _;

use heapless::String;
use umsh_core::base58;

/// The schema version this module writes, and the only one it reads.
pub const SCHEMA_VERSION: &str = "1";

/// TXT keys.
pub mod key {
    /// Schema version.
    pub const V: &str = "v";
    /// Multicast group address, dotted quad.
    pub const GRP: &str = "grp";
    /// Node public key, base58.
    pub const ID: &str = "id";
    /// LoRa center frequency, Hz.
    pub const F: &str = "f";
    /// LoRa bandwidth, Hz.
    pub const BW: &str = "bw";
    /// LoRa spreading factor.
    pub const SF: &str = "sf";
    /// LoRa coding rate denominator.
    pub const CR: &str = "cr";
    /// `1` if LoRa repeating is enabled.
    pub const REP: &str = "rep";
    /// `1` if MeshCore v1 forwarding is enabled.
    pub const MC: &str = "mc";
    /// `1` if an internet bridge link is up.
    pub const IB: &str = "ib";
}

/// The LoRa half of an advertisement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoraSettings {
    pub freq_hz: u32,
    pub bw_hz: u32,
    pub sf: u8,
    /// Coding rate denominator, 5 to 8.
    pub cr: u8,
    /// Whether the node repeats LoRa to LoRa.
    pub repeater: bool,
}

/// Everything one participant's TXT record says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Advertisement {
    pub group: [u8; 4],
    pub node_key: [u8; 32],
    /// Present for a participant with a LoRa radio.
    pub lora: Option<LoraSettings>,
    pub meshcore: bool,
    pub internet_bridge: bool,
}

/// Why a TXT record could not be read as an advertisement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TxtError {
    /// A required key is absent.
    Missing(&'static str),
    /// A key's value does not parse.
    Invalid(&'static str),
    /// A schema version this module does not read.
    Version,
}

/// The longest value written: a 44-digit key.
type Value = String<{ base58::ENCODED_LEN }>;

impl Advertisement {
    /// Hand each TXT key and value to `f`, in schema order. Optional
    /// flags are written only when set.
    pub fn for_each_property(&self, mut f: impl FnMut(&'static str, &str)) {
        let mut value = Value::new();
        let mut put = |key: &'static str, args: core::fmt::Arguments<'_>| {
            value.clear();
            // Every value fits by construction: the longest is the key.
            let _ = value.write_fmt(args);
            f(key, &value);
        };
        put(key::V, format_args!("{SCHEMA_VERSION}"));
        let [a, b, c, d] = self.group;
        put(key::GRP, format_args!("{a}.{b}.{c}.{d}"));
        let id = base58::encode(&self.node_key);
        put(
            key::ID,
            format_args!("{}", core::str::from_utf8(&id).unwrap_or_default()),
        );
        if let Some(lora) = self.lora {
            put(key::F, format_args!("{}", lora.freq_hz));
            put(key::BW, format_args!("{}", lora.bw_hz));
            put(key::SF, format_args!("{}", lora.sf));
            put(key::CR, format_args!("{}", lora.cr));
            put(key::REP, format_args!("{}", lora.repeater as u8));
        }
        if self.meshcore {
            put(key::MC, format_args!("1"));
        }
        if self.internet_bridge {
            put(key::IB, format_args!("1"));
        }
    }

    /// Read an advertisement through `get`, which returns a key's value.
    /// Unknown keys are the caller's to ignore; a missing optional flag
    /// reads as false.
    pub fn parse<'a>(get: impl Fn(&str) -> Option<&'a str>) -> Result<Self, TxtError> {
        let required = |key: &'static str| get(key).ok_or(TxtError::Missing(key));
        if required(key::V)? != SCHEMA_VERSION {
            return Err(TxtError::Version);
        }
        let group = parse_dotted_quad(required(key::GRP)?).ok_or(TxtError::Invalid(key::GRP))?;
        let node_key = base58::decode(required(key::ID)?.as_bytes())
            .map_err(|_| TxtError::Invalid(key::ID))?;
        let flag = |key: &'static str| -> Result<bool, TxtError> {
            match get(key) {
                None | Some("0") => Ok(false),
                Some("1") => Ok(true),
                Some(_) => Err(TxtError::Invalid(key)),
            }
        };
        let lora = match get(key::F) {
            None => None,
            Some(freq) => {
                let number = |key: &'static str, value: &str| {
                    value.parse::<u32>().map_err(|_| TxtError::Invalid(key))
                };
                let freq_hz = number(key::F, freq)?;
                let bw_hz = number(key::BW, required(key::BW)?)?;
                let sf = u8::try_from(number(key::SF, required(key::SF)?)?)
                    .map_err(|_| TxtError::Invalid(key::SF))?;
                let cr = u8::try_from(number(key::CR, required(key::CR)?)?)
                    .ok()
                    .filter(|cr| (5..=8).contains(cr))
                    .ok_or(TxtError::Invalid(key::CR))?;
                Some(LoraSettings {
                    freq_hz,
                    bw_hz,
                    sf,
                    cr,
                    repeater: flag(key::REP)?,
                })
            }
        };
        Ok(Self {
            group,
            node_key,
            lora,
            meshcore: flag(key::MC)?,
            internet_bridge: flag(key::IB)?,
        })
    }
}

fn parse_dotted_quad(text: &str) -> Option<[u8; 4]> {
    let mut octets = [0u8; 4];
    let mut parts = text.split('.');
    for octet in octets.iter_mut() {
        let part = parts.next()?;
        // No leading zeros, signs, or empty parts: one spelling per address.
        if part.is_empty() || (part.len() > 1 && part.starts_with('0')) {
            return None;
        }
        *octet = part.parse().ok()?;
    }
    parts.next().is_none().then_some(octets)
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::collections::BTreeMap;
    use std::string::{String, ToString};

    fn record(advertisement: &Advertisement) -> BTreeMap<&'static str, String> {
        let mut map = BTreeMap::new();
        advertisement.for_each_property(|key, value| {
            map.insert(key, value.to_string());
        });
        map
    }

    fn radio() -> Advertisement {
        Advertisement {
            group: [239, 255, 134, 121],
            node_key: [7; 32],
            lora: Some(LoraSettings {
                freq_hz: 906_875_000,
                bw_hz: 250_000,
                sf: 11,
                cr: 5,
                repeater: true,
            }),
            meshcore: false,
            internet_bridge: true,
        }
    }

    #[test]
    fn a_radio_round_trips() {
        let advertisement = radio();
        let map = record(&advertisement);
        assert_eq!(map["v"], "1");
        assert_eq!(map["grp"], "239.255.134.121");
        assert_eq!(map["id"].len(), 44);
        assert_eq!(map["f"], "906875000");
        assert_eq!(map["rep"], "1");
        assert_eq!(map.get("mc"), None, "unset flags are not written");
        assert_eq!(map["ib"], "1");
        let parsed = Advertisement::parse(|key| map.get(key).map(String::as_str)).unwrap();
        assert_eq!(parsed, advertisement);
    }

    #[test]
    fn a_participant_without_a_radio_has_no_lora_keys() {
        let advertisement = Advertisement {
            lora: None,
            internet_bridge: false,
            ..radio()
        };
        let map = record(&advertisement);
        assert_eq!(
            map.keys().copied().collect::<std::vec::Vec<_>>(),
            ["grp", "id", "v"]
        );
        let parsed = Advertisement::parse(|key| map.get(key).map(String::as_str)).unwrap();
        assert_eq!(parsed, advertisement);
    }

    #[test]
    fn bad_records_are_refused_by_key() {
        let mut map = record(&radio());
        map.insert("cr", "9".to_string());
        assert_eq!(
            Advertisement::parse(|key| map.get(key).map(String::as_str)),
            Err(TxtError::Invalid("cr"))
        );
        map.insert("v", "2".to_string());
        assert_eq!(
            Advertisement::parse(|key| map.get(key).map(String::as_str)),
            Err(TxtError::Version)
        );
        map.remove("v");
        assert_eq!(
            Advertisement::parse(|key| map.get(key).map(String::as_str)),
            Err(TxtError::Missing("v"))
        );
    }

    #[test]
    fn a_group_has_one_spelling() {
        assert_eq!(
            parse_dotted_quad("239.255.134.121"),
            Some([239, 255, 134, 121])
        );
        assert_eq!(parse_dotted_quad("239.255.042.42"), None);
        assert_eq!(parse_dotted_quad("239.255.42"), None);
        assert_eq!(parse_dotted_quad("239.255.134.121.1"), None);
        assert_eq!(parse_dotted_quad("239.255.42.256"), None);
    }
}
