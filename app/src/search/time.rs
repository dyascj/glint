//! Time zones in the calculator: "time in tokyo", "london time", "3pm in
//! tokyo", "9:30am pst to berlin". A place is any city in the system's time
//! zone list ("New York", "Kolkata"), a zone abbreviation ("PST", "CET") or
//! one of a few nicknames ("NYC", "SF").

use objc2::rc::Retained;
use objc2_foundation::{NSDate, NSString, NSTimeZone};

use super::calc::Answer;

/// Nicknames and big cities that aren't a time zone's own name.
const ALIASES: &[(&str, &str, &str)] = &[
    ("nyc", "America/New_York", "New York"),
    ("ny", "America/New_York", "New York"),
    ("boston", "America/New_York", "Boston"),
    ("washington", "America/New_York", "Washington"),
    ("dc", "America/New_York", "Washington"),
    ("atlanta", "America/New_York", "Atlanta"),
    ("miami", "America/New_York", "Miami"),
    ("philadelphia", "America/New_York", "Philadelphia"),
    ("chicago", "America/Chicago", "Chicago"),
    ("dallas", "America/Chicago", "Dallas"),
    ("houston", "America/Chicago", "Houston"),
    ("austin", "America/Chicago", "Austin"),
    ("denver", "America/Denver", "Denver"),
    ("sf", "America/Los_Angeles", "San Francisco"),
    ("san francisco", "America/Los_Angeles", "San Francisco"),
    ("la", "America/Los_Angeles", "Los Angeles"),
    ("seattle", "America/Los_Angeles", "Seattle"),
    ("portland", "America/Los_Angeles", "Portland"),
    ("san diego", "America/Los_Angeles", "San Diego"),
    ("beijing", "Asia/Shanghai", "Beijing"),
    ("china", "Asia/Shanghai", "China"),
    ("mumbai", "Asia/Kolkata", "Mumbai"),
    ("delhi", "Asia/Kolkata", "Delhi"),
    ("new delhi", "Asia/Kolkata", "New Delhi"),
    ("bangalore", "Asia/Kolkata", "Bangalore"),
    ("india", "Asia/Kolkata", "India"),
    ("japan", "Asia/Tokyo", "Japan"),
    ("korea", "Asia/Seoul", "Korea"),
    ("uk", "Europe/London", "UK"),
    ("england", "Europe/London", "England"),
    ("germany", "Europe/Berlin", "Germany"),
    ("france", "Europe/Paris", "France"),
    ("spain", "Europe/Madrid", "Spain"),
    ("italy", "Europe/Rome", "Italy"),
    ("netherlands", "Europe/Amsterdam", "Netherlands"),
    ("utc", "UTC", "UTC"),
    ("gmt", "GMT", "GMT"),
];

/// Answers a time question, or None when the query isn't one.
pub fn evaluate(q: &str) -> Option<Answer> {
    let q = q.trim();
    // "time in tokyo", "now in tokyo", "tokyo time"
    let here = ["time in ", "current time in ", "now in ", "what time is it in "].iter().find_map(|p| q.strip_prefix(p)).or_else(|| q.strip_suffix(" time"));
    if let Some(place) = here {
        let (name, tz) = zone(place)?;
        return Some(answer(now(), &name, &tz));
    }
    // "<clock> [place] in|to <place>"
    let (lhs, target) = [" in ", " to "].iter().find_map(|sep| q.rsplit_once(sep))?;
    let (name, tz) = zone(target)?;
    let words: Vec<&str> = lhs.split_whitespace().collect();
    // The clock is the first one or two words ("3pm", "3 pm"); the rest is where.
    let (clock, from) = (1..=words.len().min(2)).rev().find_map(|n| {
        let c = clock(&words[..n].join(" "))?;
        let rest = words[n..].join(" ");
        let from = if rest.is_empty() { Some(None) } else { zone(&rest).map(Some) };
        from.map(|f| (c, f))
    })?;
    let local = NSTimeZone::localTimeZone();
    let src = from.as_ref().map_or(&local, |(_, tz)| tz);
    let instant = match clock {
        Clock::Now => now(),
        Clock::At(h, m) => at(now(), offset(src, now()), h, m),
    };
    Some(answer(instant, &name, &tz))
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Clock {
    Now,
    At(i64, i64),
}

/// "now", "noon", "midnight", "3pm", "3 pm", "3:30am", "15:30".
fn clock(s: &str) -> Option<Clock> {
    let s = s.trim().replace('.', "");
    match s.as_str() {
        "now" => return Some(Clock::Now),
        "noon" => return Some(Clock::At(12, 0)),
        "midnight" => return Some(Clock::At(0, 0)),
        _ => {}
    }
    let (body, half) = match (s.strip_suffix("am"), s.strip_suffix("pm")) {
        (Some(b), _) => (b.trim(), Some(false)),
        (_, Some(b)) => (b.trim(), Some(true)),
        _ => (s.as_str(), None),
    };
    let (h, m) = match body.split_once(':') {
        Some((h, m)) if m.len() == 2 => (h.parse::<i64>().ok()?, m.parse::<i64>().ok()?),
        // A bare number is only a time with am/pm ("3" alone could be anything).
        None if half.is_some() => (body.parse::<i64>().ok()?, 0),
        _ => return None,
    };
    if m > 59 {
        return None;
    }
    let h = match half {
        Some(pm) if (1..=12).contains(&h) => h % 12 + if pm { 12 } else { 0 },
        None if (0..=23).contains(&h) => h,
        _ => return None,
    };
    Some(Clock::At(h, m))
}

/// A place's display name and time zone.
fn zone(place: &str) -> Option<(String, Retained<NSTimeZone>)> {
    let place = place.trim();
    if place.is_empty() || place.len() > 40 {
        return None;
    }
    if let Some((_, id, name)) = ALIASES.iter().find(|(a, _, _)| *a == place) {
        return NSTimeZone::timeZoneWithName(&NSString::from_str(id)).map(|tz| (name.to_string(), tz));
    }
    // Abbreviations the system knows: PST, EST, CET, JST...
    let upper = place.to_uppercase();
    if upper.len() <= 5
        && upper.chars().all(|c| c.is_ascii_alphabetic())
        && let Some(id) = NSTimeZone::abbreviationDictionary().objectForKey(&NSString::from_str(&upper))
        && let Some(tz) = NSTimeZone::timeZoneWithName(&id)
    {
        return Some((upper, tz));
    }
    // Any city in the zone list: "America/Argentina/Buenos_Aires" -> "buenos aires".
    NSTimeZone::knownTimeZoneNames().iter().find_map(|id| {
        let id = id.to_string();
        let city = id.rsplit('/').next()?.replace('_', " ");
        (city.to_lowercase() == place).then(|| NSTimeZone::timeZoneWithName(&NSString::from_str(&id)).map(|tz| (city, tz))).flatten()
    })
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// Seconds east of UTC in `tz` at the instant `t`.
fn offset(tz: &NSTimeZone, t: i64) -> i64 {
    tz.secondsFromGMTForDate(&NSDate::dateWithTimeIntervalSince1970(t as f64)) as i64
}

/// The instant that reads `h:m` today on a clock `off` seconds east of UTC.
fn at(now: i64, off: i64, h: i64, m: i64) -> i64 {
    let local = now + off;
    local - local.rem_euclid(86_400) + h * 3600 + m * 60 - off
}

/// "3:42 PM" / "15:42" and weekday for `t` on a clock `off` east of UTC.
fn read(t: i64, off: i64) -> (String, String, &'static str) {
    let local = t + off;
    let secs = local.rem_euclid(86_400);
    let (h, m) = (secs / 3600, secs % 3600 / 60);
    let h12 = if h % 12 == 0 { 12 } else { h % 12 };
    // 1 January 1970 was a Thursday.
    let day = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"][local.div_euclid(86_400).rem_euclid(7) as usize];
    (format!("{h12}:{m:02} {}", if h < 12 { "AM" } else { "PM" }), format!("{h:02}:{m:02}"), day)
}

fn answer(t: i64, name: &str, tz: &NSTimeZone) -> Answer {
    let (text, raw, day) = read(t, offset(tz, t));
    Answer { text, raw, unit: Some(format!("{name} · {day}")) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clocks() {
        assert_eq!(clock("3pm"), Some(Clock::At(15, 0)));
        assert_eq!(clock("3 pm"), Some(Clock::At(15, 0)));
        assert_eq!(clock("12am"), Some(Clock::At(0, 0)));
        assert_eq!(clock("12:30 p.m."), Some(Clock::At(12, 30)));
        assert_eq!(clock("09:05"), Some(Clock::At(9, 5)));
        assert_eq!(clock("noon"), Some(Clock::At(12, 0)));
        assert_eq!(clock("3"), None, "a bare number isn't a time");
        assert_eq!(clock("13pm"), None);
        assert_eq!(clock("5 km"), None);
    }

    #[test]
    fn arithmetic_on_clocks() {
        // 2026-10-22 18:00 UTC is a Thursday.
        let t = 1_792_692_000;
        assert_eq!(read(t, 0), ("6:00 PM".into(), "18:00".into(), "Thu"));
        assert_eq!(read(t, 9 * 3600).2, "Fri", "Tokyo is already tomorrow");
        // 9:00 today in UTC-7 is 16:00 UTC.
        assert_eq!(read(at(t, -7 * 3600, 9, 0), 0).1, "16:00");
    }

    #[test]
    fn questions() {
        let a = evaluate("12:00 utc in tokyo").expect("a conversion");
        assert_eq!(a.text, "9:00 PM");
        assert!(a.unit.as_deref().is_some_and(|u| u.starts_with("Tokyo · ")));
        assert_eq!(evaluate("noon gmt to new york").map(|a| a.unit.unwrap_or_default().starts_with("New York")), Some(true));
        assert!(evaluate("time in buenos aires").is_some());
        assert!(evaluate("london time").is_some());
        assert!(evaluate("3pm pst in berlin").is_some());
        assert!(evaluate("now in nyc").is_some());
        assert!(evaluate("5 km to mi").is_none());
        assert!(evaluate("time in narnia").is_none());
        assert!(evaluate("3 in tokyo").is_none());
    }
}
