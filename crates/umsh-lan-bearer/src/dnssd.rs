//! DNS-SD advertising and browsing for hosts (`lan-bearer.md` §
//! Discovery).
//!
//! Discovery is only discovery: nothing here connects to anything. A
//! browser learns who is on the group and which LoRa settings their radios
//! use; frames still travel only over the group.

use std::collections::HashMap;
use std::net::IpAddr;

use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo, TxtProperty};

use crate::SERVICE_TYPE;
use crate::txt::{Advertisement, TxtError};

pub use mdns_sd::Error;

/// The fully qualified service type mDNS speaks in.
fn service_domain() -> String {
    format!("{SERVICE_TYPE}.local.")
}

/// One participant's record.
#[derive(Clone, Debug)]
pub struct Participant {
    /// The DNS-SD instance name: the device name.
    pub instance: String,
    pub addresses: Vec<IpAddr>,
    /// The SRV port, which is the group's port.
    pub port: u16,
    /// What its TXT record says, or why that could not be read.
    pub advertisement: Result<Advertisement, TxtError>,
}

/// A change in who is on the group.
#[derive(Clone, Debug)]
pub enum DiscoveryEvent {
    /// A participant appeared, or its record changed.
    Updated(Participant),
    /// A participant's record was withdrawn.
    Removed { instance: String },
}

/// The mDNS responder and browser for this process.
pub struct Discovery {
    daemon: ServiceDaemon,
}

impl Discovery {
    pub fn new() -> Result<Self, Error> {
        Ok(Self {
            daemon: ServiceDaemon::new()?,
        })
    }

    /// Advertise this participant as `name`, on the group's `port`.
    /// Advertising again under the same name replaces the record.
    pub fn advertise(
        &self,
        name: &str,
        port: u16,
        advertisement: &Advertisement,
    ) -> Result<Registration<'_>, Error> {
        let mut properties = Vec::new();
        advertisement.for_each_property(|key, value| {
            properties.push(TxtProperty::from((key, value)));
        });
        let host = format!("{}.local.", host_label(name));
        let info = ServiceInfo::new(&service_domain(), name, &host, (), port, properties)?
            .enable_addr_auto();
        let fullname = info.get_fullname().to_string();
        self.daemon.register(info)?;
        Ok(Registration {
            daemon: &self.daemon,
            fullname,
        })
    }

    /// Start browsing for participants.
    pub fn browse(&self) -> Result<Browser, Error> {
        Ok(Browser {
            events: self.daemon.browse(&service_domain())?,
        })
    }
}

impl Drop for Discovery {
    fn drop(&mut self) {
        let _ = self.daemon.shutdown();
    }
}

/// A record this process advertises. Withdrawn when dropped.
pub struct Registration<'a> {
    daemon: &'a ServiceDaemon,
    fullname: String,
}

impl Drop for Registration<'_> {
    fn drop(&mut self) {
        let _ = self.daemon.unregister(&self.fullname);
    }
}

/// A stream of [`DiscoveryEvent`]s.
pub struct Browser {
    events: mdns_sd::Receiver<ServiceEvent>,
}

impl Browser {
    /// The next change, or `None` once the daemon stops.
    pub async fn next(&mut self) -> Option<DiscoveryEvent> {
        loop {
            match self.events.recv_async().await.ok()? {
                ServiceEvent::ServiceResolved(service) => {
                    let values: HashMap<String, String> = service
                        .get_properties()
                        .iter()
                        .map(|property| {
                            (property.key().to_string(), property.val_str().to_string())
                        })
                        .collect();
                    let advertisement =
                        Advertisement::parse(|key| values.get(key).map(String::as_str));
                    return Some(DiscoveryEvent::Updated(Participant {
                        instance: instance_name(&service.fullname),
                        addresses: service
                            .get_addresses()
                            .iter()
                            .map(|address| address.to_ip_addr())
                            .collect(),
                        port: service.port,
                        advertisement,
                    }));
                }
                ServiceEvent::ServiceRemoved(_, fullname) => {
                    return Some(DiscoveryEvent::Removed {
                        instance: instance_name(&fullname),
                    });
                }
                _ => {}
            }
        }
    }
}

/// The instance label of a full service name, unescaped.
fn instance_name(fullname: &str) -> String {
    let suffix = format!(".{}", service_domain());
    let label = fullname.strip_suffix(&suffix).unwrap_or(fullname);
    let mut out = String::with_capacity(label.len());
    let mut chars = label.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next) = chars.next() {
                out.push(next);
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// A host label derived from a device name: letters, digits, and hyphens.
fn host_label(name: &str) -> String {
    let label: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let label = label.trim_matches('-');
    if label.is_empty() {
        "umsh".to_string()
    } else {
        label.chars().take(63).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_names_come_back_as_given() {
        assert_eq!(
            instance_name("Ridge Repeater._umsh._udp.local."),
            "Ridge Repeater"
        );
        assert_eq!(instance_name("A\\.B._umsh._udp.local."), "A.B");
    }

    #[test]
    fn host_labels_are_plain() {
        assert_eq!(host_label("Ridge Repeater #2"), "Ridge-Repeater--2");
        assert_eq!(host_label("…"), "umsh");
    }
}
