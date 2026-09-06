//! Extraction drop signals: which facts were extracted but failed to land, and why.
//!
//! Kept separate from `ontology_misses` on purpose — that table says "your
//! ontology is missing these", read by the ontology maintainer, actioned by
//! adding a type; this one says "these facts didn't land", read by whoever
//! uploaded the document, actioned by editing the document or the ontology.
//! Mixing them into one panel would muddy both.
//!
//! A logging failure never blocks extraction (callers always use `let _ =`) —
//! missing one signal is far better than aborting the whole document's
//! extraction because logging the signal failed.

use sqlx::PgPool;
use utopia_core::models::ExtractionDrop;
use utopia_core::AppResult;
use uuid::Uuid;

/// Reason codes. The frontend looks up copy by these, so they're a stable
/// contract — don't change the literals.
pub mod reason {
    /// Subject wasn't declared in entities -> type unknown, can't validate the attribute's domain
    pub const SUBJECT_NOT_DECLARED: &str = "subject_not_declared";
    /// Attribute attached to a class it shouldn't be (e.g. salary on Organization)
    pub const ATTR_DOMAIN_MISMATCH: &str = "attr_domain_mismatch";
    /// Attribute fact gave neither a value nor an object
    pub const ATTR_NO_VALUE: &str = "attr_no_value";
    /// Value doesn't fit the datatype; normalization failed
    pub const ATTR_DATATYPE: &str = "attr_datatype";
    /// Model's self-reported confidence is below threshold
    pub const LOW_CONFIDENCE: &str = "low_confidence";
    /// Relation fact is missing its object
    pub const OBJECT_MISSING: &str = "object_missing";
    /// This item from the model is malformed (e.g. missing predicate) -> skip only this item, not the whole block
    pub const MALFORMED_ITEM: &str = "malformed_item";
    /// Subject's type doesn't match the relation's declared domain, **and
    /// swapping them isn't valid either** — that means the wrong relation was
    /// picked or the type was misjudged, not a direction problem. Land it
    /// as-is plus log the signal, and leave it for a human — don't guess
    pub const DOMAIN_MISMATCH: &str = "domain_mismatch";
    /// The "entity name" the model gave is actually a whole sentence or
    /// clause — not the name of a thing. This kind of item never matches any
    /// other mention, sits as an isolated node in the graph, and drags down resolution
    pub const NOT_AN_ENTITY_NAME: &str = "not_an_entity_name";
    /// Subject violates the domain while the object fits it, so subject and
    /// object have been swapped to match the ontology's declared direction.
    /// **The action must leave a trace** — an automatic, invisible rewrite is
    /// exactly what 0001 argues against
    pub const DIRECTION_CORRECTED: &str = "direction_corrected";
    /// Model output was truncated (hit max_tokens) -> the complete items are kept, the tail is dropped
    pub const TRUNCATED_REPLY: &str = "truncated_reply";
    /// The guard let it through, but the structure looks like a clause (a
    /// long run starting with a determiner, a relative word mid-sentence).
    /// **Log only, never block**: the entity is still stored as usual, and the
    /// example is kept — #193 wants a cross-corpus annotated set before
    /// deciding which of these get promoted to a hard rule
    pub const CLAUSE_SUSPECT: &str = "clause_suspect";
}

pub async fn record(
    pool: &PgPool,
    kb_id: Uuid,
    document_id: Uuid,
    reason: &str,
    detail: &str,
    example: Option<&str>,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO extraction_drops (kb_id, document_id, reason, detail, example)
         VALUES ($1, $2, $3, left($4, 120), left($5, 200))
         ON CONFLICT (kb_id, document_id, reason, detail)
         DO UPDATE SET count = extraction_drops.count + 1,
                       example = COALESCE(EXCLUDED.example, extraction_drops.example),
                       updated_at = now()",
    )
    .bind(kb_id)
    .bind(document_id)
    .bind(reason)
    .bind(detail)
    .bind(example)
    .execute(pool)
    .await?;
    Ok(())
}

/// Clear this document's old signals when re-extraction starts — this pass
/// tells the document's story from scratch.
pub async fn clear_for_document(pool: &PgPool, document_id: Uuid) -> AppResult<()> {
    sqlx::query("DELETE FROM extraction_drops WHERE document_id = $1")
        .bind(document_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// All drop signals for a KB. Rows are aggregated by (document x reason x
/// specific object), so the count stays small; fetching it all at once lets
/// the Library both total each document's drops and expand details directly,
/// without a request per row.
pub async fn for_kb(pool: &PgPool, kb_id: Uuid) -> AppResult<Vec<ExtractionDrop>> {
    Ok(sqlx::query_as(
        "SELECT document_id, reason, detail, count, example FROM extraction_drops
         WHERE kb_id = $1 ORDER BY count DESC, reason LIMIT 2000",
    )
    .bind(kb_id)
    .fetch_all(pool)
    .await?)
}
