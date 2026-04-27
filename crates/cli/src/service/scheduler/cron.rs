//! 5-field UTC cron parser, hand-rolled to avoid pulling in a full
//! cron crate for the small subset §13 Q3 actually requires.
//!
//! Supports: `*`, `*/N`, `M-N`, `M-N/S`, comma-separated lists, and bare
//! integers. Field ranges follow POSIX cron:
//!
//! * minute       0-59
//! * hour         0-23
//! * day-of-month 1-31
//! * month        1-12
//! * day-of-week  0-6, Sunday = 0 (alias 7 = 0 also accepted)
//!
//! No seconds, no named months/weekdays, no `L`/`W`/`#` extensions, no
//! timezone — operators that need timezone support convert manually.
//!
//! `Schedule::next_after(now)` returns the first UTC fire time strictly
//! after `now`. The DoM/DoW interaction follows POSIX (and §13 Q3):
//! when both fields are restricted (not `*`), a day matches if **either**
//! the DoM or the DoW field matches; when one is `*`, only the other
//! field is consulted.

use std::collections::BTreeSet;
use std::fmt;

use chrono::{
    DateTime, Datelike, Duration as ChronoDuration, NaiveDate, TimeZone, Timelike, Utc, Weekday,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schedule {
    minute: Field,
    hour: Field,
    day_of_month: Field,
    month: Field,
    day_of_week: Field,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Field {
    /// Sorted set of accepted values inside the field's range.
    values: BTreeSet<u32>,
    /// True when the source spec was a bare `*` (no narrowing). Used
    /// for the §13 Q3 DoM/DoW intersection rule.
    is_star: bool,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CronError {
    #[error("expected 5 fields, got {0}")]
    WrongFieldCount(usize),
    #[error("field {field}: empty token")]
    EmptyToken { field: &'static str },
    #[error("field {field}: invalid value `{value}` (range {min}-{max})")]
    OutOfRange {
        field: &'static str,
        value: String,
        min: u32,
        max: u32,
    },
    #[error("field {field}: malformed token `{value}`")]
    Malformed { field: &'static str, value: String },
}

impl Schedule {
    pub fn parse(expr: &str) -> Result<Self, CronError> {
        let parts: Vec<&str> = expr.split_ascii_whitespace().collect();
        if parts.len() != 5 {
            return Err(CronError::WrongFieldCount(parts.len()));
        }
        Ok(Self {
            minute: Field::parse(parts[0], "minute", 0, 59, false)?,
            hour: Field::parse(parts[1], "hour", 0, 23, false)?,
            day_of_month: Field::parse(parts[2], "day_of_month", 1, 31, false)?,
            month: Field::parse(parts[3], "month", 1, 12, false)?,
            day_of_week: Field::parse(parts[4], "day_of_week", 0, 6, true)?,
        })
    }

    /// First fire time strictly later than `from`. Returns `None` only
    /// if no match exists within ~5 years (the parser allows specs like
    /// `0 0 31 2 *` that never fire — caller decides what to do).
    pub fn next_after(&self, from: DateTime<Utc>) -> Option<DateTime<Utc>> {
        // Round up to the next whole minute, then iterate. The 5-year
        // bound covers leap-year quirks for rare specs like `0 0 29 2 *`
        // (matches once every 4 years on Feb 29) — anything looser than
        // that is a typo, not a real schedule.
        let mut t = from
            .with_second(0)
            .and_then(|d| d.with_nanosecond(0))
            .unwrap_or(from)
            + ChronoDuration::minutes(1);
        let limit = from + ChronoDuration::days(5 * 366);
        while t < limit {
            // Month: jump to first day of next valid month.
            if !self.month.contains(t.month()) {
                t = advance_to_next_month(t)?;
                continue;
            }
            // Day: combine DoM and DoW per §13 Q3 / POSIX.
            if !self.day_matches(t) {
                t = next_day_midnight(t)?;
                continue;
            }
            // Hour.
            if !self.hour.contains(t.hour()) {
                t = next_hour_top(t)?;
                continue;
            }
            // Minute.
            if !self.minute.contains(t.minute()) {
                t += ChronoDuration::minutes(1);
                continue;
            }
            return Some(t);
        }
        None
    }

    fn day_matches(&self, t: DateTime<Utc>) -> bool {
        let dom = t.day();
        let dow = weekday_to_cron(t.weekday());
        match (self.day_of_month.is_star, self.day_of_week.is_star) {
            (true, true) => true,
            (false, true) => self.day_of_month.contains(dom),
            (true, false) => self.day_of_week.contains(dow),
            // POSIX: both restricted → OR.
            (false, false) => self.day_of_month.contains(dom) || self.day_of_week.contains(dow),
        }
    }
}

impl Field {
    fn parse(
        spec: &str,
        name: &'static str,
        min: u32,
        max: u32,
        is_dow: bool,
    ) -> Result<Self, CronError> {
        if spec.is_empty() {
            return Err(CronError::EmptyToken { field: name });
        }
        let mut values = BTreeSet::new();
        let mut star_only = true;
        for token in spec.split(',') {
            let token = token.trim();
            if token.is_empty() {
                return Err(CronError::EmptyToken { field: name });
            }
            if token != "*" && !token.starts_with("*/") {
                star_only = false;
            }
            let (range_part, step) = match token.split_once('/') {
                Some((r, s)) => {
                    let step: u32 = s.parse().map_err(|_| CronError::Malformed {
                        field: name,
                        value: token.into(),
                    })?;
                    if step == 0 {
                        return Err(CronError::Malformed {
                            field: name,
                            value: token.into(),
                        });
                    }
                    (r, step)
                }
                None => (token, 1),
            };
            let (lo, hi) = if range_part == "*" {
                (min, max)
            } else if let Some((a, b)) = range_part.split_once('-') {
                let a = parse_value(a, name, min, max, is_dow)?;
                let b = parse_value(b, name, min, max, is_dow)?;
                if a > b {
                    return Err(CronError::Malformed {
                        field: name,
                        value: token.into(),
                    });
                }
                (a, b)
            } else {
                let v = parse_value(range_part, name, min, max, is_dow)?;
                (v, v)
            };
            let mut v = lo;
            while v <= hi {
                values.insert(v);
                v += step;
            }
        }
        if values.is_empty() {
            return Err(CronError::Malformed {
                field: name,
                value: spec.into(),
            });
        }
        Ok(Self {
            values,
            is_star: star_only,
        })
    }

    fn contains(&self, v: u32) -> bool {
        self.values.contains(&v)
    }
}

fn parse_value(
    s: &str,
    field: &'static str,
    min: u32,
    max: u32,
    is_dow: bool,
) -> Result<u32, CronError> {
    let n: u32 = s.parse().map_err(|_| CronError::Malformed {
        field,
        value: s.into(),
    })?;
    // POSIX cron accepts `7` as Sunday alias for DoW.
    let n = if is_dow && n == 7 { 0 } else { n };
    if n < min || n > max {
        return Err(CronError::OutOfRange {
            field,
            value: s.into(),
            min,
            max,
        });
    }
    Ok(n)
}

fn weekday_to_cron(w: Weekday) -> u32 {
    // chrono::Weekday::Sun.num_days_from_sunday() == 0
    w.num_days_from_sunday()
}

fn advance_to_next_month(t: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let (y, m) = if t.month() == 12 {
        (t.year() + 1, 1)
    } else {
        (t.year(), t.month() + 1)
    };
    let nd = NaiveDate::from_ymd_opt(y, m, 1)?;
    Utc.from_local_datetime(&nd.and_hms_opt(0, 0, 0)?).single()
}

fn next_day_midnight(t: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let next_day = t.date_naive().succ_opt()?;
    Utc.from_local_datetime(&next_day.and_hms_opt(0, 0, 0)?)
        .single()
}

fn next_hour_top(t: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let plus = t + ChronoDuration::hours(1);
    plus.with_minute(0).and_then(|d| d.with_second(0))
}

impl fmt::Display for Schedule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} {} {} {}",
            self.minute, self.hour, self.day_of_month, self.month, self.day_of_week
        )
    }
}

impl fmt::Display for Field {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_star {
            f.write_str("*")
        } else {
            let s: Vec<String> = self.values.iter().map(u32::to_string).collect();
            f.write_str(&s.join(","))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn dt(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()
    }

    #[test]
    fn parses_basic_patterns() {
        Schedule::parse("* * * * *").unwrap();
        Schedule::parse("*/5 * * * *").unwrap();
        Schedule::parse("0 9 * * 1-5").unwrap();
        Schedule::parse("0,15,30,45 * * * *").unwrap();
        Schedule::parse("0 0 1 1 *").unwrap();
    }

    #[test]
    fn rejects_garbage() {
        assert!(matches!(
            Schedule::parse("not a cron"),
            Err(CronError::WrongFieldCount(_))
        ));
        assert!(matches!(
            Schedule::parse("60 * * * *"),
            Err(CronError::OutOfRange { .. })
        ));
        assert!(matches!(
            Schedule::parse("* 24 * * *"),
            Err(CronError::OutOfRange { .. })
        ));
        assert!(matches!(
            Schedule::parse("* * 0 * *"),
            Err(CronError::OutOfRange { .. })
        ));
        assert!(matches!(
            Schedule::parse("* * * 13 *"),
            Err(CronError::OutOfRange { .. })
        ));
        assert!(matches!(
            Schedule::parse("* * * * 8"),
            Err(CronError::OutOfRange { .. })
        ));
        assert!(matches!(
            Schedule::parse("*/0 * * * *"),
            Err(CronError::Malformed { .. })
        ));
    }

    #[test]
    fn dow_seven_aliases_sunday() {
        // POSIX accepts both 0 and 7 for Sunday; we should treat them
        // identically when computing matches.
        let s7 = Schedule::parse("0 0 * * 7").unwrap();
        let s0 = Schedule::parse("0 0 * * 0").unwrap();
        let from = dt(2026, 4, 21, 0, 0); // Tuesday
        assert_eq!(s7.next_after(from), s0.next_after(from));
    }

    #[test]
    fn next_after_basic_minute_advance() {
        let s = Schedule::parse("*/15 * * * *").unwrap();
        // 09:07 → next is 09:15.
        assert_eq!(
            s.next_after(dt(2026, 4, 24, 9, 7)),
            Some(dt(2026, 4, 24, 9, 15))
        );
        // Boundary: 09:15 → 09:30 (strictly after).
        assert_eq!(
            s.next_after(dt(2026, 4, 24, 9, 15)),
            Some(dt(2026, 4, 24, 9, 30))
        );
    }

    #[test]
    fn next_after_rolls_hour_and_day() {
        let s = Schedule::parse("0 9 * * *").unwrap();
        // 09:00:01 (within the same minute as the fire) — strictly later
        // means tomorrow 09:00.
        let from = dt(2026, 4, 24, 9, 0).with_second(1).unwrap();
        assert_eq!(s.next_after(from), Some(dt(2026, 4, 25, 9, 0)));
    }

    #[test]
    fn next_after_dow_only() {
        // Mondays at 09:00.
        let s = Schedule::parse("0 9 * * 1").unwrap();
        // From Tuesday 2026-04-21 → next is Monday 2026-04-27 09:00.
        assert_eq!(
            s.next_after(dt(2026, 4, 21, 12, 0)),
            Some(dt(2026, 4, 27, 9, 0))
        );
    }

    #[test]
    fn next_after_dom_only() {
        // 1st of month at midnight UTC.
        let s = Schedule::parse("0 0 1 * *").unwrap();
        assert_eq!(
            s.next_after(dt(2026, 4, 24, 12, 0)),
            Some(dt(2026, 5, 1, 0, 0))
        );
    }

    #[test]
    fn dom_dow_both_restricted_uses_or() {
        // POSIX rule: 1st of month OR Mondays. From a Tuesday the 21st →
        // first Monday is 27th, 1st of next month is May 1st (Friday).
        // Earlier of the two = Monday 27th.
        let s = Schedule::parse("0 9 1 * 1").unwrap();
        assert_eq!(
            s.next_after(dt(2026, 4, 21, 12, 0)),
            Some(dt(2026, 4, 27, 9, 0))
        );
    }

    #[test]
    fn next_after_year_boundary() {
        // 23:59 Dec 31 → 00:00 Jan 1.
        let s = Schedule::parse("* * * * *").unwrap();
        assert_eq!(
            s.next_after(dt(2026, 12, 31, 23, 59)),
            Some(dt(2027, 1, 1, 0, 0))
        );
    }

    #[test]
    fn leap_day_february_29() {
        // 2028 is a leap year. From 2026-04-01 → next is 2028-02-29 00:00.
        let s = Schedule::parse("0 0 29 2 *").unwrap();
        assert_eq!(
            s.next_after(dt(2026, 4, 1, 0, 0)),
            Some(dt(2028, 2, 29, 0, 0))
        );
    }

    #[test]
    fn ranges_and_steps_compose() {
        // 9, 12, 15, 18 (every 3 hours from 9 to 18, top of hour).
        let s = Schedule::parse("0 9-18/3 * * *").unwrap();
        assert_eq!(
            s.next_after(dt(2026, 4, 24, 8, 0)),
            Some(dt(2026, 4, 24, 9, 0))
        );
        assert_eq!(
            s.next_after(dt(2026, 4, 24, 9, 0)),
            Some(dt(2026, 4, 24, 12, 0))
        );
        assert_eq!(
            s.next_after(dt(2026, 4, 24, 18, 0)),
            Some(dt(2026, 4, 25, 9, 0))
        );
    }

    #[test]
    fn list_field() {
        let s = Schedule::parse("0,30 * * * *").unwrap();
        assert_eq!(
            s.next_after(dt(2026, 4, 24, 10, 0)),
            Some(dt(2026, 4, 24, 10, 30))
        );
        assert_eq!(
            s.next_after(dt(2026, 4, 24, 10, 30)),
            Some(dt(2026, 4, 24, 11, 0))
        );
    }

    #[test]
    fn display_round_trip_for_star() {
        let s = Schedule::parse("* * * * *").unwrap();
        assert_eq!(s.to_string(), "* * * * *");
    }
}
