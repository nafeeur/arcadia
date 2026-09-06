//! Agent/conversation memory: the episodes fast path.
//!
//! Design (zero new wheels): the memory space *is* the knowledge base itself;
//! each episode is a chunk appended to the "Memory log" document under an
//! implicit "Memory" source (with the event timestamp embedded as a line in
//! the text). This reuses the whole pipeline for free: the chunk enters
//! full-text/vector indexing (memory becomes searchable), fact_evidence points
//! at the chunk (memory-derived facts trace back to the original words),
//! incremental extraction only processes new episodes via extracted_at, a
//! fact's valid_from takes the event time, and the temporal engine
//! auto-closes contradictions with existing functional facts — "liked A last
//! month, switched to B this month" naturally becomes two intervals. Ledger
//! discipline: episodes are append-only, never rewritten.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use utopia_core::AppResult;
use uuid::Uuid;

pub const MEMORY_SOURCE_KIND: &str = "memory";
const MEMORY_DOC_KEY: &str = "memory:log";

/// The implicit Memory source per KB (undeletable; visible in the Library —
/// memory transparency is a feature).
pub async fn get_or_create_memory_source(pool: &PgPool, kb_id: Uuid) -> AppResult<Uuid> {
    if let Some((id,)) = sqlx::query_as::<_, (Uuid,)>(
        "SELECT id FROM sources WHERE kb_id = $1 AND kind = $2 LIMIT 1",
    )
    .bind(kb_id)
    .bind(MEMORY_SOURCE_KIND)
    .fetch_optional(pool)
    .await?
    {
        return Ok(id);
    }
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO sources (id, kb_id, kind, name, config) VALUES ($1, $2, $3, 'Memory', '{}')",
    )
    .bind(id)
    .bind(kb_id)
    .bind(MEMORY_SOURCE_KIND)
    .execute(pool)
    .await?;
    Ok(id)
}

/// The Memory log document (one per KB). Bypasses documents::create's
/// same-content dedup — it isn't a content-addressed file, so sha256 is
/// filled with a sentinel value.
pub async fn get_or_create_memory_doc(pool: &PgPool, kb_id: Uuid) -> AppResult<Uuid> {
    if let Some((id,)) = sqlx::query_as::<_, (Uuid,)>(
        "SELECT id FROM documents
          WHERE kb_id = $1 AND external_key = $2 AND deleted_at IS NULL LIMIT 1",
    )
    .bind(kb_id)
    .bind(MEMORY_DOC_KEY)
    .fetch_optional(pool)
    .await?
    {
        return Ok(id);
    }
    let source_id = get_or_create_memory_source(pool, kb_id).await?;
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO documents
            (id, kb_id, source_id, filename, mime, size_bytes, sha256,
             status, graph_status, external_key, doc_time_source)
         VALUES ($1, $2, $3, 'memory-log.md', 'text/markdown', 0, 'memory:log',
                 'ready', 'done', $4, 'none')",
    )
    .bind(id)
    .bind(kb_id)
    .bind(source_id)
    .bind(MEMORY_DOC_KEY)
    .execute(pool)
    .await?;
    Ok(id)
}

/// Append an episode: a new chunk (extracted_at empty -> incremental
/// extraction will pick it up; embedding empty -> memory_ingest will fill it
/// in). The event time is embedded in the first line of the text, and the
/// extraction model uses it to set valid_from.
pub async fn append_episode(
    pool: &PgPool,
    kb_id: Uuid,
    text: &str,
    occurred_at: DateTime<Utc>,
) -> AppResult<(Uuid, Uuid)> {
    let doc_id = get_or_create_memory_doc(pool, kb_id).await?;
    let stamped = format!("[{}] {}", occurred_at.format("%Y-%m-%d %H:%M"), text.trim());
    let chunk_id = Uuid::now_v7();
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO chunks (id, kb_id, document_id, seq, text, char_start, char_end, doc_version)
         VALUES ($1, $2, $3,
                 (SELECT COALESCE(MAX(seq), -1) + 1 FROM chunks
                  WHERE document_id = $3 AND superseded_at IS NULL),
                 $4, 0, $5, 1)",
    )
    .bind(chunk_id)
    .bind(kb_id)
    .bind(doc_id)
    .bind(&stamped)
    .bind(stamped.chars().count() as i32)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE documents SET chunk_count = chunk_count + 1, size_bytes = size_bytes + $2,
                updated_at = now() WHERE id = $1",
    )
    .bind(doc_id)
    .bind(stamped.len() as i64)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok((doc_id, chunk_id))
}

/// Whether this document is the memory log.
///
/// **Extraction uses this to decide whether a fact needs human sign-off**
/// (0015): a memory is a sentence someone deliberately said in conversation —
/// one at a time, with the person right there, so confirming it costs almost
/// nothing; whereas an ingested document brings in tens of thousands at once,
/// where confirming each one is impossible, so that path still goes with
/// optimistic write plus after-the-fact review.
pub async fn is_memory_document(pool: &PgPool, document_id: Uuid) -> AppResult<bool> {
    let found: Option<(i32,)> = sqlx::query_as(
        "SELECT 1 FROM documents d JOIN sources s ON s.id = d.source_id
          WHERE d.id = $1 AND s.kind = $2",
    )
    .bind(document_id)
    .bind(MEMORY_SOURCE_KIND)
    .fetch_optional(pool)
    .await?;
    Ok(found.is_some())
}
