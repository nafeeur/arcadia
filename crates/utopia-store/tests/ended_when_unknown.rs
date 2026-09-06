//! The "ended, but we don't know when" state, tested against a real database.
//!
//! Before each end got its own precision, `valid_to IS NULL` meant both "still
//! ongoing" and "ended but we don't know when". Those two things have **opposite
//! truth values** — one says the relation holds now, the other says it doesn't —
//! but the ledger had only one way to write it, so "former CEO of Weta Digital"
//! could only be written as the former, and the graph would assert something the
//! text says has already ended.
//!
//! Three things are pinned here, each living in SQL or a SQL constraint that
//! `cargo check` can't see a word of:
//!
//! - All three end states can be stored; the four self-contradictory combinations
//!   cannot
//! - A new observation of "ended, but we don't know when" **won't** be merged as
//!   "says nothing" into an open row
//! - The temporal engine **doesn't treat it as an open row** to close — it doesn't
//!   even know its own end date
//!
//! One of those four negative cases used to slip through: when `valid_to` has a
//! date but the precision is NULL, `NULL IN ('year',…)` evaluates to NULL, `TRUE
//! AND NULL` is NULL, and a CHECK treats NULL as passing. Three-valued logic is
//! silent here — you only see it by actually running it.
//!
//! Skipped, not failed, when `UTOPIA_DATABASE_URL` is unset. Builds and tears down
//! its own fixtures; never touches an existing database.

use sqlx::PgPool;
use utopia_store::graph::{Validity, ENDED_UNKNOWN};
use uuid::Uuid;

struct Fixture {
    kb: Uuid,
    subject: Uuid,
    predicate: Uuid,
    object_a: Uuid,
    object_b: Uuid,
}

async fn seed(pool: &PgPool) -> anyhow::Result<Fixture> {
    let (org, ws, kb) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    let etype = Uuid::now_v7();
    let predicate = Uuid::now_v7();
    let (subject, object_a, object_b) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());

    sqlx::query("INSERT INTO organizations (id, name) VALUES ($1, 'ended-unknown-test')")
        .bind(org)
        .execute(pool)
        .await?;
    sqlx::query("INSERT INTO workspaces (id, org_id, name) VALUES ($1, $2, 'ended-unknown-test')")
        .bind(ws)
        .bind(org)
        .execute(pool)
        .await?;
    sqlx::query(
        "INSERT INTO knowledge_bases (id, workspace_id, name)
         VALUES ($1, $2, 'ended-unknown-test')",
    )
    .bind(kb)
    .bind(ws)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO entity_types (id, kb_id, key, label) VALUES ($1, $2, 'thing', 'Thing')",
    )
    .bind(etype)
    .bind(kb)
    .execute(pool)
    .await?;
    // functional: unique on the subject side, so the temporal engine will reconcile-close it
    sqlx::query(
        "INSERT INTO relation_types (id, kb_id, key, label, temporal, functional)
         VALUES ($1, $2, 'leads', 'leads', 'state', TRUE)",
    )
    .bind(predicate)
    .bind(kb)
    .execute(pool)
    .await?;
    for (id, name) in [
        (subject, "Akkaraju"),
        (object_a, "Weta"),
        (object_b, "Stability"),
    ] {
        sqlx::query(
            "INSERT INTO entities (id, kb_id, type_id, canonical_name) VALUES ($1, $2, $3, $4)",
        )
        .bind(id)
        .bind(kb)
        .bind(etype)
        .bind(name)
        .execute(pool)
        .await?;
    }
    Ok(Fixture {
        kb,
        subject,
        predicate,
        object_a,
        object_b,
    })
}

fn t(s: &str) -> chrono::DateTime<chrono::Utc> {
    s.parse().unwrap()
}

async fn shape(pool: &PgPool, id: Uuid) -> anyhow::Result<(bool, Option<String>)> {
    let row: (bool, Option<String>) =
        sqlx::query_as("SELECT valid_to IS NULL, valid_to_precision FROM facts WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await?;
    Ok(row)
}

#[tokio::test]
async fn a_relation_the_text_says_is_over_is_not_stored_as_ongoing() -> anyhow::Result<()> {
    let Some(url) = utopia_store::test_db::url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    let f = seed(&pool).await?;

    let run = async {
        // "Akkaraju, former CEO of Weta" -- the text says it ended, but doesn't give a date
        let (ended, _) = utopia_store::graph::insert_fact(
            &pool,
            f.kb,
            f.subject,
            Some(f.predicate),
            f.object_a,
            Validity::starting(Some(t("2020-01-01T00:00:00Z")), Some("day")).ended_when_unknown(),
            0.9,
        )
        .await?;
        let (to_is_null, prec) = shape(&pool, ended).await?;
        assert!(to_is_null, "no date to write, so valid_to stays NULL");
        assert_eq!(
            prec.as_deref(),
            Some(ENDED_UNKNOWN),
            "but the precision must say \"it ended\" -- without it, this is indistinguishable from \"still ongoing\""
        );

        // **The same assertion getting a second "ended, but we don't know when" observation
        // must not be merged as "says nothing".**
        // The old criterion was valid_from.is_none() && valid_to.is_none(), and this
        // observation satisfies both -- it would get merged into the open row, losing
        // the one piece of information it carried (that it ended)
        let (second, created) = utopia_store::graph::insert_fact(
            &pool,
            f.kb,
            f.subject,
            Some(f.predicate),
            f.object_b,
            Validity::default().ended_when_unknown(),
            0.9,
        )
        .await?;
        assert!(created, "it said something, so it shouldn't be treated as a weaker statement and merged away");
        assert_eq!(
            shape(&pool, second).await?.1.as_deref(),
            Some(ENDED_UNKNOWN)
        );
        Ok::<_, anyhow::Error>(())
    }
    .await;

    sqlx::query("DELETE FROM knowledge_bases WHERE id = $1")
        .bind(f.kb)
        .execute(&pool)
        .await?;
    run
}

/// The temporal engine **must not treat "already ended, when unknown" as an open row**
/// to close: an assertion that doesn't even know its own end time has no standing
/// to fix someone else's end time.
#[tokio::test]
async fn an_already_ended_fact_is_not_treated_as_an_open_claim() -> anyhow::Result<()> {
    let Some(url) = utopia_store::test_db::url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    let f = seed(&pool).await?;

    let run = async {
        // Old row: ended, don't know when
        let (old, _) = utopia_store::graph::insert_fact(
            &pool,
            f.kb,
            f.subject,
            Some(f.predicate),
            f.object_a,
            Validity::starting(Some(t("2020-01-01T00:00:00Z")), Some("day")).ended_when_unknown(),
            0.9,
        )
        .await?;
        // New row: a different object, starting in 2024. A functional relation, so the engine will go looking for an "open row"
        let (new, _) = utopia_store::graph::insert_fact(
            &pool,
            f.kb,
            f.subject,
            Some(f.predicate),
            f.object_b,
            Validity::starting(Some(t("2024-01-01T00:00:00Z")), Some("day")),
            0.9,
        )
        .await?;
        let report = utopia_store::temporal::reconcile_new_fact(
            &pool,
            f.kb,
            new,
            f.subject,
            f.predicate,
            Some(f.object_b),
            None,
            utopia_store::temporal::Uniqueness::SubjectSide,
            Validity::starting(Some(t("2024-01-01T00:00:00Z")), Some("day")),
            0.9,
        )
        .await?;
        assert_eq!(report.corrected.len(), 0, "the old row already ended, it shouldn't be closed again");
        assert_eq!(report.conflicts, 0, "nor is it a conflict -- the two spans never overlapped");
        // Old row untouched
        let still: Option<chrono::DateTime<chrono::Utc>> =
            sqlx::query_scalar("SELECT valid_to FROM facts WHERE id = $1")
                .bind(old)
                .fetch_one(&pool)
                .await?;
        assert!(still.is_none(), "the engine shouldn't invent an end date for it out of thin air");
        Ok::<_, anyhow::Error>(())
    }
    .await;

    sqlx::query("DELETE FROM knowledge_bases WHERE id = $1")
        .bind(f.kb)
        .execute(&pool)
        .await?;
    run
}
