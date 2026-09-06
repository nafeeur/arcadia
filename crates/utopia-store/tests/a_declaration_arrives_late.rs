//! Declaration arrives late: in a knowledge base whose ontology grew on its own, a
//! successor doesn't close its predecessor (#341).
//!
//! Pins down three things:
//! - **Candidates are computed, not guessed** — only a side with two or more open-
//!   valued holders counts; the side where everyone leads exactly one project is not
//!   reported
//! - **Reconciliation works once the declaration is added** — walking the timeline:
//!   three open facts close into one chain, each predecessor closes at the
//!   **earliest** successor's start, independent of ingest order; rerunning is a
//!   no-op
//! - **Without a declaration, nothing moves** — the reconcile endpoint refuses; the
//!   engine never infers on a human's behalf
//!
//! Skips rather than fails when `UTOPIA_DATABASE_URL` is unset. Builds and tears
//! down its own data.

use chrono::{DateTime, TimeZone, Utc};
use sqlx::PgPool;
use utopia_store::temporal;
use uuid::Uuid;

struct Fixture {
    org: Uuid,
    kb: Uuid,
    /// A relation with no axiom declared at all
    leads: Uuid,
    /// An attribute, likewise undeclared
    salary: Uuid,
    aurora: Uuid,
    zhang: Uuid,
    li: Uuid,
    zhou: Uuid,
    lin: Uuid,
}

fn day(y: i32, m: u32, d: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(y, m, d, 0, 0, 0).unwrap()
}

async fn seed(pool: &PgPool) -> anyhow::Result<Fixture> {
    let (org, ws, kb) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    let etype = Uuid::now_v7();
    let (leads, salary) = (Uuid::now_v7(), Uuid::now_v7());
    let (aurora, zhang, li, zhou, lin) = (
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
    );
    sqlx::query("INSERT INTO organizations (id, name) VALUES ($1, 'late-declaration-test')")
        .bind(org)
        .execute(pool)
        .await?;
    sqlx::query(
        "INSERT INTO workspaces (id, org_id, name) VALUES ($1, $2, 'late-declaration-test')",
    )
    .bind(ws)
    .bind(org)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO knowledge_bases (id, workspace_id, name) VALUES ($1, $2, 'late-declaration-test')",
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
    sqlx::query(
        "INSERT INTO relation_types (id, kb_id, key, label) VALUES ($1, $2, 'leads', 'leads')",
    )
    .bind(leads)
    .bind(kb)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO relation_types (id, kb_id, key, label, kind, datatype, unit)
         VALUES ($1, $2, 'salary', 'salary', 'attribute', 'number', 'CNY')",
    )
    .bind(salary)
    .bind(kb)
    .execute(pool)
    .await?;
    for (id, name) in [
        (aurora, "Project Aurora"),
        (zhang, "Zhang San"),
        (li, "Li Si"),
        (zhou, "Zhou Qi"),
        (lin, "Lin Zhao"),
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
        org,
        kb,
        leads,
        salary,
        aurora,
        zhang,
        li,
        zhou,
        lin,
    })
}

/// An open relation fact, start date precision "day".
async fn edge(
    pool: &PgPool,
    kb: Uuid,
    s: Uuid,
    p: Uuid,
    o: Uuid,
    from: DateTime<Utc>,
) -> anyhow::Result<Uuid> {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO facts (id, kb_id, subject_id, predicate_id, object_id, valid_from, valid_from_precision)
         VALUES ($1, $2, $3, $4, $5, $6, 'day')",
    )
    .bind(id)
    .bind(kb)
    .bind(s)
    .bind(p)
    .bind(o)
    .bind(from)
    .execute(pool)
    .await?;
    Ok(id)
}

/// An open attribute fact, value carried in `object_value`.
async fn attr(
    pool: &PgPool,
    kb: Uuid,
    s: Uuid,
    p: Uuid,
    value: serde_json::Value,
    from: DateTime<Utc>,
) -> anyhow::Result<Uuid> {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO facts (id, kb_id, subject_id, predicate_id, object_value, valid_from, valid_from_precision)
         VALUES ($1, $2, $3, $4, $5, $6, 'day')",
    )
    .bind(id)
    .bind(kb)
    .bind(s)
    .bind(p)
    .bind(value)
    .bind(from)
    .execute(pool)
    .await?;
    Ok(id)
}

/// A still-live fact on a predicate: (subject name, start, end), ordered by start.
async fn live(
    pool: &PgPool,
    kb: Uuid,
    p: Uuid,
) -> anyhow::Result<Vec<(String, Option<DateTime<Utc>>, Option<DateTime<Utc>>)>> {
    Ok(sqlx::query_as(
        "SELECT e.canonical_name, f.valid_from, f.valid_to
         FROM facts f JOIN entities e ON e.id = f.subject_id
         WHERE f.kb_id = $1 AND f.predicate_id = $2 AND f.invalidated_at IS NULL
         ORDER BY f.valid_from",
    )
    .bind(kb)
    .bind(p)
    .fetch_all(pool)
    .await?)
}

#[tokio::test]
async fn a_succession_closes_once_someone_declares_it() -> anyhow::Result<()> {
    let Some(url) = utopia_store::test_db::url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    let f = seed(&pool).await?;

    let run = async {
        // Ingest order is deliberately scrambled: Li Si first, Zhou Qi second, Zhang San last -- the timeline differs from ingest order
        edge(&pool, f.kb, f.li, f.leads, f.aurora, day(2024, 7, 5)).await?;
        edge(&pool, f.kb, f.zhou, f.leads, f.aurora, day(2025, 9, 1)).await?;
        edge(&pool, f.kb, f.zhang, f.leads, f.aurora, day(2023, 2, 1)).await?;
        attr(
            &pool,
            f.kb,
            f.lin,
            f.salary,
            serde_json::json!({ "value": 28000, "unit": "CNY" }),
            day(2023, 6, 1),
        )
        .await?;
        attr(
            &pool,
            f.kb,
            f.lin,
            f.salary,
            serde_json::json!({ "value": 32000, "unit": "CNY" }),
            day(2024, 2, 20),
        )
        .await?;

        // -- candidates --
        let cands = temporal::uniqueness_candidates(&pool, f.kb).await?;
        let on_leads: Vec<_> = cands.iter().filter(|c| c.predicate_id == f.leads).collect();
        assert_eq!(on_leads.len(), 1, "everyone leads exactly one project: the subject side should not be reported");
        let leads = on_leads[0];
        assert_eq!(leads.side, "object");
        assert!(!leads.declared);
        assert_eq!((leads.holders, leads.open_facts), (1, 3));
        assert_eq!((leads.would_close, leads.would_review), (2, 0));
        let ex = &leads.examples[0];
        assert_eq!(ex.holder, "Project Aurora");
        assert_eq!(
            ex.values.iter().map(|v| v.name.as_str()).collect::<Vec<_>>(),
            ["Zhang San", "Li Si", "Zhou Qi"],
            "examples are ordered by timeline, not by ingest order"
        );
        let salary = cands
            .iter()
            .find(|c| c.predicate_id == f.salary)
            .expect("two open salaries are a subject-side candidate");
        assert_eq!(salary.side, "subject");
        assert_eq!((salary.holders, salary.open_facts, salary.would_close), (1, 2, 1));
        assert_eq!(salary.examples[0].values[0].name, "28000 CNY", "literal value carries its unit");

        // -- without a declaration, nothing moves --
        assert!(
            temporal::reconcile_predicate(&pool, f.kb, f.leads).await.is_err(),
            "a predicate without a uniqueness declaration cannot be reconciled: the engine never infers on a human's behalf"
        );
        assert_eq!(live(&pool, f.kb, f.leads).await?.len(), 3);

        // -- declare object-side uniqueness, reconcile --
        sqlx::query("UPDATE relation_types SET inverse_functional = TRUE WHERE id = $1")
            .bind(f.leads)
            .execute(&pool)
            .await?;
        let r = temporal::reconcile_predicate(&pool, f.kb, f.leads).await?;
        assert_eq!((r.corrected.len(), r.conflicts), (2, 0));
        assert_eq!(
            live(&pool, f.kb, f.leads).await?,
            vec![
                ("Zhang San".to_string(), Some(day(2023, 2, 1)), Some(day(2024, 7, 5))),
                ("Li Si".to_string(), Some(day(2024, 7, 5)), Some(day(2025, 9, 1))),
                ("Zhou Qi".to_string(), Some(day(2025, 9, 1)), None),
            ],
            "each predecessor closes at the earliest successor's start: Zhang San ends at Li Si, Li Si ends at Zhou Qi"
        );
        // The rewritten original row is still there; visible by winding the record axis back
        let invalidated: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM facts WHERE kb_id = $1 AND predicate_id = $2 AND invalidated_at IS NOT NULL",
        )
        .bind(f.kb)
        .bind(f.leads)
        .fetch_one(&pool)
        .await?;
        assert_eq!(invalidated, 2);

        // Rerunning is a no-op; it's also gone from the candidates
        let again = temporal::reconcile_predicate(&pool, f.kb, f.leads).await?;
        assert!(again.corrected.is_empty() && again.conflicts == 0);
        assert!(temporal::uniqueness_candidates(&pool, f.kb)
            .await?
            .iter()
            .all(|c| c.predicate_id != f.leads));

        // -- attribute goes through the subject side --
        sqlx::query("UPDATE relation_types SET functional = TRUE WHERE id = $1")
            .bind(f.salary)
            .execute(&pool)
            .await?;
        let r = temporal::reconcile_predicate(&pool, f.kb, f.salary).await?;
        assert_eq!((r.corrected.len(), r.conflicts), (1, 0));
        let closed: Option<DateTime<Utc>> = sqlx::query_scalar(
            "SELECT valid_to FROM facts
             WHERE kb_id = $1 AND predicate_id = $2 AND invalidated_at IS NULL
               AND object_value->>'value' = '28000'",
        )
        .bind(f.kb)
        .bind(f.salary)
        .fetch_one(&pool)
        .await?;
        assert_eq!(closed, Some(day(2024, 2, 20)), "the old salary ends at the new salary's start");
        Ok::<_, anyhow::Error>(())
    }
    .await;

    // Delete the KB before the org: things like pending_facts.proposed_by don't cascade to org
    sqlx::query("DELETE FROM knowledge_bases WHERE id = $1")
        .bind(f.kb)
        .execute(&pool)
        .await?;
    sqlx::query("DELETE FROM organizations WHERE id = $1")
        .bind(f.org)
        .execute(&pool)
        .await?;
    run
}
