//! The phone's time zone as the POSIX TZ rule `PROP_TZ_RULE` carries.

use umsh_hal::tz::{Rule, tzif_rule};
use umsh_ulcp_device::session::TZ_RULE_MAX_LEN;

/// Where the zone database lives on Apple's platforms, as on most Unix
/// systems.
const ZONEINFO: &str = "/usr/share/zoneinfo";

/// Whether a device accepts `rule` as its `PROP_TZ_RULE`: it parses, fits,
/// and keeps offsets within UTC−12:00 through UTC+14:00.
pub(crate) fn device_accepts(rule: &str) -> bool {
    if rule.len() > TZ_RULE_MAX_LEN {
        return false;
    }
    let Ok(parsed) = Rule::parse(rule) else {
        return false;
    };
    let civil = -12 * 60..=14 * 60;
    core::iter::once(parsed.std_offset_min())
        .chain(parsed.dst_offset_min())
        .all(|minutes| civil.contains(&minutes))
}

/// The rule a zone such as `America/Los_Angeles` follows now, from the
/// system's zone database. `None` when the zone is not there or its rule
/// is not one a device accepts.
#[uniffi::export]
pub fn zone_rule(identifier: String) -> Option<String> {
    if identifier.is_empty() || identifier.starts_with('/') || identifier.contains("..") {
        return None;
    }
    let tzif = std::fs::read(format!("{ZONEINFO}/{identifier}")).ok()?;
    let rule = tzif_rule(&tzif)?;
    device_accepts(rule).then(|| rule.to_owned())
}

/// A rule for a zone that keeps `minutes_east` all year, named by its
/// offset: −420 is `<-07>7`, 330 is `<+0530>-5:30`, and 0 is `UTC0`.
#[uniffi::export]
pub fn fixed_offset_rule(minutes_east: i16) -> String {
    if minutes_east == 0 {
        return "UTC0".to_owned();
    }
    let magnitude = minutes_east.unsigned_abs();
    let (hours, minutes) = (magnitude / 60, magnitude % 60);
    // A rule's offset is the one to add to local time to reach UTC, the
    // opposite sign of the abbreviation's.
    let (name_sign, rule_sign) = if minutes_east > 0 {
        ('+', "-")
    } else {
        ('-', "")
    };
    if minutes == 0 {
        format!("<{name_sign}{hours:02}>{rule_sign}{hours}")
    } else {
        format!("<{name_sign}{hours:02}{minutes:02}>{rule_sign}{hours}:{minutes:02}")
    }
}

/// The offset `rule` keeps at `unix_seconds`, in minutes east of UTC.
#[uniffi::export]
pub fn rule_offset_min(rule: String, unix_seconds: i64) -> Option<i16> {
    Rule::parse(&rule)
        .ok()
        .map(|parsed| parsed.offset_at(unix_seconds))
}

/// The abbreviation `rule` uses at `unix_seconds`, such as `PDT` or
/// `+0530`.
#[uniffi::export]
pub fn rule_abbreviation(rule: String, unix_seconds: i64) -> Option<String> {
    let parsed = Rule::parse(&rule).ok()?;
    let name = if parsed.is_dst_at(unix_seconds) {
        parsed.dst_name()?
    } else {
        parsed.std_name()
    };
    Some(name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_offset_rules_read_back_as_their_offset() {
        assert_eq!(fixed_offset_rule(-420), "<-07>7");
        assert_eq!(fixed_offset_rule(330), "<+0530>-5:30");
        assert_eq!(fixed_offset_rule(0), "UTC0");
        assert_eq!(fixed_offset_rule(-570), "<-0930>9:30");
        assert_eq!(fixed_offset_rule(840), "<+14>-14");
        for minutes in -12 * 60..=14 * 60 {
            let rule = fixed_offset_rule(minutes);
            assert!(device_accepts(&rule), "{rule}");
            let parsed = Rule::parse(&rule).unwrap();
            assert_eq!(parsed.std_offset_min(), minutes, "{rule}");
            assert_eq!(parsed.dst_offset_min(), None, "{rule}");
        }
    }

    #[test]
    fn rule_offset_follows_daylight_saving() {
        let rule = "PST8PDT,M3.2.0,M11.1.0".to_owned();
        // 2026-01-15 and 2026-07-15, both at 12:00 UTC.
        assert_eq!(rule_offset_min(rule.clone(), 1_768_478_400), Some(-480));
        assert_eq!(rule_offset_min(rule.clone(), 1_784_116_800), Some(-420));
        assert_eq!(
            rule_abbreviation(rule.clone(), 1_784_116_800).as_deref(),
            Some("PDT")
        );
        assert_eq!(
            rule_abbreviation(rule, 1_768_478_400).as_deref(),
            Some("PST")
        );
        assert_eq!(rule_offset_min("nonsense".to_owned(), 0), None);
    }

    #[test]
    fn device_accepts_only_civil_rules() {
        assert!(device_accepts("UTC0"));
        assert!(device_accepts("CET-1CEST,M3.5.0,M10.5.0/3"));
        assert!(!device_accepts("<-13>13"));
        assert!(!device_accepts("<+15>-15"));
        assert!(!device_accepts(""));
        assert!(!device_accepts(&format!(
            "UTC0{}",
            " ".repeat(TZ_RULE_MAX_LEN)
        )));
    }

    #[test]
    fn zone_identifiers_stay_inside_the_database() {
        assert_eq!(zone_rule("../../etc/passwd".to_owned()), None);
        assert_eq!(zone_rule("/etc/passwd".to_owned()), None);
        assert_eq!(zone_rule(String::new()), None);
    }

    #[test]
    fn zone_rule_reads_the_system_database() {
        // Where the host has a database at all: Apple's and most Linux
        // systems ship one, a minimal container may not.
        if !std::path::Path::new(ZONEINFO).join("UTC").exists() {
            return;
        }
        let rule = zone_rule("America/Los_Angeles".to_owned()).unwrap();
        assert_eq!(rule, "PST8PDT,M3.2.0,M11.1.0");
    }
}
