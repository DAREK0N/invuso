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

/// The local day (`YYYY-MM-DD`) of a moment in Unix milliseconds, such as
/// a row's `created_at`; UTC if the time zone cannot be read.
pub fn local_date_of(unix_ms: i64) -> String {
    let utc = OffsetDateTime::from_unix_timestamp(unix_ms.div_euclid(1000))
        .unwrap_or(OffsetDateTime::UNIX_EPOCH);
    let date = UtcOffset::local_offset_at(utc)
        .map(|offset| utc.to_offset(offset))
        .unwrap_or(utc)
        .date();
    format!(
        "{:04}-{:02}-{:02}",
        date.year(),
        u8::from(date.month()),
        date.day()
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

/// Unix seconds of a valid `occurred_at`, so times recorded with different
/// UTC offsets (at home and on a trip) compare by when they happened.
/// `None` if it is not valid.
pub fn instant(occurred_at: &str) -> Option<i64> {
    invuso_core::domain::validate_occurred_at(occurred_at).ok()?;
    let local = parse_local(occurred_at.get(0..10)?, occurred_at.get(11..16)?)?;
    let second: u8 = occurred_at.get(17..19)?.parse().ok()?;
    let offset = match occurred_at.get(19..)? {
        "Z" => UtcOffset::UTC,
        text => {
            let sign: i8 = if text.starts_with('-') { -1 } else { 1 };
            let hours: i8 = text.get(1..3)?.parse().ok()?;
            let minutes: i8 = text.get(4..6)?.parse().ok()?;
            UtcOffset::from_hms(sign * hours, sign * minutes, 0).ok()?
        }
    };
    Some(local.assume_offset(offset).unix_timestamp() + i64::from(second))
}

/// Day of the week of a `YYYY-MM-DD` date, Monday = 0 … Sunday = 6.
pub fn weekday(date: &str) -> Option<u8> {
    Some(parse_date(date)?.weekday().number_days_from_monday())
}

/// The calendar day before a `YYYY-MM-DD` date, in the same form.
pub fn previous_day(date: &str) -> Option<String> {
    let day = parse_date(date)?.previous_day()?;
    Some(format!(
        "{:04}-{:02}-{:02}",
        day.year(),
        u8::from(day.month()),
        day.day()
    ))
}

fn parse_date(date: &str) -> Option<Date> {
    if !is_iso_date(date) {
        return None;
    }
    let year = date[0..4].parse().ok()?;
    let month = Month::try_from(date[5..7].parse::<u8>().ok()?).ok()?;
    let day = date[8..10].parse().ok()?;
    Date::from_calendar_date(year, month, day).ok()
}

fn parse_local(date: &str, time: &str) -> Option<PrimitiveDateTime> {
    Some(PrimitiveDateTime::new(parse_date(date)?, parse_time(time)?))
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
    fn local_date_of_a_moment() {
        // Noon UTC is the same day in every time zone up to ±11 hours.
        let noon = Date::from_calendar_date(2026, Month::October, 4)
            .unwrap()
            .with_hms(12, 0, 0)
            .unwrap()
            .assume_utc();
        let ms = noon.unix_timestamp() * 1000 + 999;
        assert_eq!(local_date_of(ms), "2026-10-04");
        assert!(is_iso_date(&local_date_of(0)));
    }

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
    fn weekday_and_previous_day() {
        // 2026-10-04 is a Sunday.
        assert_eq!(weekday("2026-10-04"), Some(6));
        assert_eq!(weekday("2026-10-05"), Some(0));
        assert_eq!(weekday("2026-13-01"), None);
        assert_eq!(previous_day("2026-10-04").as_deref(), Some("2026-10-03"));
        assert_eq!(previous_day("2026-03-01").as_deref(), Some("2026-02-28"));
        assert_eq!(previous_day("2024-03-01").as_deref(), Some("2024-02-29"));
        assert_eq!(previous_day("2026-01-01").as_deref(), Some("2025-12-31"));
        assert_eq!(previous_day("x"), None);
    }

    #[test]
    fn instants_compare_across_offsets() {
        // 08:13 in Berlin is after 12:30 in Tokyo on the same day.
        let berlin = instant("2026-10-06T08:13:00+02:00").unwrap();
        let tokyo = instant("2026-10-06T12:30:00+09:00").unwrap();
        assert!(berlin > tokyo);
        assert_eq!(
            instant("2026-10-06T06:13:00Z"),
            Some(berlin),
            "same instant in UTC"
        );
        assert_eq!(instant("2026-10-06T02:43:00-03:30"), Some(berlin));
        assert_eq!(instant("2026-10-06T08:13:59+02:00"), Some(berlin + 59));
        assert_eq!(instant("gestern"), None);
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
