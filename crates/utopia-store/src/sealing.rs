//! Credential backfill-sealing: plaintext credentials persisted before the
//! upgrade get sealed on the first startup after the key is installed.
//!
//! Idempotent, runs on every startup: the test is "value has no `enc:v1:`
//! prefix"; already-sealed values are left alone. The four credential spots
//! (`llm_settings`'s two keys, `data_sources.conn_string`, the credential keys
//! in `sources.config`, `sources.ingest_token`) are each scanned here — a new
//! storage location must be added here too, or it stays plaintext forever
//! with nothing to warn about it.

use sqlx::PgPool;
use utopia_core::models::SOURCE_SECRET_KEYS;
use utopia_core::{secrets, AppResult};
use uuid::Uuid;

/// Returns the number of rows sealed. Does nothing if the key isn't installed
pub async fn backfill(pool: &PgPool) -> AppResult<usize> {
    if !secrets::is_ready() {
        return Ok(0);
    }
    Ok(seal_llm_settings(pool, None).await?
        + seal_data_sources(pool, None).await?
        + seal_sources(pool, None).await?)
}

/// `only` = seal just this workspace (for tests; pass None at startup to scan everything)
pub async fn seal_llm_settings(pool: &PgPool, only: Option<Uuid>) -> AppResult<usize> {
    let rows: Vec<(Uuid, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT workspace_id, chat_api_key, embed_api_key FROM llm_settings
          WHERE ($1::uuid IS NULL OR workspace_id = $1)
            AND ((chat_api_key IS NOT NULL AND chat_api_key NOT LIKE 'enc:v1:%')
              OR (embed_api_key IS NOT NULL AND embed_api_key NOT LIKE 'enc:v1:%'))",
    )
    .bind(only)
    .fetch_all(pool)
    .await?;
    let n = rows.len();
    for (ws, chat, embed) in rows {
        sqlx::query(
            "UPDATE llm_settings SET chat_api_key = $2, embed_api_key = $3 WHERE workspace_id = $1",
        )
        .bind(ws)
        .bind(secrets::seal_opt(chat.as_deref()))
        .bind(secrets::seal_opt(embed.as_deref()))
        .execute(pool)
        .await?;
    }
    Ok(n)
}

pub async fn seal_data_sources(pool: &PgPool, only: Option<Uuid>) -> AppResult<usize> {
    let rows: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT id, conn_string FROM data_sources
          WHERE ($1::uuid IS NULL OR id = $1) AND conn_string NOT LIKE 'enc:v1:%'",
    )
    .bind(only)
    .fetch_all(pool)
    .await?;
    let n = rows.len();
    for (id, conn) in rows {
        sqlx::query("UPDATE data_sources SET conn_string = $2 WHERE id = $1")
            .bind(id)
            .bind(secrets::seal(&conn))
            .execute(pool)
            .await?;
    }
    Ok(n)
}

/// The sources table is small, so read the whole thing back and check in Rust:
/// the credential keys are buried in JSON, which would make a SQL check more
/// convoluted, not less
pub async fn seal_sources(pool: &PgPool, only: Option<Uuid>) -> AppResult<usize> {
    let rows: Vec<(Uuid, serde_json::Value, Option<String>)> = sqlx::query_as(
        "SELECT id, config, ingest_token FROM sources WHERE ($1::uuid IS NULL OR id = $1)",
    )
    .bind(only)
    .fetch_all(pool)
    .await?;
    let mut n = 0;
    for (id, mut config, token) in rows {
        let plain_key = config.as_object().is_some_and(|o| {
            SOURCE_SECRET_KEYS.iter().any(|k| {
                o.get(*k)
                    .and_then(|v| v.as_str())
                    .is_some_and(|s| !secrets::is_sealed(s))
            })
        });
        let plain_token = token.as_deref().is_some_and(|t| !secrets::is_sealed(t));
        if !plain_key && !plain_token {
            continue;
        }
        secrets::seal_json_keys(&mut config, SOURCE_SECRET_KEYS);
        sqlx::query("UPDATE sources SET config = $2, ingest_token = $3 WHERE id = $1")
            .bind(id)
            .bind(config)
            .bind(secrets::seal_opt(token.as_deref()))
            .execute(pool)
            .await?;
        n += 1;
    }
    Ok(n)
}
