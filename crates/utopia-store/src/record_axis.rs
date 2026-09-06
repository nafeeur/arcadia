//! Record-axis predicates (0019): `held_at(T)` — **which rows we believed held at time T**.
//!
//! The world axis (`valid_from` / `valid_to`) answers "what was the world
//! like then"; the record axis (`recorded_at` / `invalidated_at`) answers
//! "what did we believe the world was like then". The write side has tracked
//! both axes since the graph migration, but the read side has only ever been
//! able to roll back the first one — a fact edited away in March doesn't
//! exist at any position of the slider.
//!
//! **Predicates get assembled only here.** Once the defense is scattered
//! across every read site, one missed spot fails silently: SQL won't error,
//! and `cargo check` won't say a word (that's exactly the trip 0009 took —
//! the `human_type_decisions` test is what that fall left behind). So read
//! paths call the functions here instead of writing `invalidated_at` themselves.
//!
//! **Not used on the write path** (0019): `confirm_fact` / `reject_fact`,
//! adopted retractions, and dedup-lookup are all guards over "the current
//! row" — a correction always happens in the present; there's no such thing
//! as "editing a row as of March".
//!
//! Parameters bind to `Option<DateTime<Utc>>`: `NULL` means "now", and the
//! predicate degenerates to `invalidated_at IS NULL` (no row's invalidation
//! moment can be later than now). The read path therefore needs only one
//! statement, not one for replay and another for the present — two would be
//! the next place a fix gets missed.

/// The record-axis interval formed by the start/end columns: `since <= T < invalidated_at`.
fn held(alias: &str, since: &str, param: usize) -> String {
    format!(
        "{alias}.{since} <= coalesce(${param}, now()) \
         AND ({alias}.invalidated_at IS NULL OR {alias}.invalidated_at > coalesce(${param}, now()))"
    )
}

/// `facts`: the assertion is still held by us at time T.
pub fn facts_held_at(alias: &str, param: usize) -> String {
    held(alias, "recorded_at", param)
}

/// `derived_facts`: derived at time T, and not yet overturned as of then —
/// the replayed graph keeps the edge derived **at that time**, not the
/// conclusion of today's rule set.
pub fn derived_held_at(alias: &str, param: usize) -> String {
    held(alias, "derived_at", param)
}

/// `axiom_violations`: the violation was still open at time T. Column names
/// differ from the two tables above (`detected_at` / `decided_at` + `status`),
/// but it's asking the same question — so it belongs here too, rather than
/// being assembled ad hoc at a read site.
///
/// A historical row that was resolved but never got a `decided_at` is
/// treated as "not open back then either": better to omit one ghost edge
/// than to add a contradiction to March's graph that wasn't actually
/// discovered until today.
pub fn violation_open_at(alias: &str, param: usize) -> String {
    format!(
        "{alias}.detected_at <= coalesce(${param}, now()) \
         AND ({alias}.status = 'open' OR {alias}.decided_at > coalesce(${param}, now()))"
    )
}

/// `fact_conflicts`: the temporal conflict was still open at time T.
pub fn conflict_open_at(alias: &str, param: usize) -> String {
    format!(
        "{alias}.created_at <= coalesce(${param}, now()) \
         AND ({alias}.status = 'open' OR {alias}.resolved_at > coalesce(${param}, now()))"
    )
}

/// `documents`: the document was still in the KB at time T. Deletion leaves a
/// tombstone (#268), so a "deleted" document should still appear normally at
/// any time before its deletion — its chunks really were searchable back then.
pub fn document_live_at(alias: &str, param: usize) -> String {
    format!(
        "{alias}.created_at <= coalesce(${param}, now()) \
         AND ({alias}.deleted_at IS NULL OR {alias}.deleted_at > coalesce(${param}, now()))"
    )
}

/// `chunks`: the chunk was still the current version at time T. Whether
/// evidence has "disappeared" must be judged against the version at that
/// time — a passage superseded today by re-parsing is still live evidence on
/// March's graph.
pub fn chunk_live_at(alias: &str, param: usize) -> String {
    format!(
        "{alias}.created_at <= coalesce(${param}, now()) \
         AND ({alias}.superseded_at IS NULL OR {alias}.superseded_at > coalesce(${param}, now()))"
    )
}

/// `entity_merges`: was this merge **in effect** at time T (0019's second cut / #336)?
///
/// Entities carry no record axis of their own — `merged_into` only says a
/// merge happened, not when. The moment lives on this table instead, and
/// since it's asking the same question as the others, the column names
/// differ but the shape doesn't.
pub fn merge_in_effect_at(alias: &str, param: usize) -> String {
    format!(
        "{alias}.created_at <= coalesce(${param}, now()) \
         AND ({alias}.reverted_at IS NULL OR {alias}.reverted_at > coalesce(${param}, now()))"
    )
}

/// Whether an entity was a standalone node at time T: it existed by then, and
/// wasn't swallowed by a merge in effect at that time.
///
/// **Replaces `merged_into IS NULL` on the read path.** When the parameter is
/// NULL the two are equivalent (a reverted merge isn't in effect now, so that
/// entity would show up anyway), but once a moment is passed, an entity
/// merged away in March grows back in February — which is exactly the point
/// of this cut.
pub fn entity_visible_at(alias: &str, param: usize) -> String {
    let merged = merge_in_effect_at("m", param);
    format!(
        "{alias}.created_at <= coalesce(${param}, now()) \
         AND NOT EXISTS (SELECT 1 FROM entity_merges m \
                          WHERE m.source_id = {alias}.id AND {merged})"
    )
}

/// A fact's subject (`on_object = false`) or object at time T.
///
/// **The "now" path doesn't go through this function**: `fact_owner_at`
/// wraps the column in a way that loses the index, and "now" is the path
/// every single graph render takes. Only replay pays that cost — and replay
/// is rare to begin with.
pub fn owner_at(fact_alias: &str, column: &str, as_of: Option<usize>, on_object: bool) -> String {
    match as_of {
        None => format!("{fact_alias}.{column}"),
        Some(param) => {
            format!("fact_owner_at({fact_alias}.id, {fact_alias}.{column}, ${param}, {on_object})")
        }
    }
}
