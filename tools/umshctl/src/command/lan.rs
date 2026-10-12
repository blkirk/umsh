//! Configure a radio's LAN bearer, and list who is on the LAN.
use std::net::{Ipv4Addr, SocketAddrV4};
use std::time::Duration;

use super::persist;
use crate::{App, output::subfield};
use anyhow::{Result, bail};
use umsh::ulcp_wire::{
    ids::prop,
    lan::{Group, State},
};
use umsh_lan_bearer::dnssd::{Discovery, DiscoveryEvent};

#[derive(Debug, clap::Subcommand)]
pub enum LanOp {
    Show,
    On,
    Off,
    /// Set the multicast group, as `GROUP:PORT`.
    Group {
        #[arg(value_parser = parse_group)]
        group: SocketAddrV4,
    },
    /// Set the ingress limit, in frames per minute.
    Limit {
        #[arg(value_parser = clap::value_parser!(u16).range(1..))]
        per_minute: u16,
    },
    /// List the participants advertising on the LAN.
    Browse {
        /// How long to listen, in seconds.
        #[arg(long, default_value_t = 3)]
        timeout: u64,
    },
}

impl LanOp {
    /// Browsing is a host's own business; no radio is involved.
    pub fn needs_device(&self) -> bool {
        !matches!(self, Self::Browse { .. })
    }
}

fn parse_group(spec: &str) -> Result<SocketAddrV4> {
    crate::parse_group(spec)
}

pub async fn run(app: &mut App, op: Option<LanOp>) -> Result<()> {
    if let Some(LanOp::Browse { timeout }) = op {
        return browse(Duration::from_secs(timeout)).await;
    }
    let no_save = app.no_save;
    let device = app.device()?;
    match op.unwrap_or(LanOp::Show) {
        LanOp::Show => {
            let [enabled, group, limit, state] = device
                .read_all(&[
                    prop::LAN_BEARER_ENABLED,
                    prop::LAN_BEARER_GROUP,
                    prop::LAN_BEARER_INGRESS_LIMIT,
                    prop::LAN_BEARER_STATE,
                ])
                .await?;
            subfield(
                "bearer",
                if enabled == [1] {
                    "enabled"
                } else {
                    "disabled"
                },
            );
            if let Some(group) = Group::decode(&group) {
                subfield(
                    "group",
                    SocketAddrV4::new(group.address.into(), group.port).to_string(),
                );
            }
            if let Ok(limit) = <[u8; 2]>::try_from(limit.as_slice()) {
                subfield(
                    "ingress limit",
                    format!("{} frames/min", u16::from_le_bytes(limit)),
                );
            }
            if let Some(state) = state.first().copied().and_then(State::decode) {
                subfield(
                    "state",
                    match state {
                        State::Disabled => "disabled",
                        State::Active => "active",
                        State::NoNetwork => "no network",
                    },
                );
            }
            return Ok(());
        }
        LanOp::On => {
            device.set_prop(prop::LAN_BEARER_ENABLED, &[1]).await?;
        }
        LanOp::Off => {
            device.set_prop(prop::LAN_BEARER_ENABLED, &[0]).await?;
        }
        LanOp::Group { group } => {
            let group = Group {
                address: group.ip().octets(),
                port: group.port(),
            };
            device
                .set_prop(prop::LAN_BEARER_GROUP, &group.encode())
                .await?;
        }
        LanOp::Limit { per_minute } => {
            device
                .set_prop(prop::LAN_BEARER_INGRESS_LIMIT, &per_minute.to_le_bytes())
                .await?;
        }
        LanOp::Browse { .. } => unreachable!("handled above"),
    }
    persist(device, no_save).await
}

/// Print each participant as it is resolved, for `timeout`.
async fn browse(timeout: Duration) -> Result<()> {
    let discovery = Discovery::new()?;
    let mut browser = discovery.browse()?;
    let mut seen = 0usize;
    let deadline = tokio::time::sleep(timeout);
    tokio::pin!(deadline);
    loop {
        let event = tokio::select! {
            _ = &mut deadline => break,
            event = browser.next() => event,
        };
        let Some(event) = event else { break };
        let DiscoveryEvent::Updated(participant) = event else {
            continue;
        };
        seen += 1;
        println!("{}", participant.instance);
        let addresses: Vec<String> = participant
            .addresses
            .iter()
            .map(ToString::to_string)
            .collect();
        subfield("addresses", addresses.join(", "));
        match participant.advertisement {
            Ok(advert) => {
                subfield(
                    "group",
                    SocketAddrV4::new(Ipv4Addr::from(advert.group), participant.port).to_string(),
                );
                subfield(
                    "identity",
                    umsh::core::PublicKey(advert.node_key).to_string(),
                );
                match advert.lora {
                    Some(lora) => {
                        subfield(
                            "lora",
                            format!(
                                "{:.3} MHz, {} kHz, SF{}, CR 4/{}",
                                lora.freq_hz as f64 / 1e6,
                                lora.bw_hz as f64 / 1e3,
                                lora.sf,
                                lora.cr
                            ),
                        );
                        subfield("role", if lora.repeater { "repeater" } else { "leaf" });
                    }
                    None => subfield("lora", "none"),
                }
                if advert.meshcore {
                    subfield("meshcore", "forwarding");
                }
                if advert.internet_bridge {
                    subfield("internet bridge", "connected");
                }
            }
            Err(error) => subfield("record", format!("unreadable ({error:?})")),
        }
    }
    if seen == 0 {
        bail!("no participants found on the LAN");
    }
    Ok(())
}
