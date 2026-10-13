//! Rendering the extcap control protocol.
//!
//! Wireshark parses these as `keyword {key=value}{key=value}` lines, one
//! per line, so every interpolated value has to be kept clear of the
//! delimiters. Nothing here does I/O: the whole wire format is a pure
//! function of the tool's state, which is what makes it testable without
//! a radio or a Wireshark.

use crate::connection::Found;

/// The interface name. Wireshark writes this into its `recent` file and
/// into the preference keys for the configuration dialog, so it is
/// permanent: changing it silently orphans saved capture setups.
pub const INTERFACE: &str = "umsh";

/// The LAN bearer interface's name, permanent for the same reason.
pub const LAN_INTERFACE: &str = "umsh-lan";

/// pcap `LINKTYPE_LORATAP`. The radio frame arrives under a header
/// describing the channel it was heard on, so nothing has to be
/// invented the way the synthetic Ethernet encapsulation does.
pub const DLT_LORATAP: u32 = 270;

/// pcap `LINKTYPE_IPV4`. A LAN datagram is recorded with the addresses
/// it carried, so the sender of each frame is visible.
pub const DLT_IPV4: u32 = crate::command::capture::pcap::PCAP_LINKTYPE_IPV4;

/// The `{call=}` of the reloadable radio selector.
pub const CALL_RADIO: &str = "--radio";

/// Whether `--extcap-reload-option` names the radio selector.
///
/// Wireshark strips the leading `--` from a call before passing it back,
/// so the selector arrives as `radio`. The full spelling is accepted too,
/// for a person running the phase by hand.
pub fn is_radio_call(call: Option<&str>) -> bool {
    let Some(call) = call else {
        return false;
    };
    call.strip_prefix("--").unwrap_or(call) == CALL_RADIO.trim_start_matches('-')
}

/// Strip the delimiters of the extcap grammar out of a value.
///
/// Device names come off the radio, so a name containing a brace or a
/// newline would otherwise be able to forge fields or whole lines.
fn sanitize(value: &str) -> String {
    value
        .chars()
        .map(|c| match c {
            '{' | '}' => '(',
            '\r' | '\n' | '\t' => ' ',
            other => other,
        })
        .collect()
}

/// The reply to `--extcap-interfaces`.
///
/// The displays name no radio and no group. Wireshark fetches them once
/// at startup and shows them for the life of the session, while what is
/// actually captured is whatever the capture options pick.
pub fn interfaces() -> String {
    format!(
        "extcap {{version={}}}{{display=UMSH}}\n\
         interface {{value={INTERFACE}}}{{display=UMSH radio}}\n\
         interface {{value={LAN_INTERFACE}}}{{display=UMSH LAN}}\n",
        env!("CARGO_PKG_VERSION"),
    )
}

/// The reply to `--extcap-dlts` for `interface`.
///
/// Exactly one line each: Wireshark never passes a chosen DLT back on
/// `--capture`, so a second link type would be unactionable.
pub fn dlts(interface: &str) -> String {
    if interface == LAN_INTERFACE {
        format!("dlt {{number={DLT_IPV4}}}{{name=IPV4}}{{display=IPv4}}\n")
    } else {
        format!("dlt {{number={DLT_LORATAP}}}{{name=LORATAP}}{{display=LoRaTap}}\n")
    }
}

/// The reply to `--extcap-config` for the LAN interface, offering
/// `interfaces`—each a name and its IPv4 address—to join on.
pub fn lan_config(interfaces: &[(String, std::net::Ipv4Addr)]) -> String {
    let mut out = format!(
        "arg {{number=0}}{{call=--group}}{{display=Group}}\
         {{tooltip=GROUP[:PORT]. Empty is the default group.}}{{type=string}}\
         {{placeholder={}:{}}}{{required=false}}\n",
        std::net::Ipv4Addr::from(umsh_lan_bearer::DEFAULT_GROUP),
        umsh_lan_bearer::DEFAULT_PORT,
    );
    out.push_str(
        "arg {number=1}{call=--lan-interface}{display=Network interface}\
         {type=selector}{required=false}\n",
    );
    out.push_str("value {arg=1}{value=}{display=Automatic}{default=true}\n");
    for (name, address) in interfaces {
        out.push_str(&format!(
            "value {{arg=1}}{{value={address}}}{{display={} ({address})}}\n",
            sanitize(name),
        ));
    }
    out.push_str(
        "arg {number=2}{call=--umsh-only}{display=UMSH frames only}\
         {tooltip=Drop MeshCore and malformed frames before they reach Wireshark.}\
         {type=boolflag}{default=false}\n",
    );
    out
}

/// The reply to `--extcap-config`: the capture-options dialog.
///
/// The RF overrides are strings rather than numbers on purpose. A
/// numeric extcap argument renders as a spin box that always holds a
/// value and is always passed, which would rewrite the live PHY of a
/// radio the user only wanted to listen to. Empty means "leave the
/// radio alone".
pub fn config() -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "arg {{number=0}}{{call={CALL_RADIO}}}{{display=Radio}}\
         {{tooltip=BLE radio to capture from. Leave empty to use the saved default, \
         then discover.}}{{type=selector}}{{reload=true}}{{placeholder=Scan for radios}}\
         {{required=false}}{{group=Connection}}\n"
    ));
    out.push_str(&radio_value_auto());
    out.push_str(
        "arg {number=1}{call=--serial-port}{display=Serial port}\
         {tooltip=Capture over a named serial port instead of BLE. A port must be named: \
         umshctl never probes serial ports, because opening one can reset or DFU-trigger \
         hardware.}{type=string}{required=false}{group=Connection}\n",
    );
    out.push_str(
        "arg {number=2}{call=--baud}{display=Serial bit rate}{type=unsigned}\
         {default=115200}{required=false}{group=Connection}\n",
    );
    out.push_str(
        "arg {number=3}{call=--umsh-only}{display=UMSH frames only}\
         {tooltip=Drop frames that are not valid UMSH before they reach Wireshark.}\
         {type=boolflag}{default=false}{group=Capture}\n",
    );
    out.push_str(
        "arg {number=4}{call=--idle-probe-secs}{display=Idle health probe (s)}\
         {tooltip=Seconds of silence before the link and channel RSSI are re-checked.}\
         {type=unsigned}{default=10}{range=1,3600}{group=Capture}\n",
    );
    out.push_str(
        "arg {number=5}{call=--no-reconnect}{display=Do not recover a dropped link}\
         {type=boolflag}{default=false}{group=Capture}\n",
    );
    for (number, call, display, hint) in [
        (6, "--freq-khz", "Frequency (kHz)", ""),
        (7, "--bw-hz", "Bandwidth (Hz)", ""),
        (8, "--sf", "Spreading factor", " (5-12)"),
        (9, "--cr", "Coding rate denominator", " (5-8)"),
        (10, "--sync-word", "Sync word (hex)", ""),
    ] {
        out.push_str(&format!(
            "arg {{number={number}}}{{call={call}}}{{display={display}{hint}}}\
             {{tooltip=Live-only override, never saved to the radio. Leave empty to \
             capture on the radio's current configuration.}}{{type=string}}\
             {{required=false}}{{group=RF overrides}}\n"
        ));
    }
    out
}

/// The always-present first entry of the radio selector.
fn radio_value_auto() -> String {
    "value {arg=0}{value=}{display=Auto (saved default, then discover)}{default=true}\n".to_string()
}

/// The reply to `--extcap-reload-option=--radio`: what a scan just saw.
///
/// This is the one phase allowed to touch the radio, and only because
/// the user pressed Reload in the dialog.
pub fn radio_values(found: &[Found]) -> String {
    let mut out = radio_value_auto();
    for entry in found {
        let rssi = match entry.rssi {
            Some(rssi) => format!("  ({rssi} dBm)"),
            None => String::new(),
        };
        out.push_str(&format!(
            "value {{arg=0}}{{value={}}}{{display={}{}}}\n",
            sanitize(&entry.id),
            sanitize(&entry.name),
            rssi,
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(id: &str, name: &str, rssi: Option<i16>) -> Found {
        Found {
            id: id.to_string(),
            name: name.to_string(),
            rssi,
        }
    }

    #[test]
    fn the_interfaces_name_no_radio_and_no_group() {
        let text = interfaces();
        assert!(text.contains("interface {value=umsh}{display=UMSH radio}\n"));
        assert!(text.contains("interface {value=umsh-lan}{display=UMSH LAN}\n"));
    }

    /// Wireshark cannot tell us which DLT the user picked, so offering a
    /// choice would be a lie.
    #[test]
    fn exactly_one_dlt_is_offered() {
        let text = dlts(INTERFACE);
        assert_eq!(text.lines().count(), 1);
        assert!(text.contains("{number=270}"));
        let text = dlts(LAN_INTERFACE);
        assert_eq!(text.lines().count(), 1);
        assert!(text.contains("{number=228}"));
    }

    #[test]
    fn the_lan_dialog_offers_every_interface_and_automatic() {
        let text = lan_config(&[
            ("en0".into(), std::net::Ipv4Addr::new(192, 168, 1, 5)),
            ("evil}\nx".into(), std::net::Ipv4Addr::new(10, 0, 0, 1)),
        ]);
        assert!(text.contains("{value=}{display=Automatic}{default=true}"));
        assert!(text.contains("{value=192.168.1.5}{display=en0 (192.168.1.5)}"));
        assert!(!text.contains("evil}"));
        assert_numbered(&text);
    }

    #[test]
    fn config_numbers_are_contiguous_and_calls_unique() {
        assert_numbered(&config());
    }

    fn assert_numbered(text: &str) {
        let mut numbers = Vec::new();
        let mut calls = Vec::new();
        for line in text.lines().filter(|line| line.starts_with("arg ")) {
            let number = line
                .split("{number=")
                .nth(1)
                .and_then(|rest| rest.split('}').next())
                .unwrap()
                .parse::<usize>()
                .unwrap();
            let call = line
                .split("{call=")
                .nth(1)
                .and_then(|rest| rest.split('}').next())
                .unwrap()
                .to_string();
            numbers.push(number);
            calls.push(call);
        }
        assert_eq!(
            numbers,
            (0..numbers.len()).collect::<Vec<_>>(),
            "a gap or duplicate silently drops an argument from the dialog",
        );
        let mut unique = calls.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), calls.len(), "duplicate {{call=}}: {calls:?}");
    }

    /// Every `type=selector` needs at least one value line, or the
    /// dialog renders an empty, unusable dropdown.
    #[test]
    fn the_radio_selector_always_offers_auto() {
        assert!(config().contains("value {arg=0}{value=}"));
        assert!(radio_values(&[]).contains("{value=}"));
    }

    #[test]
    fn the_radio_call_matches_with_or_without_dashes() {
        assert!(is_radio_call(Some("radio")));
        assert!(is_radio_call(Some("--radio")));
        assert!(!is_radio_call(Some("serial-port")));
        assert!(!is_radio_call(Some("radios")));
        assert!(!is_radio_call(None));
    }

    #[test]
    fn radio_values_render_name_and_rssi() {
        let text = radio_values(&[
            found("id-1", "T-Echo", Some(-52)),
            found("id-2", "Solar", None),
        ]);
        assert!(
            text.contains("{value=id-1}{display=T-Echo  (-52 dBm)}"),
            "{text}"
        );
        assert!(text.contains("{value=id-2}{display=Solar}"), "{text}");
    }

    /// A device names itself over the air, so the name is untrusted
    /// input to a brace-delimited grammar.
    #[test]
    fn a_hostile_device_name_cannot_forge_lines() {
        let text = radio_values(&[found("id", "evil}\ninterface {value=fake", None)]);
        assert_eq!(
            text.lines().count(),
            2,
            "auto entry plus exactly one device: {text}",
        );
        assert!(!text.contains("interface {value=fake"));
    }
}
