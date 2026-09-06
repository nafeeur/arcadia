use sqlx::PgPool;
use utopia_core::models::LlmSettings;
use utopia_core::{secrets, AppError, AppResult};
use uuid::Uuid;

/// Unsealed on the way out: both API keys are sealed at rest
/// (`utopia_core::secrets`). Every query that returns `LlmSettings` passes
/// through here
fn opened(mut s: LlmSettings) -> AppResult<LlmSettings> {
    s.chat_api_key = secrets::open_opt(s.chat_api_key.as_deref()).map_err(AppError::Other)?;
    s.embed_api_key = secrets::open_opt(s.embed_api_key.as_deref()).map_err(AppError::Other)?;
    Ok(s)
}

pub async fn get(pool: &PgPool, workspace_id: Uuid) -> AppResult<Option<LlmSettings>> {
    let row: Option<LlmSettings> =
        sqlx::query_as("SELECT * FROM llm_settings WHERE workspace_id = $1")
            .bind(workspace_id)
            .fetch_optional(pool)
            .await?;
    row.map(opened).transpose()
}

/// Any one workspace's settings that has a chat model configured. Used by the
/// endpoint probe: the endpoint address is shared across the deployment, so it
/// doesn't matter which workspace's config it's read from, and the probe has
/// no "current workspace" context anyway.
pub async fn any_with_chat(pool: &PgPool) -> AppResult<Option<LlmSettings>> {
    let row: Option<LlmSettings> = sqlx::query_as(
        "SELECT * FROM llm_settings
         WHERE chat_base_url IS NOT NULL AND chat_model IS NOT NULL
         ORDER BY workspace_id LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;
    row.map(opened).transpose()
}

/// upsert; passing None for an api_key means keep the old value (the frontend
/// never sends secrets back).
#[allow(clippy::too_many_arguments)]
pub async fn upsert(
    pool: &PgPool,
    workspace_id: Uuid,
    chat_base_url: Option<&str>,
    chat_api_key: Option<&str>,
    chat_model: Option<&str>,
    embed_base_url: Option<&str>,
    embed_api_key: Option<&str>,
    embed_model: Option<&str>,
    embed_dim: Option<i32>,
) -> AppResult<LlmSettings> {
    // Sealed on the way in; None stays None (keeps the old value)
    let chat_api_key = secrets::seal_opt(chat_api_key);
    let embed_api_key = secrets::seal_opt(embed_api_key);
    let row: LlmSettings = sqlx::query_as(
        "INSERT INTO llm_settings
             (workspace_id, chat_base_url, chat_api_key, chat_model,
              embed_base_url, embed_api_key, embed_model, embed_dim, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, now())
         ON CONFLICT (workspace_id) DO UPDATE SET
             chat_base_url  = EXCLUDED.chat_base_url,
             chat_api_key   = COALESCE(EXCLUDED.chat_api_key, llm_settings.chat_api_key),
             chat_model     = EXCLUDED.chat_model,
             embed_base_url = EXCLUDED.embed_base_url,
             embed_api_key  = COALESCE(EXCLUDED.embed_api_key, llm_settings.embed_api_key),
             embed_model    = EXCLUDED.embed_model,
             embed_dim      = EXCLUDED.embed_dim,
             updated_at     = now()
         RETURNING *",
    )
    .bind(workspace_id)
    .bind(chat_base_url)
    .bind(chat_api_key)
    .bind(chat_model)
    .bind(embed_base_url)
    .bind(embed_api_key)
    .bind(embed_model)
    .bind(embed_dim)
    .fetch_one(pool)
    .await?;
    opened(row)
}
