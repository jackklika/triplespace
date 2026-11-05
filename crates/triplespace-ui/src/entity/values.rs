//! Data values as text (wikibase-compat §3.3): times, coordinates, dates and numbers in
//! the reader's language, for the models of [`super::model`] and the pages that write a
//! date. The values themselves are drawn by the skin's `entity.html` macros from
//! [`super::model::Value`].
//!
//! Numbers are grouped and dates written in English forms for now: the messages carry the
//! word order and month names, and per-language number formats come with ICU4X decimal
//! formatting when a translation needs them.

use scatter_wikibase_model::value::{GlobeCoordinate, Time};

/// A point in time as the reader's language writes it, to the value's precision; `None`
/// where the time string cannot be read.
#[must_use]
pub fn time_text(m: &crate::i18n::Messages, time: &Time) -> Option<String> {
    let parts = parse_time(&time.time)?;
    // Historical numbering: `-0044` is 44 BCE, and there is no year 0.
    let year = if time.time.starts_with('-') {
        -parts.year
    } else {
        parts.year
    };
    let year_text = |y: i64| -> String {
        if y < 0 {
            m.with("ts-time-bce", &[&y.unsigned_abs().to_string()])
        } else {
            y.to_string()
        }
    };
    Some(match time.precision {
        11.. if parts.month > 0 && parts.day > 0 => m.with(
            "ts-time-day",
            &[
                &parts.day.to_string(),
                &m.get(&format!("ts-month-{}", parts.month)),
                &year_text(year),
            ],
        ),
        10.. if parts.month > 0 => m.with(
            "ts-time-month",
            &[
                &m.get(&format!("ts-month-{}", parts.month)),
                &year_text(year),
            ],
        ),
        8 => m.with("ts-time-decade", &[&year_text(year - year.rem_euclid(10))]),
        7 => {
            let century = (year.abs() - 1) / 100 + 1;
            let s = m.with("ts-time-century", &[&ordinal(century)]);
            if year < 0 {
                m.with("ts-time-bce", &[&s])
            } else {
                s
            }
        }
        6 => {
            let millennium = (year.abs() - 1) / 1000 + 1;
            let s = m.with("ts-time-millennium", &[&ordinal(millennium)]);
            if year < 0 {
                m.with("ts-time-bce", &[&s])
            } else {
                s
            }
        }
        _ => year_text(year),
    })
}

/// Coordinates as degrees, minutes and seconds to the value's precision.
#[must_use]
pub fn coordinate_text(g: &GlobeCoordinate) -> String {
    let precision = g.precision.unwrap_or(1.0 / 3600.0);
    format!(
        "{}, {}",
        dms(g.latitude, precision, ('N', 'S')),
        dms(g.longitude, precision, ('E', 'W'))
    )
}

/// A date from an ISO 8601 timestamp, as the reader's language writes it.
#[must_use]
pub fn date(m: &crate::i18n::Messages, iso: &str) -> String {
    let d = iso.split('T').next().unwrap_or(iso);
    let mut it = d.split('-');
    match (
        it.next(),
        it.next().and_then(|x| x.parse::<u32>().ok()),
        it.next().and_then(|x| x.parse::<u32>().ok()),
    ) {
        (Some(y), Some(mo), Some(day)) if (1..=12).contains(&mo) => m.with(
            "ts-time-day",
            &[&day.to_string(), &m.get(&format!("ts-month-{mo}")), y],
        ),
        _ => iso.to_string(),
    }
}

/// The parts of a Wikibase time string.
#[derive(Debug, PartialEq, Eq)]
struct TimeParts {
    year: i64,
    month: u32,
    day: u32,
}

fn parse_time(s: &str) -> Option<TimeParts> {
    let body = s.trim_start_matches(['+', '-']);
    let (date, _) = body.split_once('T')?;
    let mut it = date.split('-');
    let year = it.next()?.parse::<i64>().ok()?;
    let month = it.next()?.parse::<u32>().ok()?;
    let day = it.next()?.parse::<u32>().ok()?;
    Some(TimeParts { year, month, day })
}

/// An English ordinal: 1st, 2nd, 3rd, 4th, 11th, 21st.
fn ordinal(n: i64) -> String {
    let suffix = match (n % 10, n % 100) {
        (1, r) if r != 11 => "st",
        (2, r) if r != 12 => "nd",
        (3, r) if r != 13 => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

pub(super) fn num(s: &str) -> Option<f64> {
    s.trim_start_matches('+').parse().ok()
}

pub(super) fn trim_float(f: f64) -> String {
    let s = format!("{f:.12}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    s.to_string()
}

/// A decimal string with its sign dropped when positive and its whole part grouped by
/// thousands: `+3898747` → `3,898,747`, `-1234.50` → `-1,234.50`.
#[must_use]
pub fn group(amount: &str) -> String {
    let (sign, rest) = match amount.as_bytes().first() {
        Some(b'+') => ("", &amount[1..]),
        Some(b'-') => ("-", &amount[1..]),
        _ => ("", amount),
    };
    let (whole, frac) = rest
        .split_once('.')
        .map_or((rest, None), |(w, f)| (w, Some(f)));
    if !whole.bytes().all(|b| b.is_ascii_digit()) {
        return amount.to_string();
    }
    let mut grouped = String::new();
    for (i, c) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    match frac {
        Some(f) => format!("{sign}{grouped}.{f}"),
        None => format!("{sign}{grouped}"),
    }
}

/// Degrees as degrees, minutes and seconds to the value's precision.
fn dms(value: f64, precision: f64, (pos, neg): (char, char)) -> String {
    let hemi = if value < 0.0 { neg } else { pos };
    let v = value.abs();
    if precision >= 1.0 {
        return format!("{}°{hemi}", v.round());
    }
    if precision >= 1.0 / 60.0 - 1e-12 {
        let total = (v * 60.0).round();
        return format!("{}°{}′{hemi}", (total / 60.0).floor(), total % 60.0);
    }
    let seconds_precision = precision * 3600.0;
    // As many decimals of a second as the precision has, up to six.
    let places: i32 = (0..=6)
        .find(|d| seconds_precision * 10f64.powi(*d) >= 1.0 - 1e-9)
        .unwrap_or(6);
    let decimals = usize::try_from(places).unwrap_or(0);
    let scale = 10f64.powi(places);
    let total = (v * 3600.0 * scale).round() / scale;
    let d = (total / 3600.0).floor();
    let m = ((total - d * 3600.0) / 60.0).floor();
    let s = total - d * 3600.0 - m * 60.0;
    format!("{d}°{m}′{s:.decimals$}″{hemi}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers() {
        assert_eq!(group("+3898747"), "3,898,747");
        assert_eq!(group("-1234.50"), "-1,234.50");
        assert_eq!(group("+12"), "12");
        assert_eq!(group("+0.5"), "0.5");
        assert_eq!(group("abc"), "abc");
    }

    #[test]
    fn times_and_ordinals() {
        assert_eq!(
            parse_time("+1952-03-11T00:00:00Z"),
            Some(TimeParts {
                year: 1952,
                month: 3,
                day: 11
            })
        );
        assert_eq!(ordinal(1), "1st");
        assert_eq!(ordinal(11), "11th");
        assert_eq!(ordinal(22), "22nd");
        assert_eq!(ordinal(103), "103rd");
    }

    #[test]
    fn coordinates() {
        assert_eq!(dms(34.0522, 1.0 / 60.0, ('N', 'S')), "34°3′N");
        assert_eq!(dms(-118.2437, 1.0 / 60.0, ('E', 'W')), "118°15′W");
        assert_eq!(dms(51.4779, 1.0, ('N', 'S')), "51°N");
        assert_eq!(dms(48.8584, 1.0 / 3600.0, ('N', 'S')), "48°51′30″N");
        assert_eq!(dms(48.858_37, 0.000_01, ('N', 'S')), "48°51′30.13″N");
    }
}
