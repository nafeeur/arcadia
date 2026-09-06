//! Data mapping API: listing metric definitions, revising them, and revision history.
//!
//! Approval (`decide`) stays in `review_routes` — that endpoint already writes the audit trail
//! (`mapping.decided`); just call it from a different screen. No reason to move the endpoint just to move the page.

use axum::extract::{Path, Query, State};
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use utopia_core::models::Role;
use utopia_core::AppError;
use uuid::Uuid;

use super::graph_routes::require_kb;
use crate::auth::AuthUser;
use crate::error::ApiResult;
use crate::state::AppState;

/// Items per page. Metric definitions are denser than the review queue (one row = one definition), so a page can afford 25
const MAPPING_PAGE: i64 = 25;

#[derive(Deserialize)]
pub struct ListQuery {
    /// proposed | confirmed | rejected; default = all
    status: Option<String>,
    q: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

/// A page of metric definitions + counts for each of the three statuses.
///
/// **Viewer-readable.** A metric definition is "how this number is computed", and it directly
/// determines the answer to any question about that number — being able to see the answer but
/// not the definition means trusting an algorithm you're not allowed to see. Only editing
/// (`revise`) requires Editor.
pub async fn list(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(kb_id): Path<Uuid>,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    require_kb(&state, &user, kb_id, Role::Viewer).await?;
    let status = q.status.as_deref().filter(|s| !s.is_empty());
    if let Some(s) = status {
        if !matches!(s, "proposed" | "confirmed" | "rejected") {
            return Err(AppError::invalid(
                "bad_status",
                "status must be proposed, confirmed or rejected",
            )
            .into());
        }
    }
    let needle = q.q.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let limit = q.limit.unwrap_or(MAPPING_PAGE).clamp(1, 200);
    let offset = q.offset.unwrap_or(0).max(0);

    let (items, total) =
        utopia_store::mappings::page(&state.pool, kb_id, status, needle, limit, offset).await?;
    let (proposed, confirmed, rejected) =
        utopia_store::mappings::status_counts(&state.pool, kb_id).await?;
    Ok(Json(json!({
        "items": items,
        "total": total,
        "counts": { "proposed": proposed, "confirmed": confirmed, "rejected": rejected },
    })))
}

#[derive(Deserialize)]
pub struct ReviseReq {
    table_name: Option<String>,
    expr: Option<String>,
    sql: Option<String>,
    unit: Option<String>,
    summary: Option<String>,
    #[serde(default)]
    derived: bool,
}

/// Revise a metric definition.
///
/// **Before this, `mappings::revise` had zero callers**: the function existed, the trail table
/// existed, but there was no route. So once a definition was confirmed, no one could ever change
/// it again, and no one could see it either — the answer engine computed against it, but people couldn't reach it.
pub async fn revise(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((kb_id, mapping_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<ReviseReq>,
) -> ApiResult<Json<serde_json::Value>> {
    require_kb(&state, &user, kb_id, Role::Editor).await?;
    // Blank counts as unfilled: when the frontend clears an input field it sends "", but that
    // should land as NULL, not an empty string — otherwise "is expr configured" has to check both IS NULL and = ''
    let clean = |s: &Option<String>| -> Option<String> {
        s.as_deref()
            .map(str::trim)
            .filter(|x| !x.is_empty())
            .map(str::to_string)
    };
    let (table_name, expr, sql, unit, summary) = (
        clean(&req.table_name),
        clean(&req.expr),
        clean(&req.sql),
        clean(&req.unit),
        clean(&req.summary),
    );
    if table_name.is_none() && expr.is_none() && sql.is_none() {
        return Err(AppError::invalid(
            "empty_mapping",
            "A mapping needs at least one of table, expression or SQL",
        )
        .into());
    }
    utopia_store::mappings::revise(
        &state.pool,
        kb_id,
        mapping_id,
        table_name.as_deref(),
        expr.as_deref(),
        sql.as_deref(),
        unit.as_deref(),
        summary.as_deref(),
        req.derived,
        user.id,
    )
    .await?;
    let _ = utopia_store::audit::record(
        &state.pool,
        Some(kb_id),
        user.id,
        "mapping.revised",
        "concept_mapping",
        Some(mapping_id),
        json!({}),
    )
    .await;
    Ok(Json(json!({ "ok": true })))
}

/// Revision history for one metric definition. 0006 says the trail exists so we can answer
/// "how was this number computed last quarter" — this is where that promise is kept.
pub async fn revisions(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((kb_id, mapping_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<serde_json::Value>> {
    require_kb(&state, &user, kb_id, Role::Viewer).await?;
    let rows = utopia_store::mappings::revisions(&state.pool, kb_id, mapping_id).await?;
    Ok(Json(json!({ "revisions": rows })))
}
