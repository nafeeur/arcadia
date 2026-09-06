//! Per-model concurrency limits.
//!
//! The real constraint is the provider's rate limit, and that's scoped to
//! (base_url, model) — a local Ollama might only take 2 concurrent requests,
//! while a hosted API can take 50. Previously a single deployment-level
//! `worker_concurrency` governed all tasks, which meant one number was
//! governing two entirely different things.
//!
//! A model with no dedicated config falls back to
//! `deployment_settings.default_model_concurrency` (default 10).

use sqlx::PgPool;
use utopia_core::models::ModelLimit;
use utopia_core::AppResult;

/// How much concurrency this model is allowed. Falls back to the deployment
/// default when there's no dedicated config.
///
/// Queried once before every LLM call — against a call that routinely takes
/// twenty-odd seconds, this query's cost is negligible, and in exchange an
/// admin's change takes effect immediately, with no cache invalidation needed.
pub async fn limit_for(pool: &PgPool, base_url: &str, model: &str) -> AppResult<usize> {
    let row: Option<(i32,)> = sqlx::query_as(
        "SELECT max_concurrent FROM model_concurrency WHERE base_url = $1 AND model = $2",
    )
    .bind(base_url)
    .bind(model)
    .fetch_optional(pool)
    .await?;
    if let Some((n,)) = row {
        return Ok(n.max(1) as usize);
    }
    let (dflt,): (i32,) =
        sqlx::query_as("SELECT default_model_concurrency FROM deployment_settings LIMIT 1")
            .fetch_optional(pool)
            .await?
            .unwrap_or((10,));
    Ok(dflt.max(1) as usize)
}

/// Configured models plus the deployment default (fetched in one go for the admin page).
pub async fn list(pool: &PgPool) -> AppResult<(Vec<ModelLimit>, i32)> {
    let rows: Vec<ModelLimit> = sqlx::query_as(
        "SELECT base_url, model, max_concurrent FROM model_concurrency ORDER BY base_url, model",
    )
    .fetch_all(pool)
    .await?;
    let (dflt,): (i32,) =
        sqlx::query_as("SELECT default_model_concurrency FROM deployment_settings LIMIT 1")
            .fetch_optional(pool)
            .await?
            .unwrap_or((10,));
    Ok((rows, dflt))
}

/// Set a model's concurrency. `max_concurrent` of None deletes the dedicated
/// config and falls back to the default.
pub async fn set(
    pool: &PgPool,
    base_url: &str,
    model: &str,
    max_concurrent: Option<i32>,
) -> AppResult<()> {
    match max_concurrent {
        Some(n) => {
            if !(1..=256).contains(&n) {
                return Err(utopia_core::AppError::invalid(
                    "concurrency_range",
                    "max_concurrent must be between 1 and 256",
                ));
            }
            sqlx::query(
                "INSERT INTO model_concurrency (base_url, model, max_concurrent)
                 VALUES ($1, $2, $3)
                 ON CONFLICT (base_url, model)
                 DO UPDATE SET max_concurrent = EXCLUDED.max_concurrent, updated_at = now()",
            )
            .bind(base_url)
            .bind(model)
            .bind(n)
            .execute(pool)
            .await?;
        }
        None => {
            sqlx::query("DELETE FROM model_concurrency WHERE base_url = $1 AND model = $2")
                .bind(base_url)
                .bind(model)
                .execute(pool)
                .await?;
        }
    }
    Ok(())
}

/// Deployment default concurrency (used by every model without its own config).
pub async fn set_default(pool: &PgPool, value: i32) -> AppResult<()> {
    if !(1..=256).contains(&value) {
        return Err(utopia_core::AppError::invalid(
            "concurrency_range",
            "default_model_concurrency must be between 1 and 256",
        ));
    }
    sqlx::query("UPDATE deployment_settings SET default_model_concurrency = $1")
        .bind(value)
        .execute(pool)
        .await?;
    Ok(())
}

/// Models actually in use across the deployment (those seen in any workspace's
/// settings), so the admin page can list what's configurable.
pub async fn models_in_use(pool: &PgPool) -> AppResult<Vec<(String, String, String)>> {
    Ok(sqlx::query_as(
        "SELECT DISTINCT chat_base_url, chat_model, 'chat' FROM llm_settings
          WHERE chat_base_url IS NOT NULL AND chat_model IS NOT NULL
         UNION
         SELECT DISTINCT embed_base_url, embed_model, 'embed' FROM llm_settings
          WHERE embed_base_url IS NOT NULL AND embed_model IS NOT NULL",
    )
    .fetch_all(pool)
    .await?)
}
