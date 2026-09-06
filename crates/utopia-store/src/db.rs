use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::time::Duration;

/// Default connection pool ceiling.
///
/// **It no longer equals worker concurrency** (worker default is already 64, see
/// migration 0011), and that's intentional: background tasks spend most of their
/// time waiting on model replies, and during that wait they hold no connection and
/// are capped at a dozen or so by the per-model semaphore anyway. What the pool
/// needs to cover is the work that's **actually running** — short queries like
/// per-chunk epoch checks, vector search, unmatched-count tallies — which arrive
/// in bursts.
///
/// It used to be hardcoded at 10 while the worker default was 32 — a 3x overshoot
/// that, on collision, first slows requests down and then times them out, never
/// with an error that says "pool exhausted". So the number this constant should
/// answer to is "how many short queries are running at once", not "how many task
/// slots exist"; when the worker count goes up, the former is what should move.
const DEFAULT_MAX_CONNECTIONS: u32 = 32;

pub async fn connect(database_url: &str, max_connections: Option<u32>) -> anyhow::Result<PgPool> {
    let max = max_connections.unwrap_or(DEFAULT_MAX_CONNECTIONS).max(2);
    let pool = PgPoolOptions::new()
        .max_connections(max)
        // Fail loudly and early when a connection can't be acquired, instead of
        // hanging the request on the default 30s — an undersized pool should be
        // visibly the pool's fault
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
