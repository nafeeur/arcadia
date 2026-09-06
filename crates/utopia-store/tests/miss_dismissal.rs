//! What happens after dismissing an unmatched wording, run against a real database.
//!
//! This is all SQL, invisible to `cargo check`. And this behavior **used to be wrong and
//! invisible**: `record_miss` carried a `WHERE dismissed_at IS NULL`, so dismissing once
//! stopped both surfacing and counting. A wording that appeared once in the first document,
//! once dismissed, kept its count pinned at 1 even after twenty more documents used it —
//! the basis for that original judgment had long since stopped holding, and nobody could see it.
//!
//! What this pins down is **suppression separate from counting**:
//!
//! - `record_miss` keeps accumulating after dismissal
//! - `list_misses` no longer returns it (proposal and auto ontology-expansion steps are unchanged)
//! - `list_dismissed_misses` returns it, carrying the **updated** count
//! - after `restore_miss`, it's back in the normal list with a continuous count, not reset to zero
//!
//! Skipped, not failed, when `UTOPIA_DATABASE_URL` is unset. Builds and tears down its own data — never touches an existing database.

use sqlx::PgPool;
use uuid::Uuid;

async fn fresh_kb(pool: &PgPool) -> anyhow::Result<Uuid> {
    let (org, ws, kb) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    sqlx::query("INSERT INTO organizations (id, name) VALUES ($1, 'dismissal-test')")
        .bind(org)
        .execute(pool)
        .await?;
    sqlx::query("INSERT INTO workspaces (id, org_id, name) VALUES ($1, $2, 'dismissal-test')")
        .bind(ws)
        .bind(org)
        .execute(pool)
        .await?;
    sqlx::query(
        "INSERT INTO knowledge_bases (id, workspace_id, name) VALUES ($1, $2, 'dismissal-test')",
    )
    .bind(kb)
    .bind(ws)
    .execute(pool)
    .await?;
    Ok(kb)
}

fn count_of(rows: &[utopia_core::models::OntologyMiss], key: &str) -> Option<i32> {
    rows.iter().find(|m| m.key == key).map(|m| m.count)
}

#[tokio::test]
async fn dismissing_stops_the_suggestion_but_not_the_counting() -> anyhow::Result<()> {
    let Some(url) = utopia_store::test_db::url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    let kb = fresh_kb(&pool).await?;

    let run = async {
        use utopia_store::ontology as ont;
        // Appeared once in the first document
        ont::record_miss(&pool, kb, "relation_type", "acquired", Some("A → B")).await?;
        assert_eq!(
            count_of(&ont::list_misses(&pool, kb).await?, "acquired"),
            Some(1)
        );

        // The user, seeing "appeared 1 time", judges this a one-off wording
        ont::dismiss_miss(&pool, kb, "relation_type", "acquired").await?;
        assert_eq!(
            count_of(&ont::list_misses(&pool, kb).await?, "acquired"),
            None,
            "忽略之后不该再进建议列表"
        );

        // The next two documents also mention it. **Key assertion**: the count must
        // keep advancing, otherwise the basis for that judgment goes stale with nobody noticing
        ont::record_miss(&pool, kb, "relation_type", "acquired", Some("C → D")).await?;
        ont::record_miss(&pool, kb, "relation_type", "acquired", Some("E → F")).await?;
        assert_eq!(
            count_of(&ont::list_dismissed_misses(&pool, kb).await?, "acquired"),
            Some(3),
            "忽略期间的出现次数必须照记"
        );
        assert_eq!(
            count_of(&ont::list_misses(&pool, kb).await?, "acquired"),
            None,
            "计数在涨，但抑制照旧"
        );

        // Seeing it climb to 3, the user restores it
        ont::restore_miss(&pool, kb, "relation_type", "acquired").await?;
        assert_eq!(
            count_of(&ont::list_misses(&pool, kb).await?, "acquired"),
            Some(3),
            "撤回之后计数是连续的，不是从头来过"
        );
        assert_eq!(
            count_of(&ont::list_dismissed_misses(&pool, kb).await?, "acquired"),
            None
        );
        Ok::<_, anyhow::Error>(())
    }
    .await;

    sqlx::query("DELETE FROM knowledge_bases WHERE id = $1")
        .bind(kb)
        .execute(&pool)
        .await?;
    run
}
