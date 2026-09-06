//! Manually correcting a fact's validity interval (302), run against a real database.
//!
//! Extraction read "first half of 2023" as January 1st; before this, the only fix was
//! to delete the document and re-extract. The whole risk of this path is **that it
//! looks like a plain UPDATE**: change one date, the graph looks right, and nobody can
//! tell the ledger is missing something. So here we pin down four things, none of
//! which `cargo check` can see:
//!
//! - The old row stays in the ledger with its invalidation time recorded, and the
//!   correction row chains back to it via `supersedes` — an in-place edit would make
//!   this very change disappear, and that's exactly what the record axis is meant to
//!   replay (0019)
//! - Evidence is copied onto the correction row. Skip this and the fact, right after
//!   its time is fixed, instantly becomes "stated by no one" and gets swept gray by
//!   the stale check
//! - Reconciliation must rerun after the start moves: only once the successor's
//!   start date is shifted does the uniqueness invariant see this collision for the
//!   first time
//! - An already-invalidated row can't be edited; returns None instead of inserting a
//!   correction that dangles off a dead row
//!
//! Skips rather than fails when `UTOPIA_DATABASE_URL` is unset. Builds and tears down
//! its own data, never touches an existing database.

use sqlx::PgPool;
use utopia_store::graph::Validity;
use uuid::Uuid;

struct Fixture {
    kb: Uuid,
    zhang: Uuid,
    li: Uuid,
    project: Uuid,
    leads: Uuid,
    chunk: Uuid,
    doc: Uuid,
}

async fn seed(pool: &PgPool) -> anyhow::Result<Fixture> {
    let (org, ws, kb) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    let etype = Uuid::now_v7();
    let leads = Uuid::now_v7();
    let (zhang, li, project) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    let (src, doc, chunk) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());

    sqlx::query("INSERT INTO organizations (id, name) VALUES ($1, 'time-edit-test')")
        .bind(org)
        .execute(pool)
        .await?;
    sqlx::query("INSERT INTO workspaces (id, org_id, name) VALUES ($1, $2, 'time-edit-test')")
        .bind(ws)
        .bind(org)
        .execute(pool)
        .await?;
    sqlx::query(
        "INSERT INTO knowledge_bases (id, workspace_id, name) VALUES ($1, $2, 'time-edit-test')",
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
    // inverse_functional: only one person leads a given project at a time — uniqueness
    // on the object side, exactly the invariant that moving the start date collides with
    sqlx::query(
        "INSERT INTO relation_types (id, kb_id, key, label, temporal, inverse_functional)
         VALUES ($1, $2, 'leads', 'leads', 'state', TRUE)",
    )
    .bind(leads)
    .bind(kb)
    .execute(pool)
    .await?;
    for (id, name) in [(zhang, "Zhang San"), (li, "Li Si"), (project, "Phoenix")] {
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
    sqlx::query("INSERT INTO sources (id, kb_id, name) VALUES ($1, $2, 'time-edit-test')")
        .bind(src)
        .bind(kb)
        .execute(pool)
        .await?;
    sqlx::query(
        "INSERT INTO documents (id, kb_id, source_id, filename, sha256, status)
         VALUES ($1, $2, $3, 'memo.md', 'timeedit', 'ready')",
    )
    .bind(doc)
    .bind(kb)
    .bind(src)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO chunks (id, kb_id, document_id, seq, text)
         VALUES ($1, $2, $3, 0, 'Zhang San led Phoenix in the first half of 2023.')",
    )
    .bind(chunk)
    .bind(kb)
    .bind(doc)
    .execute(pool)
    .await?;
    Ok(Fixture {
        kb,
        zhang,
        li,
        project,
        leads,
        chunk,
        doc,
    })
}

fn t(s: &str) -> chrono::DateTime<chrono::Utc> {
    s.parse().unwrap()
}

/// An open fact with evidence attached.
async fn assert_fact(
    pool: &PgPool,
    f: &Fixture,
    subject: Uuid,
    from: &str,
    precision: &str,
) -> anyhow::Result<Uuid> {
    let (id, _) = utopia_store::graph::insert_fact(
        pool,
        f.kb,
        subject,
        Some(f.leads),
        f.project,
        Validity::starting(Some(t(from)), Some(precision)),
        0.9,
    )
    .await?;
    sqlx::query(
        "INSERT INTO fact_evidence (fact_id, chunk_id, quote, document_id, doc_version)
         VALUES ($1, $2, 'led Phoenix', $3, 1)",
    )
    .bind(id)
    .bind(f.chunk)
    .bind(f.doc)
    .execute(pool)
    .await?;
    Ok(id)
}

#[derive(sqlx::FromRow)]
struct Row {
    valid_from: Option<chrono::DateTime<chrono::Utc>>,
    valid_from_precision: Option<String>,
    valid_to: Option<chrono::DateTime<chrono::Utc>>,
    valid_to_precision: Option<String>,
    invalidated_at: Option<chrono::DateTime<chrono::Utc>>,
    supersedes: Option<Uuid>,
}

async fn row(pool: &PgPool, id: Uuid) -> anyhow::Result<Row> {
    Ok(sqlx::query_as(
        "SELECT valid_from, valid_from_precision, valid_to, valid_to_precision,
                invalidated_at, supersedes
         FROM facts WHERE id = $1",
    )
    .bind(id)
    .fetch_one(pool)
    .await?)
}

async fn evidence_count(pool: &PgPool, id: Uuid) -> anyhow::Result<i64> {
    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM fact_evidence WHERE fact_id = $1")
        .bind(id)
        .fetch_one(pool)
        .await?;
    Ok(n)
}

/// Fixing a date must leave a visible trace in the ledger: the old row is invalidated
/// but not deleted, the correction row chains back to it, and evidence follows along.
#[tokio::test]
async fn correcting_a_date_leaves_the_old_reading_in_the_ledger() -> anyhow::Result<()> {
    let Some(url) = utopia_store::test_db::url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    let f = seed(&pool).await?;

    let run = async {
        // Extraction read it as January 1st, precise to the day — the source text only said "first half"
        let wrong = assert_fact(&pool, &f, f.zhang, "2023-01-01T00:00:00Z", "day").await?;

        let corrected = utopia_store::temporal::correct_interval(
            &pool,
            wrong,
            Validity::starting(Some(t("2023-06-01T00:00:00Z")), Some("month")),
        )
        .await?
        .expect("this row is still live, it should be editable");

        let old = row(&pool, wrong).await?;
        assert!(
            old.invalidated_at.is_some(),
            "the old row must record when it was superseded — an in-place UPDATE would make this change disappear"
        );
        assert_eq!(
            old.valid_from,
            Some(t("2023-01-01T00:00:00Z")),
            "the old row's world interval must not move at all: it records what we read at the time"
        );

        let new = row(&pool, corrected).await?;
        assert_eq!(new.supersedes, Some(wrong), "the correction row must chain back to the one it replaces");
        assert!(new.invalidated_at.is_none(), "the correction row is current");
        assert_eq!(new.valid_from, Some(t("2023-06-01T00:00:00Z")));
        assert_eq!(
            new.valid_from_precision.as_deref(),
            Some("month"),
            "precision follows the value — writing to the month means month, it shouldn't inherit the old row's 'day'"
        );

        assert_eq!(
            evidence_count(&pool, corrected).await?,
            1,
            "evidence must be copied onto the correction row. Without it, this fact instantly becomes \"stated by no one\" and gets swept gray"
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

/// All three states of the end bound must be editable, especially **reopening a closed
/// interval** — that's the only way back from "we mistakenly thought it had ended," and
/// it looks exactly like "nothing was filled in."
#[tokio::test]
async fn an_end_that_was_never_there_can_be_taken_back() -> anyhow::Result<()> {
    let Some(url) = utopia_store::test_db::url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    let f = seed(&pool).await?;

    let run = async {
        let fact = assert_fact(&pool, &f, f.zhang, "2023-01-01T00:00:00Z", "day").await?;
        // First close it to 2024
        let closed = utopia_store::temporal::correct_interval(
            &pool,
            fact,
            Validity {
                from: Some(t("2023-01-01T00:00:00Z")),
                from_precision: Some("day"),
                to: Some(t("2024-01-01T00:00:00Z")),
                to_precision: Some("year"),
                attested_at: None,
            },
        )
        .await?
        .expect("should be editable");
        assert_eq!(
            row(&pool, closed).await?.valid_to,
            Some(t("2024-01-01T00:00:00Z"))
        );

        // That was wrong: it hadn't actually ended. Take the end bound back entirely
        let reopened = utopia_store::temporal::correct_interval(
            &pool,
            closed,
            Validity::starting(Some(t("2023-01-01T00:00:00Z")), Some("day")),
        )
        .await?
        .expect("should be editable");
        let r = row(&pool, reopened).await?;
        assert!(r.valid_to.is_none(), "the end bound was taken back");
        assert!(
            r.valid_to_precision.is_none(),
            "precision must be taken back too: leaving 'year' with an empty date would be blocked by the CHECK, and under the old three-valued logic this combination once slipped through silently"
        );
        assert_eq!(r.supersedes, Some(closed), "the two corrections chain together, not each dangling on its own");
        Ok::<_, anyhow::Error>(())
    }
    .await;

    sqlx::query("DELETE FROM knowledge_bases WHERE id = $1")
        .bind(f.kb)
        .execute(&pool)
        .await?;
    run
}

/// Moving the start date earlier must trigger reconciliation. **This is the half of
/// the fix that's easiest to miss**: the interval is now correct, the edge in the graph
/// looks right too, but nobody has recomputed its relationship to the successor.
///
/// The two people here swap identities across the edit — before the fix, Zhang San
/// took office in 2025 and is Li Si's successor; after correcting to 2023, he becomes
/// the predecessor and should be closed off at Li Si's start date. The uniqueness
/// invariant only sees this for the first time during this reconciliation.
#[tokio::test]
async fn moving_a_start_earlier_makes_it_the_predecessor() -> anyhow::Result<()> {
    let Some(url) = utopia_store::test_db::url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    let f = seed(&pool).await?;

    let run = async {
        // Both open: Li Si took over 2024-07, Zhang San's date was extracted as
        // 2025-01 (wrong — he was actually in office since 2023). The invariant is
        // already violated at this point, it just hasn't been computed yet
        let li_fact = assert_fact(&pool, &f, f.li, "2024-07-01T00:00:00Z", "month").await?;
        let zhang_fact = assert_fact(&pool, &f, f.zhang, "2025-01-01T00:00:00Z", "month").await?;

        // Correct Zhang San back to the real 2023 date
        let fixed = utopia_store::temporal::correct_interval(
            &pool,
            zhang_fact,
            Validity::starting(Some(t("2023-01-01T00:00:00Z")), Some("month")),
        )
        .await?
        .expect("should be editable");
        let report = utopia_store::temporal::reconcile_moved_facts(&pool, f.kb, &[fixed]).await?;

        assert!(
            !report.corrected.is_empty(),
            "reconciliation must act: Zhang San starts 2023, Li Si took over 2024-07, the two can't both carry an open interval"
        );
        // The new fact starts earlier = it's the predecessor, closed at the old fact's start (the engine's criterion)
        assert!(
            row(&pool, fixed).await?.invalidated_at.is_some(),
            "the one that gets closed is Zhang San's own row, not Li Si's — the earlier one is the predecessor"
        );
        let current: Row = sqlx::query_as(
            "SELECT valid_from, valid_from_precision, valid_to, valid_to_precision,
                    invalidated_at, supersedes
             FROM facts WHERE supersedes = $1",
        )
        .bind(fixed)
        .fetch_one(&pool)
        .await?;
        assert_eq!(
            current.valid_to,
            Some(t("2024-07-01T00:00:00Z")),
            "Zhang San's interval closes at Li Si's start date"
        );
        assert_eq!(
            current.valid_from,
            Some(t("2023-01-01T00:00:00Z")),
            "the start date keeps its corrected value"
        );
        assert!(
            row(&pool, li_fact).await?.invalidated_at.is_none(),
            "Li Si's fact is untouched: he's the incumbent, it should stay open"
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

/// An already-invalidated row can't be corrected. Without this gate, two concurrent
/// edits would each produce a correction dangling off the dead row, adding a spurious
/// edge to the graph.
#[tokio::test]
async fn a_row_that_is_already_gone_cannot_be_corrected() -> anyhow::Result<()> {
    let Some(url) = utopia_store::test_db::url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    let f = seed(&pool).await?;

    let run = async {
        let fact = assert_fact(&pool, &f, f.zhang, "2023-01-01T00:00:00Z", "day").await?;
        let first = utopia_store::temporal::correct_interval(
            &pool,
            fact,
            Validity::starting(Some(t("2023-06-01T00:00:00Z")), Some("month")),
        )
        .await?;
        assert!(first.is_some());

        // Try correcting the same (now-invalidated) id a second time
        let second = utopia_store::temporal::correct_interval(
            &pool,
            fact,
            Validity::starting(Some(t("2022-01-01T00:00:00Z")), Some("year")),
        )
        .await?;
        assert!(
            second.is_none(),
            "an invalidated row can't be edited, the caller should know nothing happened — not get a correction inserted dangling off the dead row"
        );
        let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM facts WHERE supersedes = $1")
            .bind(fact)
            .fetch_one(&pool)
            .await?;
        assert_eq!(n, 1, "a dead row should have exactly one successor");
        Ok::<_, anyhow::Error>(())
    }
    .await;

    sqlx::query("DELETE FROM knowledge_bases WHERE id = $1")
        .bind(f.kb)
        .execute(&pool)
        .await?;
    run
}
