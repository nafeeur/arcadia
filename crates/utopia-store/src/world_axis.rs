//! World-axis predicates (0022): `holds_at(T)` — **which facts hold at moment T**.
//!
//! The write side has long distinguished "still ongoing" from "ended, date unknown", and never
//! invents a start point for the original text. But the read side used to read both of those
//! back as "holds at any time": `valid_from IS NULL` was read as "has always been true", and
//! `valid_to IS NULL` was read as "still true today" without looking at the precision flag next
//! to it. Ask about a moment before any evidence existed, or a moment the source text says had
//! already ended (just without giving a date), and the graph would confidently answer anyway —
//! citing the very row that says it shouldn't hold (#345, #352).
//!
//! **The predicate is assembled in exactly one place**, for the same reason as `record_axis`:
//! guards scattered across every read site go silently missing one at a time. The frontend no
//! longer computes this itself either — edges and facts carry `holds_from` / `holds_to`
//! (the "read-out interval" projected using this same set of expressions), and the time slider
//! filters purely against those.
//!
//! The unknown end **reads up to the evidence**: `attested_at` is the date of the earliest
//! document among this row's observations. No start point -> holds from that point on; ended
//! but no date given -> holds up to that point. The asymmetry between the two ends is
//! deliberate: an open end still reads as "holds until someone says it ended" — an ending
//! arrives in the form of a record (a later document, a human correction) that closes the row;
//! a missing start point has no such corrector, nobody is going to say "in 2023 it hadn't
//! started yet". So a fact holds from the moment evidence for it exists, and before that the
//! honest answer is "no".
//!
//! `at` being NULL on the world axis means **every moment** (the canvas draws history; the
//! slider narrows it), unlike the record axis's "NULL means now": nobody holds a belief dated
//! later than the present moment, whereas a graph with no time filter is the graph of all time.

/// `facts`: the read-out lower bound — use the source's start point if it gave one, otherwise
/// the earliest evidence.
pub fn facts_holds_from(alias: &str) -> String {
    format!("COALESCE({alias}.valid_from, {alias}.attested_at)")
}

/// `facts`: the read-out upper bound — use the source's end point if it gave one; said to have
/// ended but no date given, use the earliest document that said so; otherwise open (NULL).
pub fn facts_holds_to(alias: &str) -> String {
    format!(
        "CASE WHEN {alias}.valid_to IS NOT NULL THEN {alias}.valid_to \
              WHEN {alias}.valid_to_precision = 'unknown' THEN {alias}.attested_at END"
    )
}

/// `facts`: asserts holding at moment T. `$param` of NULL means no filtering.
pub fn facts_hold_at(alias: &str, param: usize) -> String {
    format!(
        "(${param}::timestamptz IS NULL \
          OR ({from} <= ${param} AND ({to} IS NULL OR {to} > ${param})))",
        from = facts_holds_from(alias),
        to = facts_holds_to(alias),
    )
}

/// Plain interval containment, with a NULL end meaning open. Used by both derived rows and
/// phantom edges (0017 §3, whose interval lives in `detail`) — for both, the two ends aren't
/// stated by the source text, they're computed by the engine from its premises.
pub fn interval_holds_at(from: &str, to: &str, param: usize) -> String {
    format!(
        "(${param}::timestamptz IS NULL \
          OR (({from} IS NULL OR {from} <= ${param}) AND ({to} IS NULL OR {to} > ${param})))"
    )
}

/// `derived_facts`: a derived fact holds at moment T. Both ends are written by the evaluator as
/// the **read-out** interval intersection over its premises (0022 item 4: a premise with no
/// start point is counted from the anchor, one that ended with no date is counted up to the
/// anchor), so this is plain containment — a derived row needs no anchor of its own.
pub fn derived_hold_at(alias: &str, param: usize) -> String {
    interval_holds_at(
        &format!("{alias}.valid_from"),
        &format!("{alias}.valid_to"),
        param,
    )
}
