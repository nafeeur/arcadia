//! Answer-engine data sources: system-level registration (admin, credentials go in but never come out)
//! and KB-level mounting (KB admin). On mount/manual refresh, the target database's schema is
//! rendered as markdown and ingested into the KB (updated in place under the same key), so Chat
//! can retrieve the table structure before writing SQL. The safety gate for query execution lives
//! in gate 2 (the `query_data` tool).

use axum::extract::{Path, State};
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

fn require_admin(user: &utopia_core::models::User) -> Result<(), AppError> {
    if user.is_admin {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

// ---------------------------------------------------------------------------
// System level: register/test/delete
// ---------------------------------------------------------------------------

pub async fn list(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&user)?;
    let sources = utopia_store::datasources::list(&state.pool).await?;
    Ok(Json(json!({ "data_sources": sources })))
}

#[derive(Deserialize)]
pub struct CreateBody {
    pub name: String,
    #[serde(default = "default_engine")]
    pub engine: String,
    pub conn_string: String,
}
fn default_engine() -> String {
    "postgres".into()
}

pub async fn create(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Json(body): Json<CreateBody>,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&user)?;
    // The engine follows the scheme; the UI has just one connection-string field. body.engine is kept only for backward compatibility with old callers
    let engine = crate::query_engine::engine_from_conn(&body.conn_string).ok_or_else(|| {
        utopia_core::AppError::invalid(
            "unsupported_conn_scheme",
            format!(
                "Connection string must start with one of: postgres://, mysql://, trino://, databricks://, snowflake:// (engines: {})",
                crate::query_engine::ENGINES.join(", ")
            ),
        )
    })?;
    let _ = &body.engine;
    // The shape of the connection string is validated at registration time (missing token, missing
    // warehouse, etc.), with the correct syntax included in the error message; otherwise you'd only
    // find out at "test" time, and that step only ever reports back ok:false
    crate::query_engine::engine_for(engine, &body.conn_string)
        .map_err(|e| utopia_core::AppError::invalid("bad_conn_string", e.to_string()))?;
    let id = utopia_store::datasources::create(
        &state.pool,
        &body.name,
        engine,
        &body.conn_string,
        user.id,
    )
    .await?;
    // **Mountable right after registration.** Authorization is per-workspace (0014), but the
    // workspace is already invisible in the UI — a single-tenant deployment has exactly one, and
    // no one has ever seen its name. It used to be that after registering you'd still have to go
    // to the card and "authorize for workspace" first, which meant authorizing something you'd
    // never seen, and mounting would just get "not authorized". So every workspace the registering
    // user belongs to is granted at once; to narrow it, the card still lets you revoke
    for ws in utopia_store::workspaces::list_for_user(&state.pool, user.id).await? {
        utopia_store::datasources::grant(&state.pool, id, ws.id, user.id).await?;
    }
    Ok(Json(json!({ "id": id })))
}

pub async fn delete(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&user)?;
    utopia_store::datasources::delete(&state.pool, id).await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn test(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&user)?;
    let (engine, conn) = utopia_store::datasources::engine_and_conn(&state.pool, id).await?;
    let ok = match crate::query_engine::engine_for(&engine, &conn) {
        Ok(eng) => eng.test().await.is_ok(),
        Err(_) => false,
    };
    utopia_store::datasources::record_test(&state.pool, id, ok).await?;
    Ok(Json(json!({ "ok": ok })))
}

// ---------------------------------------------------------------------------
// KB level: mount/unmount/schema refresh
// ---------------------------------------------------------------------------

pub async fn mounted(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(kb_id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    require_kb(&state, &user, kb_id, Role::Viewer).await?;
    let mounted = utopia_store::datasources::mounted(&state.pool, kb_id).await?;
    Ok(Json(json!({ "data_sources": mounted })))
}

/// What a KB admin can mount (the list gives name/summary, no credentials).
///
/// **Only lists sources granted to this workspace (0014).** This used to return
/// `datasources::list` — every source in the whole deployment — so any KB's admin could see and
/// mount any production database.
pub async fn mountable(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(kb_id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    let kb = require_kb(&state, &user, kb_id, Role::Admin).await?;
    let granted =
        utopia_store::datasources::granted_to_workspace(&state.pool, kb.workspace_id).await?;
    Ok(Json(json!({ "data_sources": granted })))
}

pub async fn mount(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((kb_id, ds_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<serde_json::Value>> {
    require_kb(&state, &user, kb_id, Role::Admin).await?;
    // **Filtering the list isn't a guard.** That only blocks "can see it", while this endpoint is
    // called by id — anyone can construct a uuid themselves and hit it directly. Authorization is checked again right here
    if !utopia_store::datasources::is_granted(&state.pool, kb_id, ds_id).await? {
        return Err(AppError::invalid(
            "source_not_granted",
            "This data source is not available to this workspace",
        )
        .into());
    }
    utopia_store::datasources::mount(&state.pool, kb_id, ds_id).await?;
    // Mounting means ingesting the schema: Chat can retrieve the table structure before writing SQL
    //
    // **A failure here must not be reported as a mount failure.** The line above already wrote to
    // kb_data_sources — the source really is mounted; this used to `?` out to a 500, making people
    // think it failed to mount when it actually did — and the answer engine couldn't see what
    // tables it had. Changed to: report honestly that the mount succeeded, the schema didn't, and
    // report it to the alert center, because after that it's a silent missing-data state (0009)
    match sync_schema_doc(&state, kb_id, ds_id).await {
        Ok(synced) => Ok(Json(json!({ "ok": true, "schema_tables": synced }))),
        Err(e) => {
            let name = source_name(&state, ds_id).await;
            crate::alerting::observe_schema_sync_failure(&state, kb_id, ds_id, &name, &e).await;
            Ok(Json(json!({
                "ok": true,
                "schema_tables": 0,
                "schema_error": e.to_string(),
            })))
        }
    }
}

/// Alerts need to hold onto the name: once a source is deleted, `subject_id` can no longer resolve
/// to a name. When lookup fails, give a placeholder instead of letting the alert itself
/// fail — **a failure on the alerting path shouldn't drown out the thing it's supposed to report**.
async fn source_name(state: &AppState, ds_id: Uuid) -> String {
    utopia_store::datasources::list(&state.pool)
        .await
        .ok()
        .and_then(|all| all.into_iter().find(|d| d.id == ds_id).map(|d| d.name))
        .unwrap_or_else(|| ds_id.to_string())
}

pub async fn unmount(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((kb_id, ds_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<serde_json::Value>> {
    require_kb(&state, &user, kb_id, Role::Admin).await?;
    utopia_store::datasources::unmount(&state.pool, kb_id, ds_id).await?;
    Ok(Json(json!({ "ok": true })))
}

/// Manually refresh the schema.
///
/// **Here the error is still returned to the caller as-is** — the person clicking the button is
/// watching, and nothing was left half-done. But it still reports an alert too: the consequences
/// left behind are identical to a mount failure (source mounted, table structure stale or empty),
/// and the person clicking the button may not be the one who needs to know about it.
pub async fn sync_schema(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((kb_id, ds_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<serde_json::Value>> {
    require_kb(&state, &user, kb_id, Role::Admin).await?;
    match sync_schema_doc(&state, kb_id, ds_id).await {
        Ok(synced) => Ok(Json(json!({ "ok": true, "schema_tables": synced }))),
        Err(e) => {
            let name = source_name(&state, ds_id).await;
            crate::alerting::observe_schema_sync_failure(&state, kb_id, ds_id, &name, &e).await;
            Err(AppError::Other(e).into())
        }
    }
}

/// Agentic exploration: a background job reads the mounted source's schema and proposes
/// metric/dimension -> field mappings (low-confidence ones go to Review).
pub async fn explore(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(kb_id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    require_kb(&state, &user, kb_id, Role::Admin).await?;
    if utopia_store::datasources::mounted(&state.pool, kb_id)
        .await?
        .is_empty()
    {
        return Err(AppError::invalid("no_data_sources", "No data sources mounted").into());
    }
    utopia_store::jobs::enqueue(&state.pool, "explore_mappings", json!({ "kb_id": kb_id })).await?;
    Ok(Json(json!({ "ok": true })))
}

/// Pull information_schema, render it as markdown, and ingest it through the three-way decision
/// (updated in place under the same key). The document is attached under the per-KB "Data schemas" folder source.
async fn sync_schema_doc(state: &AppState, kb_id: Uuid, ds_id: Uuid) -> anyhow::Result<usize> {
    const MAX_TABLES: usize = 200;
    let name = utopia_store::datasources::list(&state.pool)
        .await?
        .into_iter()
        .find(|d| d.id == ds_id)
        .map(|d| d.name)
        .ok_or_else(|| anyhow::anyhow!("Data source not found"))?;
    let (engine, conn) = utopia_store::datasources::engine_and_conn(&state.pool, ds_id).await?;
    let cols = crate::query_engine::engine_for(&engine, &conn)?
        .fetch_schema()
        .await?;

    let mut md = format!(
        "# Data source: {name}\n\nEngine: {engine}. Tables and columns available for SQL queries against this source; write SQL in this engine's dialect.\n"
    );
    let mut current = String::new();
    let mut tables = 0usize;
    for c in &cols {
        let key = format!("{}.{}", c.schema, c.table);
        if key != current {
            if tables >= MAX_TABLES {
                md.push_str("\n(further tables omitted)\n");
                break;
            }
            current = key.clone();
            tables += 1;
            md.push_str(&format!("\n## {key}\n"));
        }
        md.push_str(&format!(
            "- {} ({}){}\n",
            c.column,
            c.data_type,
            c.comment
                .as_deref()
                .map(|x| format!(" — {x}"))
                .unwrap_or_default()
        ));
    }

    // Per-KB "Data schemas" container source (folder: pure container semantics)
    let folder = match sqlx::query_as::<_, (Uuid,)>(
        "SELECT id FROM sources WHERE kb_id = $1 AND kind = 'folder' AND name = 'Data schemas'",
    )
    .bind(kb_id)
    .fetch_optional(&state.pool)
    .await?
    {
        Some((id,)) => id,
        None => {
            utopia_store::sources::create(
                &state.pool,
                kb_id,
                "folder",
                "Data schemas",
                &serde_json::json!({}),
                Some("database"),
                None,
                None,
            )
            .await?
            .id
        }
    };
    crate::ingest_sources::ingest_item(
        state,
        kb_id,
        folder,
        &format!("datasource:{ds_id}:schema"),
        &format!("{name}-schema.md"),
        "text/markdown",
        md.as_bytes(),
        None,
    )
    .await?;
    state.emit_source(kb_id);
    Ok(tables)
}

// ---------------------------------------------------------------------------
// System level: authorization (0014)
//
// Authorization and mounting are two layers, each with its own owner:
//   Authorization = the system admin says "this source can be used by these workspaces"  <- here
//   Mounting = a KB admin says "my KB mounts these ones"                                 <- the group above
// Both layers are many-to-many. Mounting can only pick from the authorized set.
// ---------------------------------------------------------------------------

/// Which workspaces this source is authorized for.
pub async fn grants(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&user)?;
    let rows = utopia_store::datasources::grants_for_source(&state.pool, id).await?;
    let workspaces: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(id, name)| json!({ "id": id, "name": name }))
        .collect();
    Ok(Json(json!({ "workspaces": workspaces })))
}

pub async fn grant(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((id, workspace_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&user)?;
    utopia_store::datasources::grant(&state.pool, id, workspace_id, user.id).await?;
    let _ = utopia_store::audit::record(
        &state.pool,
        None,
        user.id,
        "data_source.granted",
        "data_source",
        Some(id),
        json!({ "workspace_id": workspace_id }),
    )
    .await;
    Ok(Json(json!({ "ok": true })))
}

/// Revoke authorization. **Unmounts anything already mounted in that workspace too** — deleting
/// only the authorization row wouldn't work, since the answer engine still reads
/// `kb_data_sources`, so the revoke wouldn't take effect. Returns how many got unmounted, so the
/// UI can say "also unmounted from 3 KBs" instead of silently cutting someone's connection.
pub async fn revoke(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((id, workspace_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&user)?;
    let unmounted = utopia_store::datasources::revoke(&state.pool, id, workspace_id).await?;
    let _ = utopia_store::audit::record(
        &state.pool,
        None,
        user.id,
        "data_source.revoked",
        "data_source",
        Some(id),
        json!({ "workspace_id": workspace_id, "unmounted": unmounted }),
    )
    .await;
    Ok(Json(json!({ "ok": true, "unmounted": unmounted })))
}
