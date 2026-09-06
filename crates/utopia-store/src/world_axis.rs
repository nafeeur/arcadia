//! World-axis predicate (0022): `holds_at(T)` — **which facts hold at instant T**.
//!
//! The write side has always distinguished "still ongoing" from "ended, date unknown,"
//! and never invents a start for the source text. The read side used to read both back
//! as "holds at any time": `valid_from IS NULL` was read as "since forever,"
//! `valid_to IS NULL` was read as "still holds today" without looking at the precision
//! next to it. Ask about an instant before the evidence appeared, or an instant the
//! source text says already ended but just didn't give a date for, and the graph would
//! confidently answer anyway — citing exactly the row that says it shouldn't hold
//! (#345, #352).
//!
//! **The predicate is assembled in exactly one place**, for the same reason as
//! `record_axis`: scatter the defense across every read site and one gets missed
//! silently. The frontend no longer computes this itself either — edges and facts
//! carry `holds_from` / `holds_to` (the "read-back interval," projected from this same
//! set of expressions), and the slider filters only against those.
//!
//! The unknown end **reads as "until evidence arrives"**: `attested_at` is the date of
//! the earliest document among this row's observations. No start given -> holds from
//! that date; ended but no date given -> holds until that date. The asymmetry between
//! the two ends is deliberate: an open end still reads as "holds until someone says it
//! ended" — that ending will arrive in the form of a record (a later document, a human
//! correction) that closes the row; a missing start has no such corrector — nobody is
//! going to show up and say "as of 2023 it hadn't started yet." So a fact holds from
//! the moment evidence exists, and before that, the honest answer is none.
//!
//! `at` being NULL means **every instant** on the world axis (the canvas draws history,
//! the slider narrows it), unlike the record axis's "NULL means now": nobody holds a
//! belief dated later than this instant, and a graph with no instant given is the graph
//! of all time.

/// `facts`: the read-back lower bound — use the given start if the source text gave
/// one, otherwise the earliest evidence.
pub fn facts_holds_from(alias: &str) -> String {
    format!("COALESCE({alias}.valid_from, {alias}.attested_at)")
}

/// `facts`: the read-back upper bound — use the given end if the source text gave one;
/// if it says ended but with no date, use the earliest document that said so;
/// otherwise open-ended (NULL).
pub fn facts_holds_to(alias: &str) -> String {
    format!(
        "CASE WHEN {alias}.valid_to IS NOT NULL THEN {alias}.valid_to \
              WHEN {alias}.valid_to_precision = 'unknown' THEN {alias}.attested_at END"
    )
}

/// `facts`: the assertion holds at instant T. `$param` being NULL means no filtering.
pub fn facts_hold_at(alias: &str, param: usize) -> String {
    format!(
        "(${param}::timestamptz IS NULL \
          OR ({from} <= ${param} AND ({to} IS NULL OR {to} > ${param})))",
        from = facts_holds_from(alias),
        to = facts_holds_to(alias),
    )
}

/// Plain interval containment, either end NULL meaning open. Used by both derived rows
/// and phantom edges (0017 §3, interval lives in `detail`) — their two ends aren't
/// stated by the source text, they're computed by the engine from the premises.
pub fn interval_holds_at(from: &str, to: &str, param: usize) -> String {
    format!(
        "(${param}::timestamptz IS NULL \
          OR (({from} IS NULL OR {from} <= ${param}) AND ({to} IS NULL OR {to} > ${param})))"
    )
}

/// `derived_facts`: the derivation holds at instant T. Both ends were written in by the
/// evaluator as the **read-back** interval intersected across premises (0022 §4: a
/// premise with no start counts from the anchor, one that ended on an unknown date
/// counts until the anchor), so this is plain containment here — a derived row needs no
/// anchor of its own.
pub fn derived_hold_at(alias: &str, param: usize) -> String {
    interval_holds_at(
        &format!("{alias}.valid_from"),
        &format!("{alias}.valid_to"),
        param,
    )
}
