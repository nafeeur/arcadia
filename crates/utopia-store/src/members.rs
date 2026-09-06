use sqlx::PgPool;
use utopia_core::models::{MemberView, OrgUser, Role};
use utopia_core::{AppError, AppResult};
use uuid::Uuid;

pub async fn list(pool: &PgPool, workspace_id: Uuid) -> AppResult<Vec<MemberView>> {
    let rows = sqlx::query_as(
        "SELECT m.user_id, u.email, u.display_name, m.role, u.is_admin
         FROM memberships m JOIN users u ON u.id = m.user_id
         -- Deactivated people no longer appear in the member list (see `users.deactivated_at`).
         -- The membership row itself is left in place — reactivating an account shouldn't
         -- require re-adding it to every workspace
         WHERE m.workspace_id = $1 AND u.deactivated_at IS NULL
         ORDER BY m.created_at",
    )
    .bind(workspace_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// All users in the deployment (the picker for adding a member).
pub async fn org_users(pool: &PgPool, org_id: Uuid) -> AppResult<Vec<OrgUser>> {
    let rows = sqlx::query_as(
        "SELECT id, email, display_name, is_admin FROM users
         WHERE org_id = $1 AND deactivated_at IS NULL ORDER BY created_at",
    )
    .bind(org_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn owner_count(pool: &PgPool, workspace_id: Uuid) -> AppResult<i64> {
    let (n,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM memberships WHERE workspace_id = $1 AND role = 'owner'",
    )
    .bind(workspace_id)
    .fetch_one(pool)
    .await?;
    Ok(n)
}

pub async fn current_role(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
) -> AppResult<Option<Role>> {
    crate::workspaces::role_of(pool, user_id, workspace_id).await
}

/// Set/add a member's role (upsert). Guard-rail logic lives in the API layer.
pub async fn set_role(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
    role: Role,
) -> AppResult<()> {
    // The target user must exist in this organization **and be active** — otherwise a
    // deactivated account could be added to a workspace while remaining invisible in the
    // member list (which filters out deactivated users), becoming a grant nobody can discover
    let exists: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM users WHERE id = $1 AND deactivated_at IS NULL")
            .bind(user_id)
            .fetch_optional(pool)
            .await?;
    if exists.is_none() {
        return Err(AppError::NotFound);
    }
    sqlx::query(
        "INSERT INTO memberships (user_id, workspace_id, role) VALUES ($1, $2, $3)
         ON CONFLICT (user_id, workspace_id) DO UPDATE SET role = EXCLUDED.role",
    )
    .bind(user_id)
    .bind(workspace_id)
    .bind(role.as_str())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn remove(pool: &PgPool, workspace_id: Uuid, user_id: Uuid) -> AppResult<()> {
    let res = sqlx::query("DELETE FROM memberships WHERE workspace_id = $1 AND user_id = $2")
        .bind(workspace_id)
        .bind(user_id)
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

/// Deactivated accounts. **Without this, reactivation is unreachable** — a deactivated person
/// disappears from every list, so an administrator has no way to get their id, and the
/// reactivation endpoint needs exactly that id.
///
/// Kept as a separate query from [`org_users`] rather than adding an "include deactivated"
/// flag: the two are read by different consumers (one feeds the member picker, the other a
/// small section of the admin page), and a boolean parameter would force every call site to
/// stop and decide which one it wants.
pub async fn deactivated_users(pool: &PgPool, org_id: Uuid) -> AppResult<Vec<OrgUser>> {
    Ok(sqlx::query_as(
        "SELECT id, email, display_name, is_admin FROM users
         WHERE org_id = $1 AND deactivated_at IS NOT NULL
         ORDER BY deactivated_at DESC",
    )
    .bind(org_id)
    .fetch_all(pool)
    .await?)
}
