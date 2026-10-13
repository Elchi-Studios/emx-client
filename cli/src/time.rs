// SPDX-License-Identifier: MPL-2.0
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Times as `emx` takes them on the command line: RFC 3339 with its zone,
//! or a while from now. Written here, without a time library, because a
//! command line needs only this much: civil dates to and from seconds
//! since 1970, in UTC.

/// Seconds since 1970 now.
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// A time for `--at`, as RFC 3339 for the service, and as seconds since
/// 1970: `2026-10-14T07:00:00+02:00` (a space for the `T` and no seconds
/// will do; a zone, `Z` or an offset, must be given, since the
/// computer's own is not known here), or a while from now, `+30m`,
/// `+2h`, `+1d`. It must be in the future and within a year, as the
/// service holds a message no longer.
pub fn parse_at(s: &str, now: i64) -> Result<(String, i64), String> {
    let s = s.trim();
    let at = if let Some(rest) = s.strip_prefix('+') {
        let (n, unit) = rest.split_at(rest.len().saturating_sub(1));
        let n: i64 = n
            .parse()
            .map_err(|_| format!("--at {s}: a while from now is a number and m, h or d, as +90m"))?;
        let secs = match unit {
            "m" => 60,
            "h" => 3600,
            "d" => 86_400,
            _ => return Err(format!("--at {s}: a while from now ends in m, h or d, as +2h")),
        };
        now + n.checked_mul(secs).ok_or_else(|| format!("--at {s}: too far"))?
    } else {
        parse_rfc3339(s)?
    };
    if at <= now {
        return Err(format!("--at {s}: that time is past"));
    }
    if at > now + 365 * 86_400 {
        return Err(format!("--at {s}: a message is held for a year at most"));
    }
    Ok((utc(at), at))
}

/// Seconds since 1970 of an RFC 3339 time with its zone. Seconds and
/// their fraction may be left out; the fraction is dropped.
pub fn parse_rfc3339(s: &str) -> Result<i64, String> {
    let bad = || {
        format!("{s}: give a time with its zone, as 2026-10-14T07:00:00+02:00 or 2026-10-14 05:00Z, or +2h")
    };
    let b = s.as_bytes();
    let num = |from: usize, len: usize| -> Option<i64> {
        let part = s.get(from..from + len)?;
        part.bytes()
            .all(|c| c.is_ascii_digit())
            .then(|| part.parse().ok())?
    };
    if b.len() < 17 || b[4] != b'-' || b[7] != b'-' || !matches!(b[10], b'T' | b't' | b' ') || b[13] != b':' {
        return Err(bad());
    }
    let (y, mo, d, h, mi) = (
        num(0, 4).ok_or_else(bad)?,
        num(5, 2).ok_or_else(bad)?,
        num(8, 2).ok_or_else(bad)?,
        num(11, 2).ok_or_else(bad)?,
        num(14, 2).ok_or_else(bad)?,
    );
    let mut rest = &s[16..];
    let mut sec = 0;
    if let Some(r) = rest.strip_prefix(':') {
        sec = r
            .get(..2)
            .filter(|p| p.bytes().all(|c| c.is_ascii_digit()))
            .and_then(|p| p.parse().ok())
            .ok_or_else(bad)?;
        rest = &r[2..];
        if let Some(r) = rest.strip_prefix('.') {
            let digits = r.bytes().take_while(|c| c.is_ascii_digit()).count();
            if digits == 0 {
                return Err(bad());
            }
            rest = &r[digits..];
        }
    }
    let offset = match rest {
        "Z" | "z" => 0,
        r if (r.starts_with('+') || r.starts_with('-')) && r.len() == 6 && r.as_bytes()[3] == b':' => {
            let oh: i64 = r[1..3].parse().map_err(|_| bad())?;
            let om: i64 = r[4..6].parse().map_err(|_| bad())?;
            if oh > 23 || om > 59 {
                return Err(bad());
            }
            let o = oh * 3600 + om * 60;
            if r.starts_with('-') {
                -o
            } else {
                o
            }
        }
        _ => return Err(bad()),
    };
    if !(1..=12).contains(&mo) || d < 1 || d > days_in_month(y, mo) || h > 23 || mi > 59 || sec > 60 {
        return Err(bad());
    }
    Ok(days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + sec.min(59) - offset)
}

/// RFC 3339 in UTC, to the second.
pub fn utc(t: i64) -> String {
    let (days, secs) = (t.div_euclid(86_400), t.rem_euclid(86_400));
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        secs / 3600,
        secs % 3600 / 60,
        secs % 60
    )
}

fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        4 | 6 | 9 | 11 => 30,
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        _ => 31,
    }
}

/// Days since 1970-01-01 of a date in the proleptic Gregorian calendar.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The date of a day counted from 1970-01-01.
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_go_to_seconds_and_back() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(parse_rfc3339("2026-10-13T00:00:00Z").unwrap(), 1_791_849_600);
        for t in [0, 951_782_400, 1_791_849_600, 4_107_542_399, -86_400] {
            assert_eq!(parse_rfc3339(&utc(t)).unwrap(), t, "{}", utc(t));
        }
        assert_eq!(utc(951_782_400), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn times_with_their_zone() {
        let at = parse_rfc3339("2026-10-14T07:00:00+02:00").unwrap();
        assert_eq!(utc(at), "2026-10-14T05:00:00Z");
        assert_eq!(parse_rfc3339("2026-10-14 05:00Z").unwrap(), at);
        assert_eq!(parse_rfc3339("2026-10-14t05:00:00.123z").unwrap(), at);
        assert_eq!(parse_rfc3339("2026-10-13T23:00:00-06:00").unwrap(), at);
        for bad in [
            "2026-10-14T07:00:00",
            "2026-10-14",
            "2026-02-30T07:00Z",
            "2026-13-01T07:00Z",
            "2026-10-14T24:00Z",
            "2026-10-14T07:00:00+2:00",
            "2026-10-14T07:00:00.Z",
            "tomorrow",
            "2026-1O-14T07:00Z",
            "2026-10-14T07:00:+5Z",
        ] {
            assert!(parse_rfc3339(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_time_to_send_at() {
        let now = parse_rfc3339("2026-10-13T08:00:00Z").unwrap();
        assert_eq!(parse_at("+90m", now).unwrap().0, "2026-10-13T09:30:00Z");
        assert_eq!(parse_at("+2h", now).unwrap().0, "2026-10-13T10:00:00Z");
        assert_eq!(parse_at("+1d", now).unwrap().0, "2026-10-14T08:00:00Z");
        assert_eq!(
            parse_at("2026-10-14T07:00:00+02:00", now).unwrap().0,
            "2026-10-14T05:00:00Z"
        );
        assert!(parse_at("+0m", now).unwrap_err().contains("past"));
        assert!(parse_at("2026-10-13T07:00Z", now).unwrap_err().contains("past"));
        assert!(parse_at("+366d", now).unwrap_err().contains("a year"));
        assert!(parse_at("+2w", now).is_err());
        assert!(parse_at("+h", now).is_err());
        assert!(parse_at("+99999999999999999d", now).is_err());
    }
}
