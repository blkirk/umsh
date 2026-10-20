//! Days and times as the screens write them, in local time.
//!
//! Nothing here knows what time it is: callers pass a time only once the
//! board's clock is known, and show nothing otherwise.

use heapless::String;

pub(crate) const WEEKDAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

pub(crate) const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

const MONTH_NAMES: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// A moment in local time: the day since 1970-01-01 and the minute of
/// that day.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Local {
    pub day: i32,
    pub minute: u16,
}

impl Local {
    /// `time`, in Unix seconds, `offset_min` minutes east of UTC.
    #[must_use]
    pub fn at(time: u32, offset_min: i16) -> Self {
        let local = i64::from(time) + i64::from(offset_min) * 60;
        Self {
            day: local.div_euclid(86_400) as i32,
            minute: (local.rem_euclid(86_400) / 60) as u16,
        }
    }

    /// The hours and minutes, such as `14:32`, or `2:32` and `PM` on a
    /// 12-hour clock.
    #[must_use]
    pub fn face(self, h24: bool) -> (String<5>, Option<&'static str>) {
        let (hour, minute) = (self.minute / 60, self.minute % 60);
        let mut face = String::new();
        if h24 {
            _ = core::fmt::write(&mut face, format_args!("{hour:02}:{minute:02}"));
            return (face, None);
        }
        let shown = match hour % 12 {
            0 => 12,
            h => h,
        };
        _ = core::fmt::write(&mut face, format_args!("{shown}:{minute:02}"));
        (face, Some(if hour < 12 { "AM" } else { "PM" }))
    }

    /// The time of day in one piece, such as `14:32` or `2:32 PM`.
    #[must_use]
    pub fn time(self, h24: bool) -> String<8> {
        let (face, half) = self.face(h24);
        let mut time = String::new();
        _ = time.push_str(&face);
        if let Some(half) = half {
            _ = time.push(' ');
            _ = time.push_str(half);
        }
        time
    }

    /// The month and day, such as `Oct 19`.
    #[must_use]
    pub fn short_date(self) -> String<8> {
        let (_, month, date) = civil(i64::from(self.day));
        let mut text = String::new();
        _ = core::fmt::write(
            &mut text,
            format_args!("{} {date}", MONTHS[month as usize - 1]),
        );
        text
    }

    /// The weekday and date written out, such as `Sunday, October 19`.
    #[must_use]
    pub fn long_date(self) -> String<32> {
        let (_, month, date) = civil(i64::from(self.day));
        let mut text = String::new();
        _ = core::fmt::write(
            &mut text,
            format_args!(
                "{}, {} {date}",
                WEEKDAYS[(self.day + 4).rem_euclid(7) as usize],
                MONTH_NAMES[month as usize - 1]
            ),
        );
        text
    }
}

pub use umsh_hal::tz::civil;

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-10-19 14:32 UTC.
    const AFTERNOON: u32 = 1_792_420_320;

    #[test]
    fn writes_the_time_on_either_clock() {
        let local = Local::at(AFTERNOON, 0);
        assert_eq!(local.time(true).as_str(), "14:32");
        assert_eq!(local.time(false).as_str(), "2:32 PM");
        let midnight = Local { day: 0, minute: 5 };
        assert_eq!(midnight.time(false).as_str(), "12:05 AM");
        let noon = Local {
            day: 0,
            minute: 12 * 60,
        };
        assert_eq!(noon.time(false).as_str(), "12:00 PM");
    }

    #[test]
    fn writes_the_date_in_local_time() {
        assert_eq!(
            Local::at(AFTERNOON, 0).long_date().as_str(),
            "Monday, October 19"
        );
        // Seven hours west, still the same day; fifteen east, the next.
        assert_eq!(
            Local::at(AFTERNOON, -7 * 60).long_date().as_str(),
            "Monday, October 19"
        );
        assert_eq!(
            Local::at(AFTERNOON, 14 * 60).long_date().as_str(),
            "Tuesday, October 20"
        );
    }
}
