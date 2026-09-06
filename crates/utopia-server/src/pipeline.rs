//! Ingest pipeline: parse → chunk → full-text index → embedding (optional) → ready.
//! Every step is idempotent: rerunning clears old chunks and old index entries first.

use crate::llm_util;
use crate::state::AppState;
use utopia_core::models::Proposer;
use uuid::Uuid;

const EMBED_BATCH: usize = 16;

pub async fn process_document(state: &AppState, document_id: Uuid) -> anyhow::Result<()> {
    match run(state, document_id).await {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ =
                utopia_store::documents::set_failed(&state.pool, document_id, &e.to_string()).await;
            if let Ok(doc) = utopia_store::documents::get(&state.pool, document_id).await {
                state.emit_document(doc.kb_id, document_id);
            }
            Err(e)
        }
    }
}

async fn run(state: &AppState, document_id: Uuid) -> anyhow::Result<()> {
    let doc = utopia_store::documents::get(&state.pool, document_id).await?;
    // Deleted after being queued (#268): tombstones don't get chunks rebuilt or re-indexed;
    // once cleared, even the source text is gone
    if doc.deleted_at.is_some() {
        tracing::info!(document = %document_id, "skipping a deleted document");
        return Ok(());
    }

    // 1. Parse (CPU-intensive, run on a blocking thread)
    utopia_store::documents::set_status(&state.pool, document_id, "parsing").await?;
    state.emit_document(doc.kb_id, document_id);
    let bytes = state.blob.get(&doc.sha256).await?;
    let filename = doc.filename.clone();
    let parsed =
        tokio::task::spawn_blocking(move || utopia_ingest::parse(&filename, &bytes)).await??;
    let text_len = parsed.text.chars().count() as i32;

    // 2. Chunk + persist
    let pieces = utopia_ingest::chunk_text(&parsed.text);
    let chunk_pairs =
        utopia_store::documents::replace_chunks(&state.pool, doc.kb_id, document_id, &pieces)
            .await?;
    let chunk_count = chunk_pairs.len() as i32;

    // 3. Full-text index (Tantivy)
    utopia_store::documents::set_status(&state.pool, document_id, "indexing").await?;
    state.emit_document(doc.kb_id, document_id);
    let search = state.search.clone();
    let kb = doc.kb_id.to_string();
    let did = document_id.to_string();
    tokio::task::spawn_blocking(move || search.reindex_document(&kb, &did, &chunk_pairs)).await??;

    // 4. embedding (only if the workspace has an embedding model configured; without one
    // it still counts as ready — you get BM25 search in the meantime)
    let kb_row = utopia_store::kbs::get(&state.pool, doc.kb_id).await?;
    let settings = utopia_store::settings::get(&state.pool, kb_row.workspace_id).await?;
    if let Some(client) = settings.as_ref().and_then(llm_util::embed_client) {
        utopia_store::documents::set_status(&state.pool, document_id, "embedding").await?;
        state.emit_document(doc.kb_id, document_id);
        let pending =
            utopia_store::documents::chunks_pending_embedding(&state.pool, document_id).await?;
        for batch in pending.chunks(EMBED_BATCH) {
            let texts: Vec<String> = batch.iter().map(|(_, t)| t.clone()).collect();
            let _permit = match settings.as_ref() {
                Some(s) => llm_util::acquire_embed(state, s).await,
                None => None,
            };
            let embeddings = client.embed(&texts).await?;
            if embeddings.len() != batch.len() {
                anyhow::bail!("Embedding 返回数量不匹配");
            }
            let items: Vec<(Uuid, Vec<f32>)> =
                batch.iter().map(|(id, _)| *id).zip(embeddings).collect();
            utopia_store::documents::set_embeddings(&state.pool, &items).await?;
        }
    }

    utopia_store::documents::set_ready(&state.pool, document_id, text_len, chunk_count).await?;

    // Two-phase: once the index is ready, queue graph extraction if a chat model is
    // configured (doesn't block search/ask availability)
    if settings.as_ref().is_some_and(|s| s.chat_ready()) {
        utopia_store::documents::set_graph_status(&state.pool, document_id, "queued").await?;
        utopia_store::jobs::enqueue(
            &state.pool,
            "extract_document",
            serde_json::json!({ "document_id": document_id }),
        )
        .await?;
    }
    state.emit_document(doc.kb_id, document_id);

    tracing::info!(%document_id, chunks = chunk_count, "文档处理完成");
    Ok(())
}

/// Memory ingest (second half of the episodes fast path): backfills embeddings for new
/// episode chunks, rebuilds the full-text index, and triggers incremental extraction (only
/// new chunks with an empty `extracted_at` get extracted).
/// No parsing, no chunking needed — an episode is already a chunk by the time it's persisted.
///
/// `proposer`: whoever said this, and the agent acting through MCP if any. Carried all the
/// way to extraction, landing in `pending_facts.proposed_by` / `proposed_token` (0015, 0026)
pub async fn memory_ingest(
    state: &AppState,
    document_id: Uuid,
    proposer: Proposer,
) -> anyhow::Result<()> {
    let doc = utopia_store::documents::get(&state.pool, document_id).await?;
    if doc.deleted_at.is_some() {
        tracing::info!(document = %document_id, "skipping a deleted document");
        return Ok(());
    }
    let kb_row = utopia_store::kbs::get(&state.pool, doc.kb_id).await?;
    let settings = utopia_store::settings::get(&state.pool, kb_row.workspace_id).await?;

    if let Some(client) = settings.as_ref().and_then(llm_util::embed_client) {
        let pending =
            utopia_store::documents::chunks_pending_embedding(&state.pool, document_id).await?;
        for batch in pending.chunks(EMBED_BATCH) {
            let texts: Vec<String> = batch.iter().map(|(_, t)| t.clone()).collect();
            let _permit = match settings.as_ref() {
                Some(s) => llm_util::acquire_embed(state, s).await,
                None => None,
            };
            let embeddings = client.embed(&texts).await?;
            if embeddings.len() != batch.len() {
                anyhow::bail!("Embedding 返回数量不匹配");
            }
            let items: Vec<(Uuid, Vec<f32>)> =
                batch.iter().map(|(id, _)| *id).zip(embeddings).collect();
            utopia_store::documents::set_embeddings(&state.pool, &items).await?;
        }
    }

    let chunks = utopia_store::documents::chunks_full(&state.pool, document_id).await?;
    let pairs: Vec<(String, String)> = chunks
        .iter()
        .map(|c| (c.id.to_string(), c.text.clone()))
        .collect();
    let search = state.search.clone();
    let kb = doc.kb_id.to_string();
    let did = document_id.to_string();
    tokio::task::spawn_blocking(move || search.reindex_document(&kb, &did, &pairs)).await??;

    if settings.as_ref().is_some_and(|s| s.chat_ready()) {
        utopia_store::documents::set_graph_status(&state.pool, document_id, "queued").await?;
        utopia_store::jobs::enqueue(
            &state.pool,
            "extract_document",
            serde_json::json!({
                "document_id": document_id,
                "proposed_by": proposer.user_id,
                "proposed_token": proposer.token_id,
            }),
        )
        .await?;
    }
    state.emit_document(doc.kb_id, document_id);
    Ok(())
}
