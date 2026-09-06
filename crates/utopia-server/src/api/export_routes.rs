//! Export the whole KB as RDF (0020).
//!
//! **Query and stream as you go**: once a page of facts is serialized, the accumulated bytes go
//! out immediately — the whole file is never assembled in memory. A KB with a hundred thousand
//! facts is exactly the kind of KB that most needs exporting, and exactly the kind that "build a
//! String first" would kill the service on.
//!
//! A mid-stream error can only truncate — the HTTP headers went out long ago. So the error goes
//! to the log, and the client gets a file that's cut short; that's still better than buffer-then-send,
//! which couldn't send anything at all on the same KB.

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use utopia_core::models::Role;
use utopia_core::AppError;
use uuid::Uuid;

use super::graph_routes::require_kb;
use crate::auth::AuthUser;
use crate::error::ApiResult;
use crate::rdf::{self, Format, Names, SharedBuf, Sink};
use crate::state::AppState;

#[derive(Deserialize)]
pub struct ExportQuery {
    /// `turtle` (default) or `jsonld`
    #[serde(default)]
    pub format: Option<String>,
    /// External address used to build IRIs. If not given, falls back to URN — stable, and doesn't pretend to know where it's deployed
    #[serde(default)]
    pub base: Option<String>,
}

pub async fn export(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(kb_id): Path<Uuid>,
    Query(q): Query<ExportQuery>,
) -> ApiResult<Response> {
    let kb = require_kb(&state, &user, kb_id, Role::Viewer).await?;
    let format = Format::parse(q.format.as_deref()).ok_or_else(|| {
        AppError::Validation("Unknown `format` (expected turtle or jsonld)".into())
    })?;
    let names = Names::new(kb_id, q.base.as_deref()).map_err(AppError::Validation)?;

    // Exporting is an act of "the whole KB leaving this machine" — the ledger should record it (same rationale as 0014)
    let _ = utopia_store::audit::record(
        &state.pool,
        Some(kb_id),
        user.id,
        "kb.exported",
        "knowledge_base",
        Some(kb_id),
        serde_json::json!({ "format": format.extension() }),
    )
    .await;

    let pool = state.pool.clone();
    let stream = async_stream::try_stream! {
        let buf = SharedBuf::default();
        let mut sink = Sink::new(format, buf.clone());

        let classes = utopia_store::export::classes(&pool, kb_id).await.map_err(io)?;
        let relations = utopia_store::export::relations(&pool, kb_id).await.map_err(io)?;
        let vocab = rdf::vocabulary(&names, &classes, &relations);
        for c in &classes {
            rdf::emit_class(&mut sink, &vocab, c)?;
        }
        for r in &relations {
            rdf::emit_relation(&mut sink, &vocab, r)?;
        }
        yield axum::body::Bytes::from(buf.take());

        let mut after = None;
        loop {
            let page = utopia_store::export::documents_page(&pool, kb_id, after).await.map_err(io)?;
            let Some(last) = page.last() else { break };
            after = Some(last.id);
            for d in &page {
                rdf::emit_document(&mut sink, &names, d)?;
            }
            yield axum::body::Bytes::from(buf.take());
        }

        let mut after = None;
        loop {
            let page = utopia_store::export::entities_page(&pool, kb_id, after).await.map_err(io)?;
            let Some(last) = page.last() else { break };
            after = Some(last.id);
            for e in &page {
                rdf::emit_entity(&mut sink, &names, &vocab, e)?;
            }
            yield axum::body::Bytes::from(buf.take());
        }

        // Current triples are judged as of "the moment of export", and the whole file uses the same now:
        // fetching the current time as we go would write the two halves of the same file against two different nows
        let now = chrono::Utc::now();
        let mut after = None;
        loop {
            let page = utopia_store::export::facts_page(&pool, kb_id, after).await.map_err(io)?;
            let Some(last) = page.last() else { break };
            after = Some(last.id);
            for f in &page {
                rdf::emit_fact(&mut sink, &names, &vocab, f, now)?;
            }
            yield axum::body::Bytes::from(buf.take());
        }

        let mut after = None;
        loop {
            let page = utopia_store::export::derived_page(&pool, kb_id, after).await.map_err(io)?;
            let Some(last) = page.last() else { break };
            after = Some(last.id);
            for d in &page {
                rdf::emit_derived(&mut sink, &names, &vocab, d)?;
            }
            yield axum::body::Bytes::from(buf.take());
        }

        sink.finish()?;
        yield axum::body::Bytes::from(buf.take());
    };

    let filename = format!("{}.{}", slug(&kb.name), format.extension());
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(format.content_type()),
    );
    if let Ok(v) = HeaderValue::from_str(&format!("attachment; filename=\"{filename}\"")) {
        headers.insert(header::CONTENT_DISPOSITION, v);
    }
    Ok((
        StatusCode::OK,
        headers,
        Body::from_stream(Box::pin(stream)
            as std::pin::Pin<
                Box<
                    dyn futures_util::Stream<Item = Result<axum::body::Bytes, std::io::Error>>
                        + Send,
                >,
            >),
    )
        .into_response())
}

/// A KB-side error mid-stream can only become an io error — the response headers already went out, the status code can't change.
fn io(e: AppError) -> std::io::Error {
    tracing::error!(error = %e, "导出中断");
    std::io::Error::other(e.to_string())
}

/// Short name used for the filename. Non-ASCII characters don't belong in the Content-Disposition
/// filename — a Chinese KB name would turn into a string of question marks there
fn slug(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    while out.contains("--") {
        out = out.replace("--", "-");
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "export".into()
    } else {
        trimmed
    }
}
