//! **True counts** for the review queue.
//!
//! The sidebar badge used to read the length of the array the endpoint
//! returned, and the endpoint always caps that at 100 — so a KB with 164
//! low-confidence facts showed 100. Clear those 100 and the remaining 64
//! surface later, looking like they grew out of nowhere.
//!
//! **Counting and fetching are two different jobs and must stay separate.**
//! Fetching is capped (ten per page, paginate for more); counting isn't:
//! `count(*)` uses the same WHERE clause as the list, and the same index.
//!
//! The eight COUNTs are folded into one query rather than fired as eight:
//! they're all against the same kb, so one round trip fills the sidebar at
//! once, whereas separate requests would make it pop in one row at a time
//! whenever the KB is switched.

use sqlx::PgPool;
use utopia_core::models::ReviewCounts;
use utopia_core::AppResult;
use uuid::Uuid;

/// Low-confidence threshold. **Shares one constant with `review_routes`** —
/// if each site hardcoded its own number, they'd eventually drift into
/// "badge says 12, clicking in shows 9".
pub const LOW_CONFIDENCE_BELOW: f32 = 0.75;

/// The "unconfirmed" predicate, written as a SQL fragment shared by `counts`
/// and the overview (`review_summary`): has evidence, but every chunk that
/// evidence points to has been superseded by a newer version. The alias is
/// fixed to `f`.
pub const UNCONFIRMED_FACT: &str = "EXISTS (SELECT 1 FROM fact_evidence fe WHERE fe.fact_id = f.id)
               AND NOT EXISTS (SELECT 1 FROM fact_evidence fe
                                 JOIN chunks c ON c.id = fe.chunk_id
                                WHERE fe.fact_id = f.id
                                  AND c.superseded_at IS NULL)";

pub async fn counts(pool: &PgPool, kb_id: Uuid) -> AppResult<ReviewCounts> {
    let sql = format!(
        "SELECT
           (SELECT count(*) FROM pending_facts WHERE kb_id = $1) AS pending,
           (SELECT count(*) FROM resolution_reviews
             WHERE kb_id = $1 AND status = 'pending') AS duplicates,
           (SELECT count(*) FROM fact_conflicts
             WHERE kb_id = $1 AND status = 'open') AS conflicts,
           (SELECT count(*) FROM facts f
             WHERE f.kb_id = $1 AND f.invalidated_at IS NULL AND {unconfirmed}) AS unconfirmed,
           (SELECT count(*) FROM facts
             WHERE kb_id = $1 AND invalidated_at IS NULL
               AND confidence < $2 AND derived_by_rule IS NULL) AS lowconf,
           (SELECT count(*) FROM concept_mappings
             WHERE kb_id = $1 AND status = 'proposed') AS mappings,
           (SELECT count(*) FROM axiom_violations
             WHERE kb_id = $1 AND status = 'open') AS violations,
           (SELECT count(*) FROM ontology_defects
             WHERE kb_id = $1 AND status = 'open') AS defects,
           (SELECT count(*) FROM entity_merges WHERE kb_id = $1) AS merges",
        unconfirmed = UNCONFIRMED_FACT,
    );
    Ok(sqlx::query_as(&sql)
        .bind(kb_id)
        .bind(LOW_CONFIDENCE_BELOW)
        .fetch_one(pool)
        .await?)
}
