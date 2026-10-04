//! Local date and time of the device, for when an expense happened
//! (EXP-01). Stored as local time with its UTC offset (migration 0001).

use invuso_core::domain::is_iso_date;
use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time, UtcOffset};

/// Today and the current minute in the device's time zone, as `YYYY-MM-DD`
/// and `HH:MM` (what `type="date"` and `type="time"` inputs use). Falls
/// back to UTC if the time zone cannot be read.
pub fn local_now() -> (String, String) {
    let now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
    let date = now.date();
    (
        format!(
            "{:04}-{:02}-{:02}",
            date.year(),
            u8::from(date.month()),
            date.day()
        ),
        format!("{:02}:{:02}", now.hour(), now.minute()),
    )
}

/// `occurred_at` for a local date (`YYYY-MM-DD`) and time (`HH:MM`), e.g.
/// `2026-10-04T19:30:00+09:00`, with the offset the device's time zone has
/// at that moment, so a date across a daylight-saving change still gets
/// the right one. `None` for an invalid date or time.
pub fn occurred_at(date: &str, time: &str) -> Option<String> {
    let local = parse_local(date, time)?;
    // The offset depends on the instant, which depends on the offset:
    // guess with the local time read as UTC, then correct once.
    let guess = UtcOffset::local_offset_at(local.assume_utc()).unwrap_or(UtcOffset::UTC);
    let offset = UtcOffset::local_offset_at(local.assume_offset(guess)).unwrap_or(guess);
    Some(format_occurred_at(local, offset))
}

fn parse_local(date: &str, time: &str) -> Option<PrimitiveDateTime> {
    if !is_iso_date(date) {
        return None;
    }
    let year = date[0..4].parse().ok()?;
    let month = Month::try_from(date[5..7].parse::<u8>().ok()?).ok()?;
    let day = date[8..10].parse().ok()?;
    let date = Date::from_calendar_date(year, month, day).ok()?;
    Some(PrimitiveDateTime::new(date, parse_time(time)?))
}

fn parse_time(text: &str) -> Option<Time> {
    let (hour, minute) = text.split_once(':')?;
    if hour.len() != 2 || minute.len() != 2 {
        return None;
    }
    Time::from_hms(hour.parse().ok()?, minute.parse().ok()?, 0).ok()
}

fn format_occurred_at(local: PrimitiveDateTime, offset: UtcOffset) -> String {
    let date = local.date();
    let (hours, minutes, _) = offset.as_hms();
    let sign = if offset.is_negative() { '-' } else { '+' };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:00{sign}{:02}:{:02}",
        date.year(),
        u8::from(date.month()),
        date.day(),
        local.hour(),
        local.minute(),
        hours.unsigned_abs(),
        minutes.unsigned_abs()
    )
}

#[cfg(test)]
mod tests {
    use invuso_core::domain::validate_occurred_at;

    use super::*;

    #[test]
    fn formats_with_offset() {
        let local = parse_local("2026-10-04", "19:30").unwrap();
        let tokyo = UtcOffset::from_hms(9, 0, 0).unwrap();
        let newfoundland = UtcOffset::from_hms(-3, -30, 0).unwrap();
        assert_eq!(
            format_occurred_at(local, tokyo),
            "2026-10-04T19:30:00+09:00"
        );
        assert_eq!(
            format_occurred_at(local, newfoundland),
            "2026-10-04T19:30:00-03:30"
        );
        assert_eq!(
            format_occurred_at(local, UtcOffset::UTC),
            "2026-10-04T19:30:00+00:00"
        );
    }

    #[test]
    fn device_time_is_valid_occurred_at() {
        let (date, time) = local_now();
        assert!(parse_time(&time).is_some());
        let stamp = occurred_at(&date, &time).unwrap();
        assert_eq!(validate_occurred_at(&stamp), Ok(()));
        assert!(stamp.starts_with(&format!("{date}T{time}:00")));
    }

    #[test]
    fn rejects_invalid_input() {
        assert_eq!(occurred_at("2026-02-30", "12:00"), None);
        assert_eq!(occurred_at("2026-10-04", "24:00"), None);
        assert_eq!(occurred_at("2026-10-04", "9:30"), None);
        assert_eq!(occurred_at("", ""), None);
        assert_eq!(parse_time("12:60"), None);
    }
}
