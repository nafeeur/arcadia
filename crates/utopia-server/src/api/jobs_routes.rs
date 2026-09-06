//! Requeue failed jobs (#216).
//!
//! A failed job could otherwise only be rerun through the object it belongs to: a document can be
//! re-extracted, a source can be re-synced; but `bootstrap_ontology` and `adjudicate_entities` have no
//! object to click. Running out of balance (#201 makes that fail on the very first attempt) stops a
//! whole batch of documents at once, and after topping up they'd need to be clicked one by one. This
//! gives two entry points instead: within a KB (Editor) and global (admin), scoped down by kind and
//! failure time — the "run it again" button on an alert passes exactly that failure's time window.

use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use utopia_core::models::Role;
use utopia_store::jobs::RequeueScope;
use uuid::Uuid;

use super::graph_routes::require_kb;
use crate::auth::AuthUser;
use crate::error::ApiResult;
use crate::state::AppState;

// Both API scopes must apply the same retry policies and report the combined total.
async fn requeue_failed(
    pool: &sqlx::PgPool,
    scope: RequeueScope<'_>,
) -> utopia_core::AppResult<u64> {
    let generic = utopia_store::jobs::requeue_failed(pool, scope).await?;
    let rss = utopia_store::rss_full_content::requeue_failed(pool, scope).await?;
    Ok(generic + rss)
}

#[cfg(test)]
#[path = "jobs_routes_tests.rs"]
mod tests;

#[derive(Deserialize, Default)]
pub struct RequeueBody {
    #[serde(default)]
    pub kind: Option<String>,
    /// Only requeue jobs that failed after this moment
    #[serde(default)]
    pub failed_since: Option<chrono::DateTime<chrono::Utc>>,
}

pub async fn failed_in_kb(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(kb_id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    require_kb(&state, &user, kb_id, Role::Viewer).await?;
    let failed = utopia_store::jobs::failed_count(&state.pool, Some(kb_id)).await?;
    Ok(Json(json!({ "failed": failed })))
}

pub async fn requeue_in_kb(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(kb_id): Path<Uuid>,
    Json(body): Json<RequeueBody>,
) -> ApiResult<Json<serde_json::Value>> {
    require_kb(&state, &user, kb_id, Role::Editor).await?;
    let requeued = requeue_failed(
        &state.pool,
        RequeueScope {
            kb_id: Some(kb_id),
            kind: body.kind.as_deref(),
            failed_since: body.failed_since,
        },
    )
    .await?;
    let _ = utopia_store::audit::record(
        &state.pool,
        Some(kb_id),
        user.id,
        "jobs.requeued",
        "kb",
        Some(kb_id),
        json!({ "requeued": requeued, "kind": body.kind, "failed_since": body.failed_since }),
    )
    .await;
    Ok(Json(json!({ "requeued": requeued })))
}

/// Global requeue: system-level alerts (not tied to a KB) go through here. Admin-only — it touches jobs across every KB.
pub async fn requeue_all(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Json(body): Json<RequeueBody>,
) -> ApiResult<Json<serde_json::Value>> {
    if !user.is_admin {
        return Err(utopia_core::AppError::Forbidden.into());
    }
    let requeued = requeue_failed(
        &state.pool,
        RequeueScope {
            kb_id: None,
            kind: body.kind.as_deref(),
            failed_since: body.failed_since,
        },
    )
    .await?;
    let _ = utopia_store::audit::record(
        &state.pool,
        None,
        user.id,
        "jobs.requeued",
        "system",
        None,
        json!({ "requeued": requeued, "kind": body.kind, "failed_since": body.failed_since }),
    )
    .await;
    Ok(Json(json!({ "requeued": requeued })))
}
