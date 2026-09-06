//! Arcadia's review workflow. Reads are scoped by the API; private traces additionally
//! carry user scope in every store query. Approval is a single fenced transaction.
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sqlx::PgPool;
use utopia_core::{AppError, AppResult};
use uuid::Uuid;

pub async fn overview(pool: &PgPool, kb: Uuid, user: Uuid) -> AppResult<Value> {
    Ok(sqlx::query_scalar(
        "SELECT jsonb_build_object(
         'documents',(SELECT count(*) FROM documents WHERE kb_id=$1 AND deleted_at IS NULL),
         'facts',(SELECT count(*) FROM facts WHERE kb_id=$1 AND invalidated_at IS NULL),
         'pending_changes',(SELECT count(*) FROM arcadia_changes WHERE kb_id=$1 AND status='pending'),
         'traces',(SELECT count(*) FROM arcadia_traces WHERE kb_id=$1 AND user_id=$2),
         'failed_documents',(SELECT count(*) FROM documents WHERE kb_id=$1 AND deleted_at IS NULL AND (status='failed' OR graph_status='failed')),
         'historical_chunks',(SELECT count(*) FROM chunks WHERE kb_id=$1 AND superseded_at IS NOT NULL))"
    ).bind(kb).bind(user).fetch_one(pool).await?)
}

pub async fn traces(pool: &PgPool, kb: Uuid, user: Uuid, offset: i64) -> AppResult<Vec<Value>> {
    Ok(sqlx::query_scalar(
        "SELECT jsonb_build_object('id',id,'question',question,'answer',left(answer,500),
         'created_at',created_at,'metadata',metadata,'source_count',jsonb_array_length(evidence))
         FROM arcadia_traces WHERE kb_id=$1 AND user_id=$2
         ORDER BY created_at DESC, id DESC LIMIT 50 OFFSET $3",
    )
    .bind(kb)
    .bind(user)
    .bind(offset.clamp(0, 1_000_000))
    .fetch_all(pool)
    .await?)
}

pub async fn trace(pool: &PgPool, kb: Uuid, user: Uuid, id: Uuid) -> AppResult<Value> {
    sqlx::query_scalar(
        "SELECT to_jsonb(t) FROM arcadia_traces t WHERE kb_id=$1 AND user_id=$2 AND id=$3",
    )
    .bind(kb)
    .bind(user)
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)
}

pub async fn impact(pool: &PgPool, kb: Uuid, user: Uuid, doc: Uuid) -> AppResult<Value> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM documents WHERE id=$1 AND kb_id=$2 AND purged_at IS NULL)",
    )
    .bind(doc)
    .bind(kb)
    .fetch_one(pool)
    .await?;
    if !exists {
        return Err(AppError::NotFound);
    }
    // These are dependencies, not a claim that all conclusions will change.
    let facts: Vec<Value> = sqlx::query_scalar(
        "SELECT DISTINCT jsonb_build_object('id',f.id,'subject',e.canonical_name,'predicate',coalesce(r.label,'Unclassified'),
          'object',coalesce(o.canonical_name,f.object_value::text),'confidence',f.confidence)
         FROM fact_evidence fe JOIN chunks c ON c.id=fe.chunk_id JOIN facts f ON f.id=fe.fact_id
         JOIN entities e ON e.id=f.subject_id LEFT JOIN entities o ON o.id=f.object_id
         LEFT JOIN relation_types r ON r.id=f.predicate_id
         WHERE c.document_id=$1 AND f.kb_id=$2 AND f.invalidated_at IS NULL LIMIT 200"
    ).bind(doc).bind(kb).fetch_all(pool).await?;
    let derived: i64 = sqlx::query_scalar(
        "SELECT count(DISTINCT fd.derived_fact_id) FROM fact_derivations fd
         JOIN derived_facts df ON df.id=fd.derived_fact_id JOIN fact_evidence fe ON fe.fact_id=fd.premise_fact_id
         JOIN chunks c ON c.id=fe.chunk_id WHERE c.document_id=$1 AND df.kb_id=$2 AND df.invalidated_at IS NULL"
    ).bind(doc).bind(kb).fetch_one(pool).await?;
    let answers: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('id',id,'question',question,'created_at',created_at)
         FROM arcadia_traces WHERE kb_id=$1 AND user_id=$2
         AND evidence @> jsonb_build_array(jsonb_build_object('document_id',$3::uuid))
         ORDER BY created_at DESC LIMIT 200",
    )
    .bind(kb)
    .bind(user)
    .bind(doc)
    .fetch_all(pool)
    .await?;
    Ok(
        json!({"facts":facts,"derived_count":derived,"answers":answers,
        "scope":"potential_dependencies","facts_capped":facts.len()==200,"answers_capped":answers.len()==200}),
    )
}

pub async fn changes(pool: &PgPool, kb: Uuid) -> AppResult<Vec<Value>> {
    Ok(sqlx::query_scalar(
        "SELECT (to_jsonb(c)-'content'-'base_content') || jsonb_build_object('filename',d.filename,
         'stale',c.base_sha<>d.sha256 OR c.base_updated_at<>d.updated_at)
         FROM arcadia_changes c JOIN documents d ON d.id=c.document_id
         WHERE c.kb_id=$1 ORDER BY c.created_at DESC LIMIT 100",
    )
    .bind(kb)
    .fetch_all(pool)
    .await?)
}

pub async fn change(pool: &PgPool, kb: Uuid, id: Uuid) -> AppResult<Value> {
    sqlx::query_scalar(
        "SELECT to_jsonb(c) || jsonb_build_object('filename',d.filename,
         'stale',c.base_sha<>d.sha256 OR c.base_updated_at<>d.updated_at,
         'before',c.base_content)
         FROM arcadia_changes c JOIN documents d ON d.id=c.document_id WHERE c.kb_id=$1 AND c.id=$2"
    ).bind(kb).bind(id).fetch_optional(pool).await?.ok_or(AppError::NotFound)
}

pub async fn propose(
    pool: &PgPool,
    kb: Uuid,
    user: Uuid,
    doc: Uuid,
    title: &str,
    reason: &str,
    content: &str,
) -> AppResult<Uuid> {
    if title.trim().is_empty()
        || title.chars().count() > 160
        || reason.trim().is_empty()
        || reason.len() > 10000
        || content.trim().is_empty()
        || content.len() > 500000
    {
        return Err(AppError::invalid(
            "invalid_proposal",
            "Title, reason and replacement text are required (text limit 500 KB)",
        ));
    }
    let id = Uuid::now_v7();
    let n=sqlx::query("INSERT INTO arcadia_changes(id,kb_id,document_id,proposed_by,title,reason,content,base_content,base_sha,base_updated_at)
        SELECT $1,$2,id,$3,$4,$5,$6,COALESCE((SELECT string_agg(text,E'\n\n' ORDER BY seq) FROM chunks WHERE document_id=documents.id AND superseded_at IS NULL),''),sha256,updated_at FROM documents
        WHERE id=$7 AND kb_id=$2 AND deleted_at IS NULL AND status='ready' AND graph_status NOT IN ('queued','extracting')")
        .bind(id).bind(kb).bind(user).bind(title.trim()).bind(reason.trim()).bind(content)
        .bind(doc).execute(pool).await?.rows_affected();
    if n == 0 {
        return Err(AppError::invalid(
            "document_not_ready",
            "Choose a ready document with no extraction in progress",
        ));
    }
    Ok(id)
}

pub async fn decide(
    pool: &PgPool,
    kb: Uuid,
    user: Uuid,
    id: Uuid,
    approve: bool,
    note: &str,
    sha: &str,
) -> AppResult<()> {
    let mut tx = pool.begin().await?;
    let row: Option<(Uuid, String, DateTime<Utc>, String)> = sqlx::query_as(
        "SELECT document_id,base_sha,base_updated_at,content FROM arcadia_changes
         WHERE id=$1 AND kb_id=$2 AND status='pending' FOR UPDATE",
    )
    .bind(id)
    .bind(kb)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((doc, base, base_time, content)) = row else {
        return Err(AppError::invalid(
            "change_decided",
            "This proposal is no longer pending",
        ));
    };
    if approve {
        let current: Option<(String,DateTime<Utc>)>=sqlx::query_as(
            "SELECT sha256,updated_at FROM documents WHERE id=$1 AND kb_id=$2 AND deleted_at IS NULL
             AND status='ready' AND graph_status NOT IN ('queued','extracting') FOR UPDATE")
            .bind(doc).bind(kb).fetch_optional(&mut *tx).await?;
        if current != Some((base, base_time)) {
            return Err(AppError::invalid(
                "stale_proposal",
                "The document changed or is processing. Create a fresh proposal.",
            ));
        }
        sqlx::query("UPDATE documents SET sha256=$2,size_bytes=$3,mime='text/plain',status='pending',
            graph_status='none',error=NULL,graph_error=NULL,extract_epoch=extract_epoch+1,updated_at=now() WHERE id=$1")
            .bind(doc).bind(sha).bind(content.len() as i64).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO document_versions(id,document_id,version,sha256,size_bytes)
            VALUES($1,$2,(SELECT coalesce(max(version),0)+1 FROM document_versions WHERE document_id=$2),$3,$4)")
            .bind(Uuid::now_v7()).bind(doc).bind(sha).bind(content.len() as i64).execute(&mut *tx).await?;
        crate::jobs::enqueue_with_max_attempts_tx(
            &mut tx,
            "process_document",
            json!({"document_id":doc}),
            3,
        )
        .await?;
    }
    sqlx::query("UPDATE arcadia_changes SET status=$3,decided_by=$4,decision_note=$5,decided_at=now() WHERE id=$1 AND kb_id=$2")
        .bind(id).bind(kb).bind(if approve {"approved"} else {"rejected"}).bind(user).bind(note)
        .execute(&mut *tx).await?;
    crate::audit::record_tx(
        &mut tx,
        Some(kb),
        user,
        if approve {
            "arcadia.change_approved"
        } else {
            "arcadia.change_rejected"
        },
        "arcadia_change",
        Some(id),
        json!({"document_id":doc,"processing_queued":approve}),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn replays(pool: &PgPool, trace: Uuid) -> AppResult<Vec<Value>> {
    Ok(sqlx::query_scalar("SELECT to_jsonb(r) FROM arcadia_replays r WHERE trace_id=$1 ORDER BY created_at DESC LIMIT 20")
        .bind(trace).fetch_all(pool).await?)
}

/// Serialize copied evidence with physical purge; reject an in-flight answer if its
/// source disappeared. Stable lock ordering prevents competing answers deadlocking.
pub async fn lock_evidence_sources(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    evidence: &Value,
) -> AppResult<()> {
    let mut ids: Vec<Uuid> = evidence
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| e["document_id"].as_str().and_then(|s| s.parse().ok()))
        .collect();
    ids.sort();
    ids.dedup();
    for id in ids {
        let live: Option<bool> =
            sqlx::query_scalar("SELECT purged_at IS NULL FROM documents WHERE id=$1 FOR SHARE")
                .bind(id)
                .fetch_optional(&mut **tx)
                .await?;
        if live != Some(true) {
            return Err(AppError::Validation("An evidence source was purged while the answer was running. Run the question again.".into()));
        }
    }
    Ok(())
}
