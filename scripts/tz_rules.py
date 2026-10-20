#!/usr/bin/env python3
"""List the time zone rules the zone database uses today.

The communicator's time zone list (crates/umsh-ux-communicator/src/zones.rs)
holds one row per distinct POSIX TZ rule, named by hand. This reads the
rule each zone follows today from the TZif footers of the host's zone
database and prints:

* every distinct rule, west to east, with the zones that follow it, and
* with --check, the rules missing from zones.rs and the rules zones.rs
  holds that no zone follows any more.

Stdlib only. Run it against an up-to-date database (ZONEINFO=/path to
point elsewhere).
"""

import argparse
import os
import re
import sys
from pathlib import Path

ZONEINFO = Path(os.environ.get("ZONEINFO", "/usr/share/zoneinfo"))
TABLE = Path(__file__).resolve().parent.parent / "crates/umsh-ux-communicator/src/zones.rs"


def zones():
    """The zone names the database lists, from zone1970.tab or zone.tab."""
    for name in ("zone1970.tab", "zone.tab"):
        path = ZONEINFO / name
        if path.exists():
            names = set()
            for line in path.read_text().splitlines():
                if line and not line.startswith("#"):
                    names.add(line.split("\t")[2])
            return sorted(names)
    sys.exit(f"no zone1970.tab or zone.tab in {ZONEINFO}")


def footer(path):
    """The POSIX TZ rule at the end of a TZif v2+ file, or None."""
    data = path.read_bytes()
    if not data.startswith(b"TZif") or data[4:5] < b"2" or not data.endswith(b"\n"):
        return None
    rule = data[:-1].rsplit(b"\n", 1)[-1].decode()
    return rule or None


def offset_minutes(text):
    """Minutes east of UTC of a POSIX offset such as `8`, `-5:30`."""
    sign = -1
    if text[0] in "+-":
        sign = -1 if text[0] == "+" else 1
        text = text[1:]
    parts = [int(p) for p in text.split(":")] + [0, 0]
    return sign * (parts[0] * 60 + parts[1])


RULE = re.compile(r"^(<[^>]+>|[A-Za-z]{3,})([-+]?[0-9:]+)")


def std_offset(rule):
    match = RULE.match(rule)
    return offset_minutes(match.group(2)) if match else 0


def table_rules():
    """The rules zones.rs holds."""
    return set(re.findall(r'zone\("[^"]*",\s*"([^"]+)"\)', TABLE.read_text()))


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--check", action="store_true", help="compare against zones.rs")
    args = parser.parse_args()

    by_rule = {}
    for name in zones():
        path = ZONEINFO / name
        if not path.is_file():
            continue
        rule = footer(path)
        if rule:
            by_rule.setdefault(rule, []).append(name)

    if args.check:
        held = table_rules()
        missing = sorted(set(by_rule) - held, key=std_offset)
        unused = sorted(held - set(by_rule), key=std_offset)
        for rule in missing:
            print(f"missing  {rule:40} {', '.join(by_rule[rule][:4])}")
        for rule in unused:
            print(f"unused   {rule}")
        return 1 if missing or unused else 0

    for rule in sorted(by_rule, key=lambda r: (std_offset(r), r)):
        print(f"{rule:40} {', '.join(by_rule[rule][:6])}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
