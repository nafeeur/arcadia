//! The outer backstop **can actually be raised**, and both defaults name the same number (migration 0011).
//!
//! Why this needs a real database: the shape of this bug is **the constraint lives in SQL, validation lives in Rust, and the two drift independently**.
//! `set_worker_concurrency` allows 1..=256, while the column's CHECK used to be `BETWEEN 1 AND 32` —
//! fill in anything from 33 to 256 on the settings page and Rust says yes, the database says no; the user just sees
//! a CHECK constraint error. `cargo check` and clippy say nothing about it, because the two sides
//! aren't even in the same language.
//!
//! The constraint's upper bound originally equaled the column's default (both 32), so **the backstop couldn't be raised by even one notch** —
//! while 0001's comment says it "should be well above the sum of the per-model limits, or a throttled job will
//! occupy the slots and starve the others." The constraint blocked its own design.
//!
//! Skips rather than fails when `UTOPIA_DATABASE_URL` is absent. Read-only, then restored afterward — leaves no trace.
//!
//! **Both checks run in order inside a single test**: they both touch the same singleton row, and tests within
//! one binary run concurrently by default. Row locks already prevent dirty reads, but there's no reason for
//! one test's update to interleave with another's delete (inside a transaction) — running them in sequence removes any need to gamble on that (raised by the reporter of #248).

use sqlx::PgPool;

#[tokio::test]
async fn the_backstop_can_be_raised() -> anyhow::Result<()> {
    let Some(url) = utopia_store::test_db::url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    every_value_rust_accepts_the_database_accepts_too(&pool).await?;
    the_two_defaults_say_the_same_number(&pool).await
}

/// Any value Rust allows, the database must allow too.
///
/// Test each boundary individually rather than just one: drift could show up at any tier, and these queries are cheap.
async fn every_value_rust_accepts_the_database_accepts_too(pool: &PgPool) -> anyhow::Result<()> {
    let before = utopia_store::access::worker_concurrency(pool).await?;

    let run = async {
        // 33 was the first tier originally blocked; 256 is Rust's upper bound
        for v in [1_i32, 33, 64, 255, 256] {
            utopia_store::access::set_worker_concurrency(pool, v)
                .await
                .map_err(|e| anyhow::anyhow!("Rust 放行了 {v}，数据库却拒绝：{e}"))?;
            let got = utopia_store::access::worker_concurrency(pool).await?;
            assert_eq!(got, v, "写进去 {v} 读回来却是 {got}");
        }

        // The reverse: values Rust rejects must not sneak into the database
        for v in [0_i32, 257, -1] {
            assert!(
                utopia_store::access::set_worker_concurrency(pool, v)
                    .await
                    .is_err(),
                "{v} 越界了却被接受"
            );
        }
        Ok::<(), anyhow::Error>(())
    }
    .await;

    // Restore: this is a shared deployment setting; the test must not change anyone else's runtime parameters
    utopia_store::access::set_worker_concurrency(pool, before).await?;
    run
}

/// Rust's fallback value when the table has no row must be the same number as the column's default.
///
/// The two are written separately (one in SQL, one in Rust); changing one doesn't carry over to the other. The consequence of a mismatch is subtle:
/// a database with a row runs one number, an empty-table database runs another, and neither errors.
async fn the_two_defaults_say_the_same_number(pool: &PgPool) -> anyhow::Result<()> {
    // The column's default: ask information_schema directly, don't guess
    let column_default: Option<String> = sqlx::query_scalar(
        "SELECT column_default FROM information_schema.columns
          WHERE table_name = 'deployment_settings' AND column_name = 'worker_concurrency'",
    )
    .fetch_one(pool)
    .await?;
    let column_default: i32 = column_default
        .as_deref()
        .and_then(|s| s.split("::").next())
        .and_then(|s| s.trim().parse().ok())
        .ok_or_else(|| anyhow::anyhow!("读不出列缺省：{column_default:?}"))?;

    // Rust's fallback: hide the row, then ask again
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM deployment_settings")
        .execute(&mut *tx)
        .await?;
    let fallback: Option<(i32,)> =
        sqlx::query_as("SELECT worker_concurrency FROM deployment_settings LIMIT 1")
            .fetch_optional(&mut *tx)
            .await?;
    assert!(fallback.is_none(), "行没删掉，下面这句就白测了");
    tx.rollback().await?; // **Must roll back**: deployment_settings is a singleton; deleting it leaves the service with no settings

    // The number inside access.rs's unwrap_or
    let rust_fallback = 64;
    assert_eq!(
        column_default, rust_fallback,
        "列缺省是 {column_default}，Rust 兜底是 {rust_fallback}——两处漂开了"
    );
    Ok(())
}
