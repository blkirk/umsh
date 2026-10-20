//! Time zone rules in POSIX form (IEEE 1003.1 §8.3, with the extensions
//! of RFC 8536 §3.3.1): a standard offset and, for a zone that keeps
//! daylight saving, a second offset and the two moments each year the
//! clock changes between them.
//!
//! Offsets here are minutes *east* of UTC, the opposite sign of a rule's
//! text: `PST8PDT,M3.2.0,M11.1.0` keeps −480 in winter and −420 in summer.

/// A zone abbreviation as a rule names it, such as `PST` or `+0530`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Abbreviation {
    bytes: [u8; Self::MAX],
    len: u8,
}

impl Abbreviation {
    /// The longest abbreviation kept. tzdb's are at most six.
    pub const MAX: usize = 8;

    /// The abbreviation's text.
    pub fn as_str(&self) -> &str {
        // Only ASCII letters, digits, `+`, and `-` are ever stored.
        core::str::from_utf8(&self.bytes[..usize::from(self.len)]).unwrap_or("")
    }
}

impl core::fmt::Debug for Abbreviation {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Debug::fmt(self.as_str(), f)
    }
}

/// The day of the year a change falls on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Day {
    /// `Jn`: day 1 to 365, never counting February 29.
    Julian(u16),
    /// `n`: day 0 to 365, counting February 29.
    Ordinal(u16),
    /// `Mm.w.d`: weekday `d` (0 is Sunday) of week `w` of month `m`, the
    /// fifth week being the last.
    Weekday { month: u8, week: u8, weekday: u8 },
}

/// When in the year a change happens: a day, and seconds past that day's
/// local midnight, which may run past the day or before it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Change {
    day: Day,
    time: i32,
}

/// One of a zone's two offsets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Zone {
    name: Abbreviation,
    offset_min: i16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Dst {
    zone: Zone,
    /// In standard time.
    start: Change,
    /// In daylight time.
    end: Change,
}

/// A time zone rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rule {
    std: Zone,
    dst: Option<Dst>,
}

/// The text is not a POSIX TZ rule, or names an offset this does not keep.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Invalid;

/// What a rule naming daylight time but no changes follows, as tzcode
/// does: the US rules since 2007.
const DEFAULT_START: Change = Change {
    day: Day::Weekday {
        month: 3,
        week: 2,
        weekday: 0,
    },
    time: 2 * 3_600,
};
const DEFAULT_END: Change = Change {
    day: Day::Weekday {
        month: 11,
        week: 1,
        weekday: 0,
    },
    time: 2 * 3_600,
};

impl Rule {
    /// `UTC0`.
    pub const UTC: Self = Self {
        std: Zone {
            name: Abbreviation {
                bytes: *b"UTC\0\0\0\0\0",
                len: 3,
            },
            offset_min: 0,
        },
        dst: None,
    };

    /// Reads a rule such as `CET-1CEST,M3.5.0,M10.5.0/3` or `<+0530>-5:30`.
    ///
    /// Offsets must be whole minutes, as every zone's are today, and
    /// abbreviations at most [`Abbreviation::MAX`] long.
    pub fn parse(text: &str) -> Result<Self, Invalid> {
        let mut p = Parser {
            text: text.as_bytes(),
            at: 0,
        };
        let std = Zone {
            name: p.name()?,
            offset_min: p.offset()?,
        };
        if p.done() {
            return Ok(Self { std, dst: None });
        }
        let name = p.name()?;
        let offset_min = if p.done() || p.peek() == Some(b',') {
            std.offset_min + 60
        } else {
            p.offset()?
        };
        let (start, end) = if p.done() {
            (DEFAULT_START, DEFAULT_END)
        } else {
            p.expect(b',')?;
            let start = p.change()?;
            p.expect(b',')?;
            (start, p.change()?)
        };
        if !p.done() {
            return Err(Invalid);
        }
        Ok(Self {
            std,
            dst: Some(Dst {
                zone: Zone { name, offset_min },
                start,
                end,
            }),
        })
    }

    /// Standard time's offset, in minutes east of UTC.
    pub const fn std_offset_min(&self) -> i16 {
        self.std.offset_min
    }

    /// Daylight time's offset, for a zone that keeps it.
    pub const fn dst_offset_min(&self) -> Option<i16> {
        match &self.dst {
            Some(dst) => Some(dst.zone.offset_min),
            None => None,
        }
    }

    /// Standard time's abbreviation.
    pub fn std_name(&self) -> &str {
        self.std.name.as_str()
    }

    /// Daylight time's abbreviation, for a zone that keeps it.
    pub fn dst_name(&self) -> Option<&str> {
        self.dst.as_ref().map(|dst| dst.zone.name.as_str())
    }

    /// Whether `other` keeps the same time as this, whatever either names
    /// its offsets.
    pub fn same_time_as(&self, other: &Self) -> bool {
        self.std.offset_min == other.std.offset_min
            && match (&self.dst, &other.dst) {
                (None, None) => true,
                (Some(a), Some(b)) => {
                    a.zone.offset_min == b.zone.offset_min && a.start == b.start && a.end == b.end
                }
                _ => false,
            }
    }

    /// Whether daylight time is kept at `unix`, in Unix seconds.
    pub fn is_dst_at(&self, unix: i64) -> bool {
        let Some(dst) = &self.dst else {
            return false;
        };
        let std = i64::from(self.std.offset_min) * 60;
        let year = civil(local_days(unix, std)).0;
        let start = dst.start.at(year) - std;
        let end = dst.end.at(year) - i64::from(dst.zone.offset_min) * 60;
        if start <= end {
            start <= unix && unix < end
        } else {
            // A southern zone: daylight time spans the new year.
            !(end <= unix && unix < start)
        }
    }

    /// Local time's offset at `unix`, in minutes east of UTC.
    pub fn offset_at(&self, unix: i64) -> i16 {
        match &self.dst {
            Some(dst) if self.is_dst_at(unix) => dst.zone.offset_min,
            _ => self.std.offset_min,
        }
    }

    /// The instant a local reading of `local` seconds names. A reading
    /// the clock passes twice, as daylight time ends, is the earlier
    /// instant; one it skips, as daylight time starts, is read in the
    /// offset before the change.
    pub fn to_utc(&self, local: i64) -> i64 {
        let std = local - i64::from(self.std.offset_min) * 60;
        let Some(dst) = &self.dst else {
            return std;
        };
        let daylight = local - i64::from(dst.zone.offset_min) * 60;
        let fits = |unix: i64| i64::from(self.offset_at(unix)) * 60 + unix == local;
        match (fits(std), fits(daylight)) {
            (true, true) => std.min(daylight),
            (true, false) => std,
            (false, true) => daylight,
            // Skipped: the offset in force just before the change.
            (false, false) => {
                if self.is_dst_at(std.min(daylight) - 1) {
                    daylight
                } else {
                    std
                }
            }
        }
    }
}

impl Change {
    /// This change's local moment in `year`, in seconds since the epoch
    /// as if local time were UTC.
    fn at(&self, year: i64) -> i64 {
        let jan1 = days_from_civil(year, 1, 1);
        let leap = is_leap(year);
        let day = match self.day {
            Day::Julian(n) => jan1 + i64::from(n) - 1 + i64::from(leap && n >= 60),
            Day::Ordinal(n) => jan1 + i64::from(n),
            Day::Weekday {
                month,
                week,
                weekday,
            } => {
                let first = days_from_civil(year, u32::from(month), 1);
                let first_weekday = (first + 4).rem_euclid(7);
                let mut day = first
                    + (i64::from(weekday) - first_weekday).rem_euclid(7)
                    + 7 * (i64::from(week) - 1);
                let next = if month == 12 {
                    days_from_civil(year + 1, 1, 1)
                } else {
                    days_from_civil(year, u32::from(month) + 1, 1)
                };
                while day >= next {
                    day -= 7;
                }
                day
            }
        };
        day * 86_400 + i64::from(self.time)
    }
}

struct Parser<'a> {
    text: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.text.get(self.at).copied()
    }

    fn done(&self) -> bool {
        self.at == self.text.len()
    }

    fn eat(&mut self, byte: u8) -> bool {
        let found = self.peek() == Some(byte);
        if found {
            self.at += 1;
        }
        found
    }

    fn expect(&mut self, byte: u8) -> Result<(), Invalid> {
        self.eat(byte).then_some(()).ok_or(Invalid)
    }

    /// Letters, or anything of letters, digits, `+`, and `-` in angle
    /// brackets: three characters at least.
    fn name(&mut self) -> Result<Abbreviation, Invalid> {
        let quoted = self.eat(b'<');
        let begin = self.at;
        while let Some(byte) = self.peek() {
            let allowed = byte.is_ascii_alphabetic()
                || (quoted && (byte.is_ascii_digit() || byte == b'+' || byte == b'-'));
            if !allowed {
                break;
            }
            self.at += 1;
        }
        let name = &self.text[begin..self.at];
        if quoted {
            self.expect(b'>')?;
        }
        if name.len() < 3 || name.len() > Abbreviation::MAX {
            return Err(Invalid);
        }
        let mut bytes = [0; Abbreviation::MAX];
        bytes[..name.len()].copy_from_slice(name);
        Ok(Abbreviation {
            bytes,
            len: name.len() as u8,
        })
    }

    /// Up to `digits` decimal digits, at least one.
    fn number(&mut self, digits: usize) -> Result<u32, Invalid> {
        let begin = self.at;
        let mut value = 0;
        while let Some(digit @ b'0'..=b'9') = self.peek() {
            if self.at - begin == digits {
                return Err(Invalid);
            }
            value = value * 10 + u32::from(digit - b'0');
            self.at += 1;
        }
        if self.at == begin {
            return Err(Invalid);
        }
        Ok(value)
    }

    /// `[+-]h[:mm[:ss]]`, in seconds, with at most `hours`.
    fn duration(&mut self, hours: u32) -> Result<i32, Invalid> {
        let negative = self.eat(b'-');
        if !negative {
            self.eat(b'+');
        }
        let h = self.number(3)?;
        let (mut m, mut s) = (0, 0);
        if self.eat(b':') {
            m = self.number(2)?;
            if self.eat(b':') {
                s = self.number(2)?;
            }
        }
        if h > hours || m > 59 || s > 59 {
            return Err(Invalid);
        }
        let total = (h * 3_600 + m * 60 + s) as i32;
        Ok(if negative { -total } else { total })
    }

    /// An offset, in the rule's west-positive sense, as minutes east.
    fn offset(&mut self) -> Result<i16, Invalid> {
        let west = self.duration(24)?;
        if west % 60 != 0 {
            return Err(Invalid);
        }
        Ok((-west / 60) as i16)
    }

    fn change(&mut self) -> Result<Change, Invalid> {
        let day = if self.eat(b'J') {
            let n = self.number(3)?;
            if !(1..=365).contains(&n) {
                return Err(Invalid);
            }
            Day::Julian(n as u16)
        } else if self.eat(b'M') {
            let month = self.number(2)?;
            self.expect(b'.')?;
            let week = self.number(1)?;
            self.expect(b'.')?;
            let weekday = self.number(1)?;
            if !(1..=12).contains(&month) || !(1..=5).contains(&week) || weekday > 6 {
                return Err(Invalid);
            }
            Day::Weekday {
                month: month as u8,
                week: week as u8,
                weekday: weekday as u8,
            }
        } else {
            let n = self.number(3)?;
            if n > 365 {
                return Err(Invalid);
            }
            Day::Ordinal(n as u16)
        };
        let time = if self.eat(b'/') {
            self.duration(167)?
        } else {
            2 * 3_600
        };
        Ok(Change { day, time })
    }
}

/// The rule a zone follows today, from its TZif file (RFC 8536): the
/// footer that version 2 and later carry, when it is one [`Rule::parse`]
/// reads. A zone database keeps one such file per zone, by name.
pub fn tzif_rule(tzif: &[u8]) -> Option<&str> {
    if !tzif.starts_with(b"TZif") || tzif.get(4).is_none_or(|version| *version < b'2') {
        return None;
    }
    let body = tzif.strip_suffix(b"\n")?;
    let start = body.iter().rposition(|byte| *byte == b'\n')? + 1;
    let rule = core::str::from_utf8(&body[start..]).ok()?;
    Rule::parse(rule).ok()?;
    Some(rule)
}

/// The local day `unix` falls on, `offset` seconds east of UTC.
fn local_days(unix: i64, offset: i64) -> i64 {
    (unix + offset).div_euclid(86_400)
}

fn is_leap(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

/// Days since 1970-01-01 of a date in the proleptic Gregorian calendar.
pub fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year.rem_euclid(400);
    let month = i64::from(month);
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The year, month, and day of the month of `days` since 1970-01-01, in
/// the proleptic Gregorian calendar.
pub fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rules covering every form: northern and southern daylight time,
    /// half- and quarter-hour offsets, a half-hour change, changes before
    /// midnight and past the day, and day numbers.
    const RULES: [&str; 23] = [
        "UTC0",
        "<+14>-14",
        "<-12>12",
        "PST8PDT,M3.2.0,M11.1.0",
        "NST3:30NDT,M3.2.0,M11.1.0",
        "CET-1CEST,M3.5.0,M10.5.0/3",
        "GMT0BST,M3.5.0/1,M10.5.0",
        "IST-5:30",
        "<+0545>-5:45",
        "AEST-10AEDT,M10.1.0,M4.1.0/3",
        "<+1030>-10:30<+11>-11,M10.1.0,M4.1.0",
        "NZST-12NZDT,M9.5.0,M4.1.0/3",
        "<-03>3<-02>,M3.5.0/-2,M10.5.0/-1",
        "IST-2IDT,M3.4.4/26,M10.5.0",
        "CST5CDT,M3.2.0/0,M11.1.0/1",
        "<-02>2<-01>,M3.5.0/-1,M10.5.0/0",
        "XXX3YYY,J60/2,300/2",
        "<-06>6<-05>,M9.1.6/22,M4.1.6/22",
        "<-04>4<-03>,M9.1.6/24,M4.1.6/24",
        "<+00>0<+02>-2,M3.5.0/1,M10.5.0/3",
        "EET-2EEST,M3.4.4/50,M10.4.4/50",
        "EET-2EEST,M4.5.5/0,M10.5.4/24",
        "<+1245>-12:45<+1345>,M9.5.0/2:45,M4.1.0/3:45",
    ];

    /// 2023-01-01 to 2031-01-01.
    const FROM: i64 = 1_672_531_200;
    const UNTIL: i64 = 1_924_992_000;

    #[test]
    fn keeps_the_same_time_as_jiff() {
        for text in RULES {
            let rule = Rule::parse(text).unwrap_or_else(|_| panic!("{text}"));
            let zone = jiff::tz::TimeZone::posix(text).unwrap();
            let theirs = |unix: i64| {
                let at = jiff::Timestamp::from_second(unix).unwrap();
                (zone.to_offset(at).seconds() / 60) as i16
            };
            let mut unix = FROM;
            while unix < UNTIL {
                let before = theirs(unix);
                assert_eq!(rule.offset_at(unix), before, "{text} at {unix}");
                // Around a change, every second.
                if theirs(unix + 3_600) != before {
                    for second in unix..unix + 3_600 {
                        assert_eq!(rule.offset_at(second), theirs(second), "{text} at {second}");
                    }
                }
                unix += 3_600;
            }
        }
    }

    /// RFC 8536's daylight time all year. jiff keeps standard time for
    /// the first hours of each UTC year here, where the RFC keeps none.
    #[test]
    fn daylight_time_all_year_never_lapses() {
        let rule = Rule::parse("EST5EDT,0/0,J365/25").unwrap();
        let mut unix = FROM;
        while unix < UNTIL {
            assert_eq!(rule.offset_at(unix), -240, "at {unix}");
            unix += 1_800;
        }
    }

    #[test]
    fn reads_offsets_east_and_names() {
        let rule = Rule::parse("PST8PDT,M3.2.0,M11.1.0").unwrap();
        assert_eq!(rule.std_offset_min(), -480);
        assert_eq!(rule.dst_offset_min(), Some(-420));
        assert_eq!(rule.std_name(), "PST");
        assert_eq!(rule.dst_name(), Some("PDT"));
        let india = Rule::parse("<+0530>-5:30").unwrap();
        assert_eq!(india.std_offset_min(), 330);
        assert_eq!(india.std_name(), "+0530");
        assert_eq!(india.dst_offset_min(), None);
        assert_eq!(Rule::parse("UTC0"), Ok(Rule::UTC));
    }

    #[test]
    fn daylight_time_without_changes_follows_the_us_rules() {
        let short = Rule::parse("CST6CDT").unwrap();
        let long = Rule::parse("CST6CDT,M3.2.0/2,M11.1.0/2").unwrap();
        assert_eq!(short, long);
        assert_eq!(short.dst_offset_min(), Some(-300));
    }

    #[test]
    fn the_same_time_by_other_names_is_the_same() {
        let a = Rule::parse("EST5EDT,M3.2.0,M11.1.0").unwrap();
        let b = Rule::parse("<-05>5<-04>,M3.2.0/2:00,M11.1.0/2").unwrap();
        assert!(a.same_time_as(&b));
        assert_ne!(a, b);
        let c = Rule::parse("EST5EDT,M3.2.0,M11.1.0/3").unwrap();
        assert!(!a.same_time_as(&c));
        assert!(!Rule::parse("EST5").unwrap().same_time_as(&a));
    }

    #[test]
    fn refuses_what_is_not_a_rule() {
        for text in [
            "",
            "PST",
            "PS8",
            "PST8PDT,M3.2.0",
            "PST8PDT,M13.2.0,M11.1.0",
            "PST8PDT,M3.6.0,M11.1.0",
            "PST8PDT,M3.2.7,M11.1.0",
            "PST8PDT,J0,M11.1.0",
            "PST8PDT,366,M11.1.0",
            "PST8PDT,M3.2.0/168,M11.1.0",
            "PST25",
            "PST8:60",
            "PST8:00:30",
            "<+05",
            "<TOOLONGNAME>5",
            "PST8 ",
        ] {
            assert_eq!(Rule::parse(text), Err(Invalid), "{text:?}");
        }
    }

    #[test]
    fn a_local_reading_names_the_earlier_instant_or_the_one_before_a_gap() {
        let rule = Rule::parse("PST8PDT,M3.2.0,M11.1.0").unwrap();
        // 2026-11-01 01:30 happens twice: first in PDT.
        let twice = days_from_civil(2026, 11, 1) * 86_400 + 90 * 60;
        assert_eq!(rule.to_utc(twice), twice + 7 * 3_600);
        // 2026-03-08 02:30 never happens: read in PST, it is 03:30 PDT.
        let skipped = days_from_civil(2026, 3, 8) * 86_400 + 150 * 60;
        assert_eq!(rule.to_utc(skipped), skipped + 8 * 3_600);
        // An ordinary summer afternoon.
        let summer = days_from_civil(2026, 7, 4) * 86_400 + 14 * 3_600;
        assert_eq!(rule.to_utc(summer), summer + 7 * 3_600);
    }

    #[test]
    fn a_tzif_file_ends_with_its_rule() {
        let mut tzif = b"TZif2".to_vec();
        tzif.extend_from_slice(b"\0\0\n\0data\n<+0530>-5:30\n");
        assert_eq!(tzif_rule(&tzif), Some("<+0530>-5:30"));
        // Version 1 carries no rule, and an empty footer is none.
        assert_eq!(tzif_rule(b"TZif\0data\nUTC0\n"), None);
        assert_eq!(tzif_rule(b"TZif2data\n\n"), None);
        assert_eq!(tzif_rule(b"not a tzif\nUTC0\n"), None);
    }

    #[test]
    fn days_and_dates_agree() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2026, 10, 19), 20_745);
        for days in -800_000..800_000 {
            let (y, m, d) = civil(days);
            assert_eq!(days_from_civil(y, m, d), days);
        }
    }
}
