//! `archive/zip_time.rs` (SPEC-0300-0014 R-327-11, R-346-2): conversion between
//! `SystemTime` and the ZIP (MS-DOS) date/time.
//!
//! Convention: the DOS fields are LOCAL civil time (what Explorer / 7-Zip read
//! and write). The local UTC offset comes from the `time` crate
//! (`UtcOffset::local_offset_at`); when it cannot be obtained (the crate refuses
//! in some multi-threaded situations) the offset is 0, i.e. UTC. The offset
//! lookup is injectable (`*_with`) so both paths are testable. A local time that
//! does not map to exactly one instant (a DST gap or overlap) is read as UTC.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// 1980-01-01 00:00:00 UTC, the DOS epoch.
const MIN_SECS: i64 = 315_532_800;

/// Days since 1970-01-01 for a proleptic Gregorian date (H. Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Inverse of `days_from_civil`.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Latest representable DOS time: 2107-12-31 23:59:58.
fn max_secs() -> i64 {
    days_from_civil(2107, 12, 31) * 86_400 + 86_398
}

/// Local UTC offset in seconds at a Unix time (`None` if unavailable).
pub type OffsetLookup<'a> = &'a dyn Fn(i64) -> Option<i32>;

/// The real lookup: the OS local offset at that instant via the `time` crate.
pub fn local_offset_secs(unix_secs: i64) -> Option<i32> {
    let instant = time::OffsetDateTime::from_unix_timestamp(unix_secs).ok()?;
    time::UtcOffset::local_offset_at(instant).ok().map(|offset| offset.whole_seconds())
}

fn unix_secs_of(time: SystemTime) -> i64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(after) => i64::try_from(after.as_secs()).unwrap_or(i64::MAX),
        Err(before) => -i64::try_from(before.duration().as_secs()).unwrap_or(i64::MAX),
    }
}

/// Converts a modification time to the ZIP field (local civil time), clamping
/// times outside 1980..=2107 (R-327-11: never an error).
pub fn system_time_to_zip(time: SystemTime) -> Option<zip::DateTime> {
    system_time_to_zip_with(time, &local_offset_secs)
}

/// `system_time_to_zip` with an explicit offset lookup (unavailable -> UTC).
pub fn system_time_to_zip_with(time: SystemTime, offset: OffsetLookup) -> Option<zip::DateTime> {
    let utc = unix_secs_of(time);
    let shift = i64::from(offset(utc).unwrap_or(0));
    let secs = utc.saturating_add(shift).clamp(MIN_SECS, max_secs());
    let (year, month, day) = civil_from_days(secs.div_euclid(86_400));
    let rest = secs.rem_euclid(86_400);
    zip::DateTime::from_date_and_time(
        u16::try_from(year).ok()?,
        month as u8,
        day as u8,
        (rest / 3600) as u8,
        (rest % 3600 / 60) as u8,
        (rest % 60) as u8,
    )
    .ok()
}

/// Converts the ZIP field (local civil time) to a `SystemTime`; `None` for an
/// invalid date such as the all-zero field (R-346-2: no restore attempt).
pub fn zip_to_system_time(dt: &zip::DateTime) -> Option<SystemTime> {
    zip_to_system_time_with(dt, &local_offset_secs)
}

/// `zip_to_system_time` with an explicit offset lookup. The instant is found by
/// guessing the offset at the civil time read as UTC, then confirming it at the
/// resulting instant; if the two disagree (a DST boundary) or the lookup fails,
/// the field is read as UTC (never an error).
pub fn zip_to_system_time_with(dt: &zip::DateTime, offset: OffsetLookup) -> Option<SystemTime> {
    if !dt.is_valid() {
        return None;
    }
    let days = days_from_civil(i64::from(dt.year()), i64::from(dt.month()), i64::from(dt.day()));
    let civil = days * 86_400 + i64::from(dt.hour()) * 3600 + i64::from(dt.minute()) * 60 + i64::from(dt.second());
    let utc = match offset(civil) {
        Some(guess) => {
            let candidate = civil - i64::from(guess);
            if offset(candidate) == Some(guess) {
                candidate
            } else {
                civil
            }
        }
        None => civil,
    };
    u64::try_from(utc).ok().map(|s| UNIX_EPOCH + Duration::from_secs(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLUS9: i32 = 9 * 3600;

    fn fields(dt: &zip::DateTime) -> (u16, u8, u8, u8, u8, u8) {
        (dt.year(), dt.month(), dt.day(), dt.hour(), dt.minute(), dt.second())
    }

    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs)
    }

    #[test]
    fn round_trips_an_ordinary_time_with_the_real_local_offset() {
        let t = at(1_577_934_246);
        let dt = system_time_to_zip(t).expect("dos time");
        assert_eq!(zip_to_system_time(&dt), Some(t));
    }

    // Interoperability: the local civil time is what lands in the DOS fields.
    #[test]
    fn writes_the_local_civil_time_for_a_fixed_offset() {
        let t = at(1_577_934_246); // 2020-01-02 03:04:06 UTC
        let dt = system_time_to_zip_with(t, &|_| Some(PLUS9)).expect("dos time");
        assert_eq!(fields(&dt), (2020, 1, 2, 12, 4, 6), "+09:00 -> 12:04:06 local");
        let west = system_time_to_zip_with(t, &|_| Some(-5 * 3600)).expect("dos time");
        assert_eq!(fields(&west), (2020, 1, 1, 22, 4, 6), "-05:00 crosses midnight backwards");
    }

    #[test]
    fn reads_the_dos_fields_as_local_time_for_a_fixed_offset() {
        let dt = zip::DateTime::from_date_and_time(2020, 1, 2, 12, 4, 6).unwrap();
        assert_eq!(zip_to_system_time_with(&dt, &|_| Some(PLUS9)), Some(at(1_577_934_246)));
    }

    #[test]
    fn a_fixed_offset_round_trips() {
        for secs in [315_532_800u64, 1_577_934_246, 1_700_000_000, 4_102_444_800] {
            let secs = secs & !1; // DOS has 2-second resolution
            let dt = system_time_to_zip_with(at(secs), &|_| Some(PLUS9)).expect("dos time");
            assert_eq!(zip_to_system_time_with(&dt, &|_| Some(PLUS9)), Some(at(secs)), "{secs}");
        }
    }

    // Offset lookup failure (thread restriction of the `time` crate) -> UTC.
    #[test]
    fn falls_back_to_utc_when_the_offset_is_unavailable() {
        let t = at(1_577_934_246);
        let dt = system_time_to_zip_with(t, &|_| None).expect("dos time");
        assert_eq!(fields(&dt), (2020, 1, 2, 3, 4, 6));
        assert_eq!(zip_to_system_time_with(&dt, &|_| None), Some(t));
    }

    // A DST boundary: the offset at the guess and at the result differ -> UTC reading.
    #[test]
    fn a_dst_boundary_falls_back_to_the_utc_reading() {
        // Offset +1h before the instant 1_000_000_000, +2h from it on.
        let lookup = |unix: i64| Some(if unix < 1_000_000_000 { 3600 } else { 7200 });
        // Civil 2001-09-09 01:46:40 read as UTC is 1_000_000_000 -> guess +2h;
        // civil - 2h lies before the switch, where the offset is +1h: inconsistent.
        let dt = zip::DateTime::from_date_and_time(2001, 9, 9, 1, 46, 40).unwrap();
        assert_eq!(zip_to_system_time_with(&dt, &lookup), Some(at(1_000_000_000)));
    }

    #[test]
    fn clamps_out_of_range_times() {
        let low = system_time_to_zip_with(UNIX_EPOCH, &|_| Some(PLUS9)).expect("dos time");
        assert_eq!((low.year(), low.month(), low.day()), (1980, 1, 1));
        let high = system_time_to_zip_with(at(7_258_118_400), &|_| Some(PLUS9)).expect("dos time");
        assert_eq!((high.year(), high.month(), high.day()), (2107, 12, 31));
        assert!(system_time_to_zip(UNIX_EPOCH).is_some());
        assert!(system_time_to_zip(at(1 << 38)).is_some());
    }
}
