//! Issuing and revoking personal access tokens (0014).
//!
//! **These are account-level routes, not knowledge-base-level** — a token belongs to a person, and
//! a person can be in several knowledge bases. Which ones a token itself can reach is governed by
//! its own `kb_ids`; that's a narrowing, not a grant of authorization.

use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::error::ApiResult;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct IssueReq {
    pub name: String,
    /// read | write. Defaults to read-only — letting an agent write to the ledger requires opting in explicitly
    #[serde(default = "default_scope")]
    pub scope: String,
    /// Default = every KB this person can access
    #[serde(default)]
    pub kb_ids: Option<Vec<Uuid>>,
    /// Days until expiry. Defaults to 90; explicitly passing 0 means it never expires
    #[serde(default = "default_days")]
    pub expires_in_days: i64,
}
fn default_scope() -> String {
    "read".into()
}
/// 90 days. **Never-expiring is an option, but not the default** — a key sitting in someone
/// else's notebook is normally forgotten about
fn default_days() -> i64 {
    90
}

/// Issue one. **The plaintext appears only in this one response** — after that, only its hash is stored.
pub async fn issue(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Json(req): Json<IssueReq>,
) -> ApiResult<Json<serde_json::Value>> {
    let expires_at = (req.expires_in_days > 0)
        .then(|| chrono::Utc::now() + chrono::Duration::days(req.expires_in_days));
    let (view, plain) = utopia_store::tokens::issue(
        &state.pool,
        user.id,
        &req.name,
        &req.scope,
        req.kb_ids.as_deref(),
        expires_at,
    )
    .await?;
    let _ = utopia_store::audit::record(
        &state.pool,
        None,
        user.id,
        "token.issued",
        "personal_token",
        Some(view.id),
        json!({ "name": view.name, "scope": view.scope }),
    )
    .await;
    // The `token` field appears exactly this once. The list endpoint can never produce it
    Ok(Json(json!({ "token": plain, "info": view })))
}

pub async fn list(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
) -> ApiResult<Json<serde_json::Value>> {
    let tokens = utopia_store::tokens::list(&state.pool, user.id).await?;
    Ok(Json(json!({ "tokens": tokens })))
}

pub async fn revoke(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(token_id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    utopia_store::tokens::revoke(&state.pool, user.id, token_id).await?;
    let _ = utopia_store::audit::record(
        &state.pool,
        None,
        user.id,
        "token.revoked",
        "personal_token",
        Some(token_id),
        json!({}),
    )
    .await;
    Ok(Json(json!({ "ok": true })))
}
