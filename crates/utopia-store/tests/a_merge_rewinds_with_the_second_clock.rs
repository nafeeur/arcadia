//! Merges must rewind too (0019 second cut / #336), run against a real database.
//!
//! A merge is **an in-place rewrite**: `UPDATE facts SET subject_id = target`. The row
//! only keeps the shape it had after the merge, so "who this fact hung on that day in
//! March" lives neither in the fact row nor the entity row — only in the `entity_merges`
//! array. This test is exactly about that mapping.
//!
//! Three points in time, because each breaks in a different way:
//! - **Before the merge**: the swallowed entity has to grow back, carrying its own facts
//! - **Within the window of a merge that was later reverted**: for that stretch they
//!   **really were** one entity — the revert moved the rows back, so the current rows
//!   show none of it; it can only be read from the array
//! - **Now**: everything as usual, and adding the mapping must not slow it down or change it

use sqlx::PgPool;
use uuid::Uuid;

fn t(s: &str) -> chrono::DateTime<chrono::Utc> {
    s.parse().unwrap()
}

struct Fixture {
    org: Uuid,
    kb: Uuid,
    /// The one that survives
    zhang_a: Uuid,
    /// Merged into A in April, still merged
    zhang_b: Uuid,
    /// Merged into A in May, reverted in June
    zhang_c: Uuid,
    fact_a: Uuid,
    fact_b: Uuid,
    fact_c: Uuid,
}

async fn seed(pool: &PgPool) -> anyhow::Result<Fixture> {
    let (org, ws, kb) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    let (person, company) = (Uuid::now_v7(), Uuid::now_v7());
    let works_for = Uuid::now_v7();
    let (acme, zenith) = (Uuid::now_v7(), Uuid::now_v7());
    let (zhang_a, zhang_b, zhang_c) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    let (fact_a, fact_b, fact_c) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());

    sqlx::query("INSERT INTO organizations (id, name) VALUES ($1, 'merge-rewind-test')")
        .bind(org)
        .execute(pool)
        .await?;
    sqlx::query("INSERT INTO workspaces (id, org_id, name) VALUES ($1, $2, 'merge-rewind-test')")
        .bind(ws)
        .bind(org)
        .execute(pool)
        .await?;
    sqlx::query(
        "INSERT INTO knowledge_bases (id, workspace_id, name) VALUES ($1, $2, 'merge-rewind-test')",
    )
    .bind(kb)
    .bind(ws)
    .execute(pool)
    .await?;
    for (id, key, label) in [
        (person, "person", "Person"),
        (company, "company", "Company"),
    ] {
        sqlx::query("INSERT INTO entity_types (id, kb_id, key, label) VALUES ($1, $2, $3, $4)")
            .bind(id)
            .bind(kb)
            .bind(key)
            .bind(label)
            .execute(pool)
            .await?;
    }
    sqlx::query(
        "INSERT INTO relation_types (id, kb_id, key, label) VALUES ($1, $2, 'works_for', 'works for')",
    )
    .bind(works_for)
    .bind(kb)
    .execute(pool)
    .await?;
    // Entity birth times are also viewed retroactively: on January's graph they don't exist yet
    for (id, type_id, name) in [
        (acme, company, "Acme"),
        (zenith, company, "Zenith"),
        (zhang_a, person, "Zhang Wei"),
        (zhang_b, person, "Zhang Wei"),
        (zhang_c, person, "Zhang Wei"),
    ] {
        sqlx::query(
            "INSERT INTO entities (id, kb_id, type_id, canonical_name, created_at)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(id)
        .bind(kb)
        .bind(type_id)
        .bind(name)
        .bind(t("2026-02-01T00:00:00Z"))
        .execute(pool)
        .await?;
    }
    for (id, subject, object, rec) in [
        (fact_a, zhang_a, acme, "2026-03-01T00:00:00Z"),
        (fact_b, zhang_b, zenith, "2026-03-02T00:00:00Z"),
        (fact_c, zhang_c, acme, "2026-03-03T00:00:00Z"),
    ] {
        sqlx::query(
            "INSERT INTO facts (id, kb_id, subject_id, predicate_id, object_id, confidence,
                                recorded_at)
             VALUES ($1, $2, $3, $4, $5, 0.9, $6)",
        )
        .bind(id)
        .bind(kb)
        .bind(subject)
        .bind(works_for)
        .bind(object)
        .bind(t(rec))
        .execute(pool)
        .await?;
    }

    // April: B merges into A, still merged
    utopia_store::resolution::merge_entities(pool, kb, zhang_b, zhang_a, None, "test").await?;
    backdate(pool, zhang_b, "2026-04-01T00:00:00Z", None).await?;

    // May: C merges into A; reverted in June. **The revert moved the facts back**, so
    // the current rows show no trace that they were one entity between May and June —
    // that window only lives in entity_merges
    let merge_c =
        utopia_store::resolution::merge_entities(pool, kb, zhang_c, zhang_a, None, "test").await?;
    backdate(pool, zhang_c, "2026-05-01T00:00:00Z", None).await?;
    utopia_store::resolution::revert_merge(pool, kb, merge_c).await?;
    backdate(
        pool,
        zhang_c,
        "2026-05-01T00:00:00Z",
        Some("2026-06-01T00:00:00Z"),
    )
    .await?;

    Ok(Fixture {
        org,
        kb,
        zhang_a,
        zhang_b,
        zhang_c,
        fact_a,
        fact_b,
        fact_c,
    })
}

/// The merge timestamp is stamped by `now()`; the test needs a deterministic date.
async fn backdate(
    pool: &PgPool,
    source: Uuid,
    created: &str,
    reverted: Option<&str>,
) -> anyhow::Result<()> {
    sqlx::query("UPDATE entity_merges SET created_at = $2, reverted_at = $3 WHERE source_id = $1")
        .bind(source)
        .bind(t(created))
        .bind(reverted.map(t))
        .execute(pool)
        .await?;
    Ok(())
}

async fn facts_of(
    pool: &PgPool,
    kb: Uuid,
    entity: Uuid,
    as_of: Option<&str>,
) -> anyhow::Result<Vec<Uuid>> {
    let (_, facts) =
        utopia_store::graph::entity_detail(pool, kb, entity, None, as_of.map(t)).await?;
    let mut ids: Vec<Uuid> = facts.iter().map(|f| f.id).collect();
    ids.sort();
    Ok(ids)
}

fn sorted(mut v: Vec<Uuid>) -> Vec<Uuid> {
    v.sort();
    v
}

#[tokio::test]
async fn a_merged_entity_comes_back_before_the_merge() -> anyhow::Result<()> {
    let Some(url) = utopia_store::test_db::url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    let f = seed(&pool).await?;

    let nodes = |as_of: Option<&'static str>| {
        let pool = pool.clone();
        async move {
            let (nodes, _, total, _) =
                utopia_store::graph::overview(&pool, f.kb, 50, None, as_of.map(t)).await?;
            let mut ids: Vec<Uuid> = nodes.iter().map(|n| n.id).collect();
            ids.sort();
            Ok::<(Vec<Uuid>, i64), anyhow::Error>((ids, total))
        }
    };

    // 1. Now: B is merged (does not appear), C was reverted (appears as usual)
    let (ids, total) = nodes(None).await?;
    assert!(!ids.contains(&f.zhang_b), "仍并着的实体不该出现在画布上");
    assert!(ids.contains(&f.zhang_c), "撤销过的合并不该继续吞掉那个实体");
    assert_eq!(total, 4, "节点总数与画布同一口径");
    assert_eq!(
        facts_of(&pool, f.kb, f.zhang_a, None).await?,
        sorted(vec![f.fact_a, f.fact_b])
    );
    assert_eq!(
        facts_of(&pool, f.kb, f.zhang_c, None).await?,
        vec![f.fact_c]
    );

    // 2. March: the three Zhang Weis each stand on their own, each with their own fact.
    //    This is the whole point of this cut — the fact row says A, but on that day in
    //    March it hung on B
    let (ids, total) = nodes(Some("2026-03-15T00:00:00Z")).await?;
    assert!(ids.contains(&f.zhang_b) && ids.contains(&f.zhang_c));
    assert_eq!(total, 5);
    assert_eq!(
        facts_of(&pool, f.kb, f.zhang_a, Some("2026-03-15T00:00:00Z")).await?,
        vec![f.fact_a],
        "三月的 A 只有自己那一条"
    );
    assert_eq!(
        facts_of(&pool, f.kb, f.zhang_b, Some("2026-03-15T00:00:00Z")).await?,
        vec![f.fact_b],
        "被并掉的 B 在三月还拿着自己那条事实"
    );

    // 3. Mid-May: C's merge was **in effect** at that point (reverted only in June).
    //    The revert has already moved the row back to C, so this snapshot can only be
    //    read from the entity_merges array
    let (ids, total) = nodes(Some("2026-05-15T00:00:00Z")).await?;
    assert!(!ids.contains(&f.zhang_c), "五月中 C 正并在 A 里");
    assert!(!ids.contains(&f.zhang_b));
    assert_eq!(total, 3);
    assert_eq!(
        facts_of(&pool, f.kb, f.zhang_a, Some("2026-05-15T00:00:00Z")).await?,
        sorted(vec![f.fact_a, f.fact_b, f.fact_c]),
        "五月中三条事实都在 A 身上"
    );

    // 4. January: the entities haven't been created yet, the graph is empty — not a
    //    fallback to the current state
    let (ids, total) = nodes(Some("2026-01-01T00:00:00Z")).await?;
    assert!(ids.is_empty(), "一月这些实体还不存在");
    assert_eq!(total, 0);

    sqlx::query("DELETE FROM knowledge_bases WHERE id = $1")
        .bind(f.kb)
        .execute(&pool)
        .await?;
    let gone = sqlx::query("DELETE FROM organizations WHERE id = $1")
        .bind(f.org)
        .execute(&pool)
        .await?;
    assert_eq!(gone.rows_affected(), 1, "一次性 org 没删掉");
    Ok(())
}
