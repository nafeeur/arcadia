//! How time is written for the model to read: **one rule, every tool line goes through it**.
//!
//! - A world-axis endpoint is written at **its own precision**: year -> `2023`,
//!   month -> `2023-06`, day -> `2023-06-01`. It used to always be `%Y-%m-%d` --
//!   a year-precision fact stamped as January 1st, exactly the disease the
//!   comment on `facts.valid_from_precision` describes: filling in a certain
//!   value where there is ignorance. Write as many digits of precision as you have.
//! - An instant with no precision -- `at`, `as_of`, `recorded_at`, `doc_time`,
//!   and a bound raised up from an anchor (both ends of a derived row, or the
//!   start read out of a fact with no start, 0022) -- is written as full
//!   RFC3339 including fractional seconds. On the record axis, a correction can
//!   land within the same second as the original entry (#351); truncating to
//!   the day or the second would collapse the two acts of knowing into one.
//! - "Ended, unknown when" is written as `ended by <anchor>`, never as `now`.
//!
//! The frontend's `web/src/time.ts::fmtTime` and the exported `rdf::world_time`
//! are two other copies of this same rule; if the three diverge, what the
//! person sees, what the model sees, and what an auditor gets are no longer the same date.
use chrono::{DateTime, SecondsFormat, Utc};

/// An instant, written out in full.
pub fn instant(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::AutoSi, true)
}

/// One end of the world axis, written at its precision. Below the hour, the
/// abbreviated ISO 8601 form is used (`2026-06-01T14:32Z`); the same string
/// parses back to the same value and precision. With no precision it's an
/// instant (an anchor, a derived bound), written in full.
pub fn world(t: DateTime<Utc>, precision: Option<&str>) -> String {
    match precision {
        Some("year") => t.format("%Y").to_string(),
        Some("month") => t.format("%Y-%m").to_string(),
        Some("day") => t.format("%Y-%m-%d").to_string(),
        Some("hour") => t.format("%Y-%m-%dT%HZ").to_string(),
        Some("minute") => t.format("%Y-%m-%dT%H:%MZ").to_string(),
        Some("second") => t.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        _ => instant(t),
    }
}

/// Both ends of a fact: what the source says (`valid_*` and its precision) and what's read out (`holds_*`, 0022).
#[derive(Debug, Clone, Copy, Default)]
pub struct Span<'a> {
    pub valid_from: Option<DateTime<Utc>>,
    pub from_precision: Option<&'a str>,
    pub valid_to: Option<DateTime<Utc>>,
    pub to_precision: Option<&'a str>,
    pub holds_from: Option<DateTime<Utc>>,
    pub holds_to: Option<DateTime<Utc>>,
}

/// `from -> to`, for the model to read.
///
/// Start: written at its precision if the source gives one; if not, but there's
/// an anchor, written as `attested <instant>` -- the model should know this
/// fact is only attested as of that piece of evidence. End: written at its
/// precision if the source gives one; if it ended but the date is unknown,
/// written as `ended by <anchor>` (or `ended, date unknown` with no anchor);
/// otherwise `now`. When neither end has anything to write, returns an empty
/// string, and the caller decides from that not to wrap it in parentheses.
pub fn span(s: Span<'_>) -> String {
    let from = match (s.valid_from, s.holds_from) {
        (Some(t), _) => Some(world(t, s.from_precision)),
        (None, Some(a)) => Some(format!("attested {}", instant(a))),
        (None, None) => None,
    };
    let ended_unknown =
        s.valid_to.is_none() && s.to_precision == Some(utopia_store::graph::ENDED_UNKNOWN);
    let to = match (s.valid_to, ended_unknown, s.holds_to) {
        (Some(t), _, _) => Some(world(t, s.to_precision)),
        (None, true, Some(a)) => Some(format!("ended by {}", instant(a))),
        (None, true, None) => Some("ended, date unknown".to_string()),
        (None, false, _) => None,
    };
    match (from, to) {
        (None, None) => String::new(),
        (Some(f), None) => format!("{f} → now"),
        (None, Some(t)) => format!("→ {t}"),
        (Some(f), Some(t)) => format!("{f} → {t}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> DateTime<Utc> {
        s.parse().unwrap()
    }

    #[test]
    fn a_world_bound_shows_exactly_its_precision() {
        let at = t("2023-06-15T00:00:00Z");
        assert_eq!(world(at, Some("year")), "2023");
        assert_eq!(world(at, Some("month")), "2023-06");
        assert_eq!(world(at, Some("day")), "2023-06-15");
        let clock = t("2026-06-01T14:32:07.382Z");
        assert_eq!(world(clock, Some("hour")), "2026-06-01T14Z");
        assert_eq!(world(clock, Some("minute")), "2026-06-01T14:32Z");
        assert_eq!(world(clock, Some("second")), "2026-06-01T14:32:07Z");
        // What's written reads back, at the same precision
        for (p, s) in [
            ("hour", "2026-06-01T14Z"),
            ("minute", "2026-06-01T14:32Z"),
            ("second", "2026-06-01T14:32:07Z"),
        ] {
            let (back, bp) = utopia_extract::parse_time(s).unwrap();
            assert_eq!((bp, world(back, Some(p))), (p, s.to_string()));
        }
        // No precision = an instant (an anchor, a derived bound): write it in full, don't pretend it's some particular day
        assert_eq!(world(at, None), "2023-06-15T00:00:00Z");
    }

    #[test]
    fn an_instant_keeps_its_fraction_and_reads_back() {
        for s in [
            "2026-09-05T02:43:53Z",
            "2026-09-05T02:43:53.382Z",
            "2026-09-05T02:43:53.382001Z",
        ] {
            assert_eq!(instant(t(s)), s);
        }
    }

    #[test]
    fn a_span_reads_each_end_by_its_own_rule() {
        let day = |s: &str| Some(t(s));
        // The source gives both ends
        assert_eq!(
            span(Span {
                valid_from: day("2023-01-01T00:00:00Z"),
                from_precision: Some("year"),
                valid_to: day("2024-07-01T00:00:00Z"),
                to_precision: Some("month"),
                holds_from: day("2023-01-01T00:00:00Z"),
                holds_to: day("2024-07-01T00:00:00Z"),
            }),
            "2023 → 2024-07"
        );
        // No start: as of the evidence; still ongoing
        assert_eq!(
            span(Span {
                holds_from: day("2024-02-20T00:00:00Z"),
                ..Span::default()
            }),
            "attested 2024-02-20T00:00:00Z → now"
        );
        // Ended, unknown when: up to the document that states it, never now
        assert_eq!(
            span(Span {
                valid_from: day("2023-06-01T00:00:00Z"),
                from_precision: Some("day"),
                valid_to: None,
                to_precision: Some("unknown"),
                holds_from: day("2023-06-01T00:00:00Z"),
                holds_to: day("2025-10-15T00:00:00Z"),
            }),
            "2023-06-01 → ended by 2025-10-15T00:00:00Z"
        );
        // No anchor available in a record-axis event
        assert_eq!(
            span(Span {
                to_precision: Some("unknown"),
                ..Span::default()
            }),
            "→ ended, date unknown"
        );
        assert_eq!(span(Span::default()), "");
    }
}
