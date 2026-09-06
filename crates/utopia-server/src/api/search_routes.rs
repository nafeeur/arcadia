use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use utopia_core::models::Role;
use utopia_core::AppError;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::error::ApiResult;
use crate::retrieval;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct SearchReq {
    pub q: String,
    #[serde(default)]
    pub top_k: Option<usize>,
    /// Record axis (0019): search **what the base had at that moment**. YYYY-MM-DD or RFC3339.
    /// Hits are chunks alive then, in documents not yet deleted then.
    #[serde(default)]
    pub as_of: Option<String>,
}

pub async fn search(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(kb_id): Path<Uuid>,
    Json(req): Json<SearchReq>,
) -> ApiResult<Json<serde_json::Value>> {
    let q = req.q.trim();
    if q.is_empty() {
        return Err(AppError::invalid("empty_query", "Search query cannot be empty").into());
    }
    let kb = utopia_store::access::require_kb(&state.pool, &user, kb_id, Role::Viewer).await?;

    let top_k = req.top_k.unwrap_or(10).min(50);
    let as_of = super::graph_routes::parse_instant("as_of", req.as_of.as_deref())?;
    let chunks = retrieval::hybrid(&state, kb_id, kb.workspace_id, q, top_k, as_of).await?;
    Ok(Json(json!({ "results": chunks })))
}
