use axum::extract::{Multipart, Path, Query, State};
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use utopia_core::models::{Document, Role};
use utopia_core::AppError;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::error::ApiResult;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct UploadQuery {
    /// Target folder source: uploads go straight into this folder (only kind=folder accepts uploads)
    #[serde(default)]
    pub source: Option<Uuid>,
}

/// Bulk upload (multipart, can be multiple files). Duplicate content (same KB, same sha256) is skipped.
pub async fn upload(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(kb_id): Path<Uuid>,
    Query(q): Query<UploadQuery>,
    mut multipart: Multipart,
) -> ApiResult<Json<serde_json::Value>> {
    utopia_store::access::require_kb(&state.pool, &user, kb_id, Role::Editor).await?;
    let target_source = match q.source {
        Some(sid) => {
            let src = utopia_store::sources::get(&state.pool, sid).await?;
            if src.kb_id != kb_id || src.kind != "folder" {
                return Err(AppError::invalid(
                    "upload_needs_folder",
                    "Uploads can only target a folder source in this knowledge base",
                )
                .into());
            }
            Some(sid)
        }
        None => None,
    };

    let mut created: Vec<Document> = Vec::new();
    let mut skipped: Vec<serde_json::Value> = Vec::new();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::invalid_detail("bad_upload", "Malformed upload", e.to_string()))?
    {
        let Some(filename) = field.file_name().map(String::from) else {
            continue;
        };
        let mime = field
            .content_type()
            .unwrap_or("application/octet-stream")
            .to_string();
        let bytes = field.bytes().await.map_err(|e| {
            AppError::invalid_detail("upload_read_failed", "Failed to read upload", e.to_string())
        })?;
        if bytes.is_empty() {
            skipped.push(json!({ "filename": filename, "reason": "empty file" }));
            continue;
        }

        let sha256 = hex(&Sha256::digest(&bytes));
        state
            .blob
            .put(&sha256, &bytes)
            .await
            .map_err(AppError::Other)?;

        match utopia_store::documents::create(
            &state.pool,
            kb_id,
            &filename,
            &mime,
            bytes.len() as i64,
            &sha256,
            target_source,
            None,
            None,
        )
        .await
        {
            Ok(doc) => {
                utopia_store::jobs::enqueue(
                    &state.pool,
                    "process_document",
                    json!({ "document_id": doc.id }),
                )
                .await?;
                created.push(doc);
            }
            Err(AppError::Conflict(_)) => {
                skipped.push(json!({ "filename": filename, "reason": "duplicate content" }));
            }
            Err(e) => return Err(e.into()),
        }
    }

    if created.is_empty() && skipped.is_empty() {
        return Err(AppError::invalid("no_files", "No files received").into());
    }
    Ok(Json(json!({ "created": created, "skipped": skipped })))
}

#[derive(serde::Deserialize)]
pub struct DocsQuery {
    /// Source scope: default = all; `none` = documents with no source; otherwise a source id
    #[serde(default)]
    pub source: Option<String>,
    /// Filename contains
    #[serde(default)]
    pub q: Option<String>,
    /// Extraction status: none | queued | extracting | done | failed
    #[serde(default)]
    pub graph: Option<String>,
    /// `deleted` = the "deleted" view: lists only tombstones (#268). Default = live ones
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub offset: Option<i64>,
}

/// A page of the document library.
///
/// **Changed to server-side filtering and pagination**: it used to fetch the whole KB at once and
/// slice on the frontend. Fine for 27 documents, but 20,000 would dump the entire table into the
/// browser; and client-side filtering has a subtler flaw too — it can only filter what's already been fetched.
pub async fn list(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(kb_id): Path<Uuid>,
    Query(q): Query<DocsQuery>,
) -> ApiResult<Json<utopia_core::models::DocumentPage>> {
    utopia_store::access::require_kb(&state.pool, &user, kb_id, Role::Viewer).await?;
    let page = utopia_store::documents::page(
        &state.pool,
        kb_id,
        parse_scope(q.source.as_deref()),
        q.q.as_deref().map(str::trim).filter(|s| !s.is_empty()),
        q.graph.as_deref().filter(|s| !s.is_empty()),
        q.state.as_deref() == Some("deleted"),
        q.limit.unwrap_or(15).clamp(1, 200),
        q.offset.unwrap_or(0).max(0),
    )
    .await?;
    Ok(Json(page))
}

/// `None` = all, `Some(None)` = documents with no source, `Some(Some(id))` = a specific source.
///
/// An unrecognized string is treated as "all" rather than an error: this parameter comes from a
/// single click in the UI, and a single click shouldn't turn the whole page into an error.
fn parse_scope(raw: Option<&str>) -> Option<Option<Uuid>> {
    match raw {
        None | Some("") => None,
        Some("none") => Some(None),
        Some(s) => s.parse().ok().map(Some),
    }
}

/// One-click retry for every extraction-failed document in this scope.
///
/// **Exists because clicking one at a time is too slow**: five failures in one source means five
/// clicks, and failures tend to come in batches (the model endpoint went down for a while, and everything that came in during that window failed).
pub async fn retry_failed(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(kb_id): Path<Uuid>,
    Query(q): Query<DocsQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    utopia_store::access::require_kb(&state.pool, &user, kb_id, Role::Editor).await?;
    let ids =
        utopia_store::documents::failed_ids(&state.pool, kb_id, parse_scope(q.source.as_deref()))
            .await?;
    // Enqueue one at a time rather than a single SQL bulk status change: queuing itself does other
    // things (canceling an in-flight job, clearing incremental markers), which live in
    // `queue_extraction_one` — bypassing it would leave a half-finished state
    let mut queued = 0usize;
    for id in &ids {
        if utopia_store::documents::queue_extraction_one(&state.pool, *id)
            .await
            .is_ok()
        {
            queued += 1;
        }
    }
    if queued > 0 {
        state.emit_document(kb_id, ids[0]);
    }
    Ok(Json(json!({ "queued": queued, "found": ids.len() })))
}

/// Document detail + all chunks (for the document viewer).
pub async fn detail(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    let doc = utopia_store::documents::get(&state.pool, id).await?;
    utopia_store::access::require_kb(&state.pool, &user, doc.kb_id, Role::Viewer).await?;
    let chunks = utopia_store::documents::chunks_full(&state.pool, id).await?;
    Ok(Json(json!({ "document": doc, "chunks": chunks })))
}

/// Reverse evidence chain: the facts extracted from each of the document's chunks (right pane of the document viewer).
pub async fn extractions(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    let doc = utopia_store::documents::get(&state.pool, id).await?;
    utopia_store::access::require_kb(&state.pool, &user, doc.kb_id, Role::Viewer).await?;
    let facts = utopia_store::graph::document_extractions(&state.pool, id).await?;
    Ok(Json(json!({ "facts": facts })))
}

pub async fn delete(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    let doc = utopia_store::documents::get(&state.pool, id).await?;
    utopia_store::access::require_kb(&state.pool, &user, doc.kb_id, Role::Editor).await?;

    // A tombstone, not a subtraction (#268): the document, chunks, evidence, and original file all
    // stay; only facts with no other provenance get invalidated
    let report = utopia_store::documents::delete(&state.pool, doc.kb_id, id, Some(user.id)).await?;
    let search = state.search.clone();
    let did = id.to_string();
    tokio::task::spawn_blocking(move || search.delete_document(&did))
        .await
        .map_err(|e| AppError::Other(e.into()))?
        .map_err(AppError::Other)?;
    // Once a premise is invalidated, the derivations that relied on it are invalidated too — no waiting for the next scheduled rematerialization
    settle_derivations(&state, doc.kb_id).await?;
    let _ = utopia_store::audit::record(
        &state.pool,
        Some(doc.kb_id),
        user.id,
        "document.deleted",
        "document",
        Some(id),
        json!({
            "filename": doc.filename,
            "deletion_id": report.deletion_id,
            "invalidated_facts": report.invalidated_facts,
        }),
    )
    .await;
    state.emit_document(doc.kb_id, id);
    Ok(Json(json!({
        "ok": true,
        "deletion_id": report.deletion_id,
        "invalidated_facts": report.invalidated_facts,
    })))
}

/// Undo a deletion: the document, chunks, and the facts invalidated by that deletion are revived
/// the same way they were removed, and the index is rebuilt.
/// A sync hitting a tombstone and re-uploading the same content go through the same store function; this is just the path a person clicks
pub async fn restore(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    let doc = utopia_store::documents::get(&state.pool, id).await?;
    utopia_store::access::require_kb(&state.pool, &user, doc.kb_id, Role::Editor).await?;
    let doc = utopia_store::documents::restore(&state.pool, doc.kb_id, id).await?;
    reindex(&state, &doc).await?;
    settle_derivations(&state, doc.kb_id).await?;
    let _ = utopia_store::audit::record(
        &state.pool,
        Some(doc.kb_id),
        user.id,
        "document.restored",
        "document",
        Some(id),
        json!({ "filename": doc.filename }),
    )
    .await;
    state.emit_document(doc.kb_id, id);
    Ok(Json(json!({ "ok": true })))
}

/// Hard delete (#268, second half): content is wiped, irreversible, only available for already-deleted
/// documents, and only a KB admin can click it.
/// The KB records it first (purged_at), then the file is deleted: if deleting the file fails, it
/// just leaves behind an orphaned original — the other way around would mean the KB claims "still
/// recoverable" while the original is already gone
pub async fn purge(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    let doc = utopia_store::documents::get(&state.pool, id).await?;
    utopia_store::access::require_kb(&state.pool, &user, doc.kb_id, Role::Admin).await?;
    let report = utopia_store::documents::purge(&state.pool, doc.kb_id, id).await?;
    for sha in &report.blobs {
        if let Err(e) = state.blob.delete(sha).await {
            tracing::warn!(document = %id, sha, error = %e, "purge: blob left behind");
        }
    }
    let _ = utopia_store::audit::record(
        &state.pool,
        Some(doc.kb_id),
        user.id,
        "document.purged",
        "document",
        Some(id),
        json!({
            "filename": doc.filename,
            "chunks": report.chunks,
            "blobs": report.blobs.len(),
        }),
    )
    .await;
    state.emit_document(doc.kb_id, id);
    Ok(Json(
        json!({ "ok": true, "chunks": report.chunks, "blobs": report.blobs.len() }),
    ))
}

/// A revived document goes back into the full-text index: the chunk text was there all along, this just rewrites the index entries
pub async fn reindex(state: &AppState, doc: &Document) -> utopia_core::AppResult<()> {
    let chunks =
        utopia_store::documents::chunks_in_document(&state.pool, doc.kb_id, doc.id).await?;
    let pairs: Vec<(String, String)> = chunks
        .into_iter()
        .map(|c| (c.id.to_string(), c.text))
        .collect();
    let search = state.search.clone();
    let (kb, did) = (doc.kb_id.to_string(), doc.id.to_string());
    tokio::task::spawn_blocking(move || search.reindex_document(&kb, &did, &pairs))
        .await
        .map_err(|e| AppError::Other(e.into()))?
        .map_err(AppError::Other)?;
    Ok(())
}

/// When a premise changes, rematerialize once so derivations catch up — KBs with the switch off
/// don't rematerialize. Shared by the delete, undo, and sync-revival paths
pub(crate) async fn settle_derivations(
    state: &AppState,
    kb_id: Uuid,
) -> utopia_core::AppResult<()> {
    let kb = utopia_store::kbs::get(&state.pool, kb_id).await?;
    if kb.materialize_inferences {
        utopia_store::reasoning::materialize(&state.pool, kb_id).await?;
    }
    Ok(())
}

/// Reprocess (parser upgrade / retry after failure).
pub async fn reprocess(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    let doc = utopia_store::documents::get(&state.pool, id).await?;
    utopia_store::access::require_kb(&state.pool, &user, doc.kb_id, Role::Editor).await?;
    utopia_store::documents::set_status(&state.pool, id, "pending").await?;
    let job_id = utopia_store::jobs::enqueue(
        &state.pool,
        "process_document",
        json!({ "document_id": id }),
    )
    .await?;
    Ok(Json(json!({ "job_id": job_id })))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Extraction-drop signals: which facts got extracted but never landed. Fetched for the whole KB
/// at once — aggregated by (document x reason x specific object) the row count is small, and the
/// Library view can both total them up and expand the detail without firing a request per row.
pub async fn extraction_drops(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(kb_id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    utopia_store::access::require_kb(&state.pool, &user, kb_id, Role::Viewer).await?;
    let drops = utopia_store::extraction_drops::for_kb(&state.pool, kb_id).await?;
    Ok(Json(json!({ "drops": drops })))
}
