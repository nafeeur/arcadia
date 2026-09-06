//! Recording-axis rewind (0019 / #307), run against a real database.
//!
//! Why this needs a real database: this defense **lives entirely inside SQL strings**. `cargo check`
//! can't see a missing WHERE clause, and clippy won't say a word — 0009 fell into exactly this trap
//! (`NULL <> uuid` doesn't error, it just selects nothing); the `human_type_decisions` test is what
//! that incident left behind.
//!
//! Both directions need assertions, because they break in different ways:
//! - a retracted row must **not appear** at `as_of = now` (an inverted predicate would resurrect every retracted row)
//! - a retracted row must appear **before** its invalidation moment (an unwired predicate defaults to "now" forever, so replay comes back empty-handed anyway)
//! - a row whose `recorded_at` is later than T must **not appear** at T (writing only the lower bound would let March see April's correction)
//!
//! Self-seeding, self-cleaning: a one-off org/workspace/kb, deleted along with the org once done. Never touches an existing database.

use sqlx::PgPool;
use uuid::Uuid;

fn t(s: &str) -> chrono::DateTime<chrono::Utc> {
    s.parse().unwrap()
}

struct Fixture {
    kb: Uuid,
    zhang: Uuid,
    /// The assertion recorded on 03-01 and corrected out on 03-20
    retracted: Uuid,
    /// The correction row recorded on 03-20
    correction: Uuid,
    /// The derivation produced on 03-05 whose premise disappeared on 03-22
    derived: Uuid,
}

/// A person takes over a project, we change our minds in March, and the engine derives an edge somewhere in between.
async fn seed(pool: &PgPool) -> anyhow::Result<Fixture> {
    let (org, ws, kb) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    let (person, project) = (Uuid::now_v7(), Uuid::now_v7());
    let (leads, part_of) = (Uuid::now_v7(), Uuid::now_v7());
    let (zhang, phoenix, program) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    let (retracted, correction, derived, rule) = (
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
    );

    sqlx::query("INSERT INTO organizations (id, name) VALUES ($1, 'rewind-test')")
        .bind(org)
        .execute(pool)
        .await?;
    sqlx::query("INSERT INTO workspaces (id, org_id, name) VALUES ($1, $2, 'rewind-test')")
        .bind(ws)
        .bind(org)
        .execute(pool)
        .await?;
    sqlx::query(
        "INSERT INTO knowledge_bases (id, workspace_id, name) VALUES ($1, $2, 'rewind-test')",
    )
    .bind(kb)
    .bind(ws)
    .execute(pool)
    .await?;
    for (id, key, label) in [
        (person, "person", "Person"),
        (project, "project", "Project"),
    ] {
        sqlx::query("INSERT INTO entity_types (id, kb_id, key, label) VALUES ($1, $2, $3, $4)")
            .bind(id)
            .bind(kb)
            .bind(key)
            .bind(label)
            .execute(pool)
            .await?;
    }
    for (id, key, label) in [(leads, "leads", "leads"), (part_of, "part_of", "part of")] {
        sqlx::query("INSERT INTO relation_types (id, kb_id, key, label) VALUES ($1, $2, $3, $4)")
            .bind(id)
            .bind(kb)
            .bind(key)
            .bind(label)
            .execute(pool)
            .await?;
    }
    for (id, type_id, name) in [
        (zhang, person, "Zhang San"),
        (phoenix, project, "Project Phoenix"),
        (program, project, "Phoenix Program"),
    ] {
        // **The entity's birth moment must be backfilled too**: a ledger where the fact is recorded
        // in March but the entity was created "just now" doesn't exist in reality — and since #336,
        // an entity not yet born at moment T no longer appears on the graph
        sqlx::query(
            "INSERT INTO entities (id, kb_id, type_id, canonical_name, created_at)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(id)
        .bind(kb)
        .bind(type_id)
        .bind(name)
        .bind(t("2026-01-01T00:00:00Z"))
        .execute(pool)
        .await?;
    }

    let fact = |id: Uuid, rec: &str, inv: Option<&str>, sup: Option<Uuid>| {
        let (rec, inv) = (t(rec), inv.map(t));
        sqlx::query(
            "INSERT INTO facts (id, kb_id, subject_id, predicate_id, object_id,
                                recorded_at, invalidated_at, supersedes)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(id)
        .bind(kb)
        .bind(zhang)
        .bind(leads)
        .bind(phoenix)
        .bind(rec)
        .bind(inv)
        .bind(sup)
    };
    fact(
        retracted,
        "2026-03-01T00:00:00Z",
        Some("2026-03-20T00:00:00Z"),
        None,
    )
    .execute(pool)
    .await?;
    fact(correction, "2026-03-20T00:00:00Z", None, Some(retracted))
        .execute(pool)
        .await?;

    // Derivations also have two axes (derived_at / invalidated_at): the replayed graph keeps the edge
    // that was derived **at the time**, not the conclusion of today's rule set
    sqlx::query(
        "INSERT INTO rules (id, kb_id, predicate_id, kind) VALUES ($1, $2, $3, 'transitive')",
    )
    .bind(rule)
    .bind(kb)
    .bind(part_of)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO derived_facts (id, kb_id, subject_id, predicate_id, object_id, rule_id,
                                    derived_at, invalidated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(derived)
    .bind(kb)
    .bind(phoenix)
    .bind(part_of)
    .bind(program)
    .bind(rule)
    .bind(t("2026-03-05T00:00:00Z"))
    .bind(t("2026-03-22T00:00:00Z"))
    .execute(pool)
    .await?;

    Ok(Fixture {
        kb,
        zhang,
        retracted,
        correction,
        derived,
    })
}

fn edge_ids(edges: &[utopia_core::models::GraphEdge]) -> Vec<Uuid> {
    let mut v: Vec<Uuid> = edges.iter().map(|e| e.id).collect();
    v.sort();
    v
}

fn sorted(mut v: Vec<Uuid>) -> Vec<Uuid> {
    v.sort();
    v
}

#[tokio::test]
async fn the_recording_axis_rewinds_on_every_graph_read() -> anyhow::Result<()> {
    let Some(url) = utopia_store::test_db::url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    let f = seed(&pool).await?;

    let overview = |as_of: Option<&'static str>| {
        let pool = pool.clone();
        async move { utopia_store::graph::overview(&pool, f.kb, 50, None, as_of.map(t)).await }
    };

    // 1. 现在：修正行在，被它顶掉的断言不在，前提没了的派生也不在
    let (nodes, edges, _, total_edges) = overview(None).await?;
    assert_eq!(
        edge_ids(&edges),
        vec![f.correction],
        "现在只该有修正行那条边"
    );
    assert_eq!(total_edges, 1, "边总数跟着画布数，不是历史全量");
    let degree = |nodes: &[utopia_core::models::GraphNode], id: Uuid| {
        nodes.iter().find(|n| n.id == id).map(|n| n.degree)
    };
    assert_eq!(degree(&nodes, f.zhang), Some(1));

    // 2. 倒回 03-10：作废前的断言回来了，三月才推出的派生也在，
    //    而 03-20 才记下的修正行**不该出现**——那是当时还没发生的事
    let (nodes, edges, _, total_edges) = overview(Some("2026-03-10T00:00:00Z")).await?;
    assert_eq!(
        edge_ids(&edges),
        sorted(vec![f.retracted, f.derived]),
        "03-10 该看见当时持有的断言与派生"
    );
    assert_eq!(total_edges, 2);
    assert_eq!(degree(&nodes, f.zhang), Some(1), "度数按当时的边数");
    assert!(
        edges
            .iter()
            .find(|e| e.id == f.derived)
            .is_some_and(|e| e.derived),
        "派生边要带 derived 位，界面靠它画出区别"
    );

    // 3. 倒回 03-25：修正行已经记下，派生已经被推翻
    let (_, edges, _, _) = overview(Some("2026-03-25T00:00:00Z")).await?;
    assert_eq!(edge_ids(&edges), vec![f.correction]);

    // 4. 更早于一切：图是空的，而不是"退化成现在"
    let (_, edges, _, total_edges) = overview(Some("2026-02-01T00:00:00Z")).await?;
    assert!(edges.is_empty(), "02-01 我们还什么都没记下");
    assert_eq!(total_edges, 0);

    // 5. 邻域铺开也按当时的边找邻居
    let (_, edges) = utopia_store::graph::neighborhood(
        &pool,
        f.kb,
        f.zhang,
        1,
        None,
        Some(t("2026-03-10T00:00:00Z")),
    )
    .await?;
    assert_eq!(edge_ids(&edges), vec![f.retracted]);
    let (_, edges) = utopia_store::graph::neighborhood(&pool, f.kb, f.zhang, 1, None, None).await?;
    assert_eq!(edge_ids(&edges), vec![f.correction]);

    // 6. 点开节点，面板说的是**当时**的事实——回放的图上点开一个节点，
    //    侧栏还答今天的事，两个说法就并排摆在同一个屏幕上
    let facts = |as_of: Option<&'static str>| {
        let pool = pool.clone();
        async move {
            utopia_store::graph::entity_detail(&pool, f.kb, f.zhang, None, as_of.map(t))
                .await
                .map(|(_, facts)| facts)
        }
    };
    let now = facts(None).await?;
    assert_eq!(now.len(), 1);
    assert_eq!(now[0].id, f.correction);
    assert!(now[0].corrected, "修正行认得出自己改写了谁");

    let march = facts(Some("2026-03-10T00:00:00Z")).await?;
    assert_eq!(march.len(), 1);
    assert_eq!(march[0].id, f.retracted, "03-10 面板该给出当时那条");

    let before = facts(Some("2026-02-01T00:00:00Z")).await?;
    assert!(before.is_empty(), "记下之前不该有事实");

    // #351：同一秒内先录入再更正。拿更正时刻退一整秒会跳过旧记录，
    // 拿更正时刻本身则已经是新记录；工具提示的「前一微秒」必须能读回旧事实。
    let first = t("2026-03-20T02:43:53.382000Z");
    let changed = t("2026-03-20T02:43:53.382002Z");
    sqlx::query("UPDATE facts SET recorded_at = $2, invalidated_at = $3 WHERE id = $1")
        .bind(f.retracted)
        .bind(first)
        .bind(changed)
        .execute(&pool)
        .await?;
    sqlx::query("UPDATE facts SET recorded_at = $2 WHERE id = $1")
        .bind(f.correction)
        .bind(changed)
        .execute(&pool)
        .await?;

    let events = utopia_store::graph::graph_changes(
        &pool,
        f.kb,
        t("2026-03-20T00:00:00Z"),
        t("2026-03-21T00:00:00Z"),
        Some(f.zhang),
        None,
        10,
    )
    .await?;
    assert_eq!(events.len(), 2);
    assert_eq!(
        (events[0].at, events[0].kind.as_str()),
        (changed, "corrected")
    );
    assert_eq!((events[1].at, events[1].kind.as_str()), (first, "asserted"));

    for (moment, expected) in [
        (changed - chrono::Duration::seconds(1), None),
        (
            changed - chrono::Duration::microseconds(1),
            Some(f.retracted),
        ),
        (changed, Some(f.correction)),
    ] {
        let (_, rows) =
            utopia_store::graph::entity_detail(&pool, f.kb, f.zhang, None, Some(moment)).await?;
        let ids: Vec<_> = rows.iter().map(|row| row.id).collect();
        assert_eq!(
            ids,
            expected.into_iter().collect::<Vec<_>>(),
            "as_of={moment}"
        );
    }

    // 拆台：facts/entities/… 全是 ON DELETE CASCADE
    let gone = sqlx::query(
        "DELETE FROM organizations WHERE id = (
             SELECT w.org_id FROM workspaces w
             JOIN knowledge_bases k ON k.workspace_id = w.id WHERE k.id = $1)",
    )
    .bind(f.kb)
    .execute(&pool)
    .await?;
    assert_eq!(gone.rows_affected(), 1, "一次性 org 没删掉");
    Ok(())
}
