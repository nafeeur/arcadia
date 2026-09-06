//! **True counts** for the review queue.
//!
//! The left-rail badge used to read the length of the array the endpoint returned, and that
//! endpoint always caps out at 100 — so a knowledge base with 164 low-confidence facts showed
//! "100" in the UI. Once you cleared those 100, the remaining 64 would surface, looking like
//! they'd appeared out of nowhere.
//!
//! **Counting and fetching are two different jobs, and must be kept separate.** Fetching has a
//! limit (ten per page, page through for the next batch); counting doesn't: `count(*)` runs
//! against the same WHERE clause as the list, and the same index.
//!
//! The eight COUNTs are combined into one query rather than fired as eight separate ones: they
//! all target the same kb, so one round trip fills the whole left rail at once, whereas
//! separate requests would make it pop in one item at a time when switching knowledge bases.

use sqlx::PgPool;
use utopia_core::models::ReviewCounts;
use utopia_core::AppResult;
use uuid::Uuid;

/// Low-confidence threshold. **Shared as one constant with `review_routes`** — writing the
/// number in two places eventually forks into "badge says 12, drill-in shows 9".
pub const LOW_CONFIDENCE_BELOW: f32 = 0.75;

/// The criterion for "unconfirmed", written as a SQL fragment shared between `counts` and the
/// overview (`review_summary`): evidence exists, but every chunk that evidence points to has
/// been superseded by a newer version. The alias is always `f`.
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
