use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::time::Duration;

/// Default connection pool cap.
///
/// **It is no longer equal to worker concurrency** (the worker default is already 64, see
/// migration 0011) — deliberately: background jobs spend most of their time waiting on model
/// responses, and during that wait they hold no connection and are throttled to around a
/// dozen by the per-model semaphore anyway. What the pool needs to cover is the work that's
/// **actually running** — the short queries like per-chunk epoch checks, vector retrieval,
/// unmatched-count tallies — which arrive in bursts.
///
/// It used to be hardcoded to 10 while the worker default was 32 — a 3x oversubscription that,
/// under load, shows up first as slow requests and then as timeouts, never as an error saying
/// "pool exhausted". So the number this is sized against is "how many short queries are
/// running concurrently", not "how many task slots exist"; when raising worker concurrency,
/// this is the one that needs to scale with it.
const DEFAULT_MAX_CONNECTIONS: u32 = 32;

pub async fn connect(database_url: &str, max_connections: Option<u32>) -> anyhow::Result<PgPool> {
    let max = max_connections.unwrap_or(DEFAULT_MAX_CONNECTIONS).max(2);
    let pool = PgPoolOptions::new()
        .max_connections(max)
        // Fail loudly early when a connection can't be acquired, rather than leaving the
        // request hanging on the default 30 seconds — an undersized pool should be obviously
        // the pool's fault
        .acquire_timeout(Duration::from_secs(10))
        .connect(database_url)
        .await?;
    tracing::info!(max_connections = max, "database connection pool established");
    Ok(pool)
}

pub async fn migrate(pool: &PgPool) -> anyhow::Result<()> {
    sqlx::migrate!("../../migrations").run(pool).await?;
    Ok(())
}
