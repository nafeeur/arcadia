//! CRUD for business rules (0021 / #277).
//!
//! **Writing a rule requires Editor**: it's part of the ontology — changing one rule once
//! changes conclusions across the whole KB, the same order of magnitude as changing an axiom.

use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use utopia_core::models::Role;
use utopia_store::business_rules::{ConclusionInput, ConditionInput};
use uuid::Uuid;

use super::graph_routes::require_kb;
use crate::auth::AuthUser;
use crate::error::ApiResult;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct RuleReq {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub subject_type_id: Uuid,
    /// typing | attribute
    pub conclusion: String,
    #[serde(default)]
    pub conclude_type_id: Option<Uuid>,
    #[serde(default)]
    pub conclude_predicate_id: Option<Uuid>,
    #[serde(default)]
    pub conclude_value: Option<serde_json::Value>,
    pub conditions: Vec<ConditionInput>,
}

#[derive(Deserialize)]
pub struct RulePatch {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    /// If given, replaces the whole set; if not, conditions are left untouched
    #[serde(default)]
    pub conditions: Option<Vec<ConditionInput>>,
    /// The conclusion is also replaced as a whole: the three fields define each other, changing only one leaves a half-finished state
    #[serde(default)]
    pub conclusion: Option<String>,
    #[serde(default)]
    pub conclude_type_id: Option<Uuid>,
    #[serde(default)]
    pub conclude_predicate_id: Option<Uuid>,
    #[serde(default)]
    pub conclude_value: Option<serde_json::Value>,
}

pub async fn list(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(kb_id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    require_kb(&state, &user, kb_id, Role::Viewer).await?;
    let rules = utopia_store::business_rules::list(&state.pool, kb_id).await?;
    Ok(Json(json!({ "rules": rules })))
}

pub async fn create(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(kb_id): Path<Uuid>,
    Json(req): Json<RuleReq>,
) -> ApiResult<Json<serde_json::Value>> {
    require_kb(&state, &user, kb_id, Role::Editor).await?;
    let id = utopia_store::business_rules::create(
        &state.pool,
        kb_id,
        &req.name,
        &req.description,
        req.subject_type_id,
        &req.conclusion,
        req.conclude_type_id,
        req.conclude_predicate_id,
        req.conclude_value.clone(),
        &req.conditions,
    )
    .await?;
    let _ = utopia_store::audit::record(
        &state.pool,
        Some(kb_id),
        user.id,
        "rule.created",
        "attribute_rule",
        Some(id),
        json!({ "name": req.name, "conclusion": req.conclusion }),
    )
    .await;
    Ok(Json(json!({ "id": id })))
}

pub async fn update(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((kb_id, rule_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<RulePatch>,
) -> ApiResult<Json<serde_json::Value>> {
    require_kb(&state, &user, kb_id, Role::Editor).await?;
    let conclusion = req.conclusion.as_ref().map(|kind| ConclusionInput {
        kind: kind.clone(),
        type_id: req.conclude_type_id,
        predicate_id: req.conclude_predicate_id,
        value: req.conclude_value.clone(),
    });
    utopia_store::business_rules::update(
        &state.pool,
        kb_id,
        rule_id,
        req.name.as_deref(),
        req.description.as_deref(),
        req.enabled,
        req.conditions.as_deref(),
        conclusion.as_ref(),
    )
    .await?;
    let _ = utopia_store::audit::record(
        &state.pool,
        Some(kb_id),
        user.id,
        "rule.updated",
        "attribute_rule",
        Some(rule_id),
        json!({ "enabled": req.enabled, "conditions_replaced": req.conditions.is_some() }),
    )
    .await;
    Ok(Json(json!({ "ok": true })))
}

pub async fn delete(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((kb_id, rule_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<serde_json::Value>> {
    require_kb(&state, &user, kb_id, Role::Editor).await?;
    utopia_store::business_rules::delete(&state.pool, kb_id, rule_id).await?;
    let _ = utopia_store::audit::record(
        &state.pool,
        Some(kb_id),
        user.id,
        "rule.deleted",
        "attribute_rule",
        Some(rule_id),
        json!({}),
    )
    .await;
    Ok(Json(json!({ "ok": true })))
}

/// Who a rule currently flags. Clicking that number in the UI opens exactly this list.
pub async fn matches(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((kb_id, rule_id)): Path<(Uuid, Uuid)>,
    axum::extract::Query(q): axum::extract::Query<MatchQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    require_kb(&state, &user, kb_id, Role::Viewer).await?;
    let per = q.per.unwrap_or(20).clamp(1, 100);
    let page = q.page.unwrap_or(0).max(0);
    let (rows, total) =
        utopia_store::business_rules::matches(&state.pool, kb_id, rule_id, per, page * per).await?;
    Ok(Json(json!({ "matches": rows, "total": total })))
}

#[derive(Deserialize)]
pub struct MatchQuery {
    #[serde(default)]
    pub page: Option<i64>,
    #[serde(default)]
    pub per: Option<i64>,
}

/// Run right now, computing this rule's conclusions.
///
/// **Doesn't wait for the next materialization cycle**: whoever wrote the rule wants to see
/// what it derives immediately, and the cycle defaults to an hour. Goes through the same
/// `materialize` — preview and the real thing share one path, so there's no separate copy of the logic to drift.
pub async fn run_now(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(kb_id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    require_kb(&state, &user, kb_id, Role::Editor).await?;
    let report = utopia_store::reasoning::materialize(&state.pool, kb_id).await?;
    state.emit_graph(kb_id);
    Ok(Json(json!({
        "rules": report.attribute_rules,
        "hits": report.rule_hits,
        "capped": report.rule_capped,
        "inserted": report.inserted,
        "invalidated": report.invalidated,
    })))
}
