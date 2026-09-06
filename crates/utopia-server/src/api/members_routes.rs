//! Workspace member management.
//! Rules: viewing members = viewer+; changing role/removing = admin+; granting or revoking the
//! owner role = owner only; a workspace must always keep at least one owner.

use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use utopia_core::models::{MemberView, OrgUser, Role};
use utopia_core::AppError;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::error::ApiResult;
use crate::state::AppState;

pub async fn list(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(workspace_id): Path<Uuid>,
) -> ApiResult<Json<Vec<MemberView>>> {
    utopia_store::workspaces::require_role(&state.pool, user.id, workspace_id, Role::Viewer)
        .await?;
    Ok(Json(
        utopia_store::members::list(&state.pool, workspace_id).await?,
    ))
}

/// Every user in the deployment (for the member picker; visible within the same organization).
pub async fn org_users(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
) -> ApiResult<Json<Vec<OrgUser>>> {
    Ok(Json(
        utopia_store::members::org_users(&state.pool, user.org_id).await?,
    ))
}

/// Deactivated accounts. **Admin-only** — it's a list of "who's been deactivated", and reactivating
/// is itself an admin action.
///
/// Without this endpoint, reactivation is a dead end: a deactivated person disappears from every
/// other list, so an admin has no way to get their id — and that id is exactly what
/// `POST /admin/users/{id}` needs.
pub async fn deactivated_users(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
) -> ApiResult<Json<Vec<OrgUser>>> {
    if !user.is_admin {
        return Err(utopia_core::AppError::Forbidden.into());
    }
    Ok(Json(
        utopia_store::members::deactivated_users(&state.pool, user.org_id).await?,
    ))
}

#[derive(Deserialize)]
pub struct SetRoleReq {
    pub role: String,
}

pub async fn set_role(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((workspace_id, target_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<SetRoleReq>,
) -> ApiResult<Json<serde_json::Value>> {
    let new_role = Role::parse(&req.role).ok_or_else(|| {
        AppError::Validation("Role must be one of owner/admin/editor/viewer".into())
    })?;
    let caller_role =
        utopia_store::workspaces::require_role(&state.pool, user.id, workspace_id, Role::Admin)
            .await?;

    let target_role =
        utopia_store::members::current_role(&state.pool, workspace_id, target_id).await?;

    // Only an owner can grant or revoke the owner role
    let touches_owner = new_role == Role::Owner || target_role == Some(Role::Owner);
    if touches_owner && caller_role != Role::Owner {
        return Err(AppError::Forbidden.into());
    }
    // Can't demote the last remaining owner
    if target_role == Some(Role::Owner)
        && new_role != Role::Owner
        && utopia_store::members::owner_count(&state.pool, workspace_id).await? <= 1
    {
        return Err(AppError::invalid("last_owner_demote", "Cannot demote the last owner").into());
    }

    utopia_store::members::set_role(&state.pool, workspace_id, target_id, new_role).await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn remove(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((workspace_id, target_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<serde_json::Value>> {
    let caller_role =
        utopia_store::workspaces::require_role(&state.pool, user.id, workspace_id, Role::Admin)
            .await?;
    let target_role = utopia_store::members::current_role(&state.pool, workspace_id, target_id)
        .await?
        .ok_or(AppError::NotFound)?;

    if target_role == Role::Owner {
        if caller_role != Role::Owner {
            return Err(AppError::Forbidden.into());
        }
        if utopia_store::members::owner_count(&state.pool, workspace_id).await? <= 1 {
            return Err(
                AppError::invalid("last_owner_remove", "Cannot remove the last owner").into(),
            );
        }
    }

    utopia_store::members::remove(&state.pool, workspace_id, target_id).await?;
    Ok(Json(json!({ "ok": true })))
}
