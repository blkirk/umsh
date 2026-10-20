//! `time`: read, set, or clear the device's wall clock (`PROP_TIME`,
//! `PROP_TZ_RULE`, `PROP_TIME_24H`).
//!
//! The clock is not persisted—an epoch written to flash accumulates
//! unbounded error while the device is off—so it comes back from a real
//! source or not at all. The time zone *is* persisted: where a device is
//! meant to be is known even when the time is not.

use anyhow::{Context as _, Result, bail};

use umsh::hal::tz::Rule;
use umsh_gnss::DateTime;

use super::persist;
use crate::App;
use crate::output::{UNAVAILABLE, field, note, or_unavailable, subfield};

#[derive(Debug, clap::Subcommand)]
pub enum TimeOp {
    /// Print the device's clock, in UTC and in its configured local time.
    Show,
    /// Set the clock from this host's clock.
    Sync,
    /// Set the clock to an explicit Unix second count.
    Set {
        #[arg(value_name = "UNIX-SECONDS")]
        epoch: u32,
    },
    /// Return the device to not knowing what time it is. A device with a
    /// screen stops showing a clock at all.
    Clear,
    /// Show or set the time zone.
    Tz {
        #[command(subcommand)]
        op: Option<TzOp>,
    },
    /// Show or set whether the device's clock writes hours from 0 to 23
    /// (24) or 1 to 12 (12).
    Hours {
        #[arg(value_parser = ["12", "24"])]
        hours: Option<String>,
    },
}

#[derive(Debug, clap::Subcommand)]
pub enum TzOp {
    /// Print the configured zone.
    Show,
    /// Set the zone, as a zone name such as `America/Los_Angeles`, read
    /// from this host's time zone database, or as a POSIX TZ rule such as
    /// `PST8PDT,M3.2.0,M11.1.0`.
    Set {
        #[arg(value_name = "ZONE|RULE", allow_hyphen_values = true)]
        zone: String,
    },
    /// Set the zone to this host's.
    Sync,
}

/// Where a host keeps its time zone database.
const ZONEINFO: &str = "/usr/share/zoneinfo";

/// The rule a zone follows today, from its TZif file.
fn tzif_rule(tzif: &[u8]) -> Option<String> {
    umsh::hal::tz::tzif_rule(tzif).map(str::to_owned)
}

/// `zone` as a rule: a zone name read from this host's database, or a
/// rule given as one.
fn zone_rule(zone: &str) -> Result<String> {
    if Rule::parse(zone).is_ok() {
        return Ok(zone.to_owned());
    }
    let path = std::path::Path::new(ZONEINFO).join(zone.trim_start_matches(':'));
    let tzif = std::fs::read(&path)
        .with_context(|| format!("{zone} is neither a POSIX TZ rule nor a zone in {ZONEINFO}"))?;
    tzif_rule(&tzif).with_context(|| format!("{} records no rule", path.display()))
}

/// This host's zone, as a rule: `TZ` if it is set, else `/etc/localtime`.
fn host_rule() -> Result<String> {
    if let Ok(zone) = std::env::var("TZ")
        && !zone.is_empty()
    {
        return zone_rule(&zone);
    }
    let tzif = std::fs::read("/etc/localtime")
        .context("this host has no /etc/localtime; give the zone with `time tz set`")?;
    tzif_rule(&tzif).context("/etc/localtime records no rule")
}

pub async fn run(app: &mut App, op: Option<TimeOp>) -> Result<()> {
    let no_save = app.no_save;
    let device = app.device()?;
    match op.unwrap_or(TimeOp::Show) {
        TimeOp::Show => {
            let Some(time) = device.time().await? else {
                bail!("device does not advertise CAP_TIME");
            };
            match time.epoch {
                Some(Some(epoch)) => {
                    field("time", format_utc(epoch));
                    subfield("epoch", epoch);
                    match time
                        .tz_rule
                        .as_deref()
                        .and_then(|rule| Rule::parse(rule).ok())
                    {
                        Some(rule) => match DateTime::from_unix(epoch)
                            .shifted(rule.offset_at(i64::from(epoch)))
                        {
                            Some(local) => subfield(
                                "local",
                                format!(
                                    "{} {}",
                                    format_civil(local),
                                    format_tz(rule.offset_at(i64::from(epoch)))
                                ),
                            ),
                            None => subfield("local", "outside the representable range"),
                        },
                        None => subfield("local", UNAVAILABLE),
                    }
                }
                Some(None) => {
                    field("time", "not set");
                    note("the device does not know what time it is and shows no clock");
                    subfield("zone", or_unavailable(time.tz_rule, |rule| rule));
                }
                None => {
                    field("time", UNAVAILABLE);
                    subfield("zone", or_unavailable(time.tz_rule, |rule| rule));
                }
            }
            return Ok(());
        }
        TimeOp::Sync => {
            let now = host_epoch()?;
            let stored = device.set_time(Some(now)).await?;
            report_set(stored);
        }
        TimeOp::Set { epoch } => {
            let stored = device.set_time(Some(epoch)).await?;
            report_set(stored);
        }
        TimeOp::Clear => {
            device.set_time(None).await?;
            println!("time cleared: the device no longer knows what time it is");
        }
        TimeOp::Tz { op } => match op.unwrap_or(TzOp::Show) {
            TzOp::Show => {
                let Some(time) = device.time().await? else {
                    bail!("device does not advertise CAP_TIME");
                };
                println!("time zone {}", or_unavailable(time.tz_rule, |rule| rule));
                return Ok(());
            }
            TzOp::Set { zone } => {
                let stored = device.set_tz_rule(&zone_rule(&zone)?).await?;
                println!("time zone {stored}");
            }
            TzOp::Sync => {
                let stored = device.set_tz_rule(&host_rule()?).await?;
                println!("time zone {stored} (from this host)");
            }
        },
        TimeOp::Hours { hours } => match hours {
            None => {
                let Some(time) = device.time().await? else {
                    bail!("device does not advertise CAP_TIME");
                };
                println!(
                    "hours {}",
                    or_unavailable(time.time_24h, |on| if on { "24" } else { "12" }.to_string())
                );
                return Ok(());
            }
            Some(hours) => {
                let on = device.set_time_24h(hours == "24").await?;
                println!("hours {}", if on { "24" } else { "12" });
            }
        },
    }
    persist(device, no_save).await
}

fn report_set(stored: Option<u32>) {
    match stored {
        Some(epoch) => println!("time {}", format_utc(epoch)),
        None => println!("time not set (the device refused the value)"),
    }
}

/// This host's wall clock as a Unix second count.
fn host_epoch() -> Result<u32> {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?;
    u32::try_from(now.as_secs())
        .map_err(|_| anyhow::anyhow!("this host's clock is outside the range PROP_TIME can carry"))
}

pub fn format_utc(epoch: u32) -> String {
    format!("{}Z", format_civil(DateTime::from_unix(epoch)))
}

fn format_civil(at: DateTime) -> String {
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        at.year, at.month, at.day, at.hour, at.minute, at.second
    )
}

/// Render an offset the way it is written, so a mistyped one is visible
/// as a zone rather than as a minute count nobody checks.
pub fn format_tz(minutes: i16) -> String {
    let sign = if minutes < 0 { '-' } else { '+' };
    let magnitude = minutes.unsigned_abs();
    format!("UTC{sign}{:02}:{:02}", magnitude / 60, magnitude % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zones_and_rules_both_name_a_rule() {
        assert_eq!(
            zone_rule("EST5EDT,M3.2.0,M11.1.0").unwrap(),
            "EST5EDT,M3.2.0,M11.1.0"
        );
        assert!(zone_rule("Nowhere/Special").is_err());
    }

    #[test]
    fn offsets_render_as_zones() {
        assert_eq!(format_tz(0), "UTC+00:00");
        assert_eq!(format_tz(-480), "UTC-08:00");
        assert_eq!(format_tz(330), "UTC+05:30");
        assert_eq!(format_tz(-30), "UTC-00:30");
    }

    #[test]
    fn the_host_zone_is_a_rule() {
        let rule = host_rule().expect("a host has a time zone");
        assert!(Rule::parse(&rule).is_ok(), "{rule}");
    }

    #[test]
    fn instants_render_as_civil_time() {
        assert_eq!(format_utc(0), "1970-01-01 00:00:00Z");
        assert_eq!(format_utc(1_000_000_000), "2001-09-09 01:46:40Z");
    }
}
