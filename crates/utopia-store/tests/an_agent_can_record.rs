//! A memory proposed via MCP must show **which agent** on the review card (#304 / 0026).
//!
//! Why this has to hit a real database: this identity chain is carried entirely by
//! SQL — a foreign key column, a LEFT JOIN, an `AS` alias. A wrong column name, a join
//! against the wrong table, or an alias that doesn't line up with the `FromRow` field —
//! `cargo check` won't say a word about any of it; the UI will just quietly render half
//! a line short.
//!
//! Both cases need asserting, because they break in different ways:
//! - A proposal with a token: the view must surface the token's name (a missing join →
//!   always null)
//! - A proposal without a token (a web chat): that field must be null (a join written as
//!   an inner join → the whole row disappears)
//!
//! Self-contained: a throwaway org/workspace/kb, deleted along with the org when done.

use sqlx::PgPool;
use utopia_store::graph::Validity;
use utopia_store::pending::{Outcome, Proposal};
use uuid::Uuid;

struct Fixture {
    org: Uuid,
    kb: Uuid,
    /// The one recorded via token
    by_agent: Uuid,
    /// The one recorded via web chat
    by_person: Uuid,
}

async fn seed(pool: &PgPool) -> anyhow::Result<Fixture> {
    let (org, ws, kb) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    let (user, token) = (Uuid::now_v7(), Uuid::now_v7());
    let (etype, doc, chunk) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    let (acme, zenith) = (Uuid::now_v7(), Uuid::now_v7());
    let tag = Uuid::now_v7();

    sqlx::query("INSERT INTO organizations (id, name) VALUES ($1, 'agent-test')")
        .bind(org)
        .execute(pool)
        .await?;
    sqlx::query("INSERT INTO workspaces (id, org_id, name) VALUES ($1, $2, 'agent-test')")
        .bind(ws)
        .bind(org)
        .execute(pool)
        .await?;
    sqlx::query(
        "INSERT INTO knowledge_bases (id, workspace_id, name) VALUES ($1, $2, 'agent-test')",
    )
    .bind(kb)
    .bind(ws)
    .execute(pool)
    .await?;
    // Email must be unique: add a throwaway suffix so it doesn't collide with leftover accounts
    sqlx::query(
        "INSERT INTO users (id, org_id, email, password_hash, display_name)
         VALUES ($1, $2, $3, 'x', 'Zhang San')",
    )
    .bind(user)
    .bind(org)
    .bind(format!("agent-test-{tag}@utopia.test"))
    .execute(pool)
    .await?;
    // token_hash must also be unique
    sqlx::query(
        "INSERT INTO personal_tokens (id, user_id, name, token_hash, token_prefix, scope)
         VALUES ($1, $2, 'Meeting notes agent', $3, 'utp_pat_test', 'write')",
    )
    .bind(token)
    .bind(user)
    .bind(format!("hash-{tag}"))
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO entity_types (id, kb_id, key, label) VALUES ($1, $2, 'org', 'Organization')",
    )
    .bind(etype)
    .bind(kb)
    .execute(pool)
    .await?;
    for (id, name) in [(acme, "Acme"), (zenith, "Zenith")] {
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
    // Blobs are shared across databases by sha, so the sha also gets a throwaway suffix
    sqlx::query(
        "INSERT INTO documents (id, kb_id, filename, sha256, status)
         VALUES ($1, $2, 'memory-log.md', $3, 'ready')",
    )
    .bind(doc)
    .bind(kb)
    .bind(format!("sha-{tag}"))
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO chunks (id, kb_id, document_id, seq, text)
         VALUES ($1, $2, $3, 0, 'Acme partnered with Zenith on 2026-03-01.')",
    )
    .bind(chunk)
    .bind(kb)
    .bind(doc)
    .execute(pool)
    .await?;

    let propose = |object_id: Uuid, proposed_token: Option<Uuid>| {
        let p = Proposal {
            kb_id: kb,
            subject_id: acme,
            predicate_id: None,
            object_id: Some(object_id),
            object_value: None,
            proposed_predicate: Some("partnered with"),
            validity: Validity::default(),
            confidence: 0.6,
            chunk_id: chunk,
            proposed_by: Some(user),
            proposed_token,
        };
        async move {
            match utopia_store::pending::propose(pool, p).await? {
                Outcome::Proposed(id) => Ok::<Uuid, anyhow::Error>(id),
                other => anyhow::bail!("提议没有入队：{other:?}"),
            }
        }
    };
    let by_agent = propose(zenith, Some(token)).await?;
    // The same (subject, predicate, object) would be judged AlreadyPending, so the
    // second one uses a different object
    let by_person = propose(acme, None).await?;

    Ok(Fixture {
        org,
        kb,
        by_agent,
        by_person,
    })
}

#[tokio::test]
async fn a_pending_fact_names_the_agent_that_proposed_it() -> anyhow::Result<()> {
    let Some(url) = utopia_store::test_db::url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    let f = seed(&pool).await?;

    let queue = utopia_store::pending::list(&pool, f.kb, 50, 0).await?;
    let find = |id: Uuid| {
        queue
            .iter()
            .find(|v| v.id == id)
            .expect("待确认项不在队列里")
    };

    // 1. The one recorded via token: both the person and the agent are answerable.
    //    The person is identity, the agent is "which client" — when one person has
    //    three agents attached, only the latter tells them apart on the card
    let agent = find(f.by_agent);
    assert_eq!(agent.proposed_by_name.as_deref(), Some("Zhang San"));
    assert_eq!(
        agent.proposed_token_name.as_deref(),
        Some("Meeting notes agent"),
        "令牌名没跟出来——多半是 VIEW_SELECT 少了那个 join"
    );

    // 2. The one recorded via web chat: no agent, but **this row itself must not
    //    disappear** (a join written as an inner join makes the whole row vanish,
    //    which is the hardest kind of loss to notice)
    let person = find(f.by_person);
    assert_eq!(person.proposed_by_name.as_deref(), Some("Zhang San"));
    assert_eq!(person.proposed_token_name, None);

    // **Delete the kb before the org.** Deleting the org cascades to users, and
    // `pending_facts.proposed_by` is a foreign key with no ON DELETE (the ledger is
    // meant to block this kind of deletion); the kb doesn't cascade with the org, so
    // doing it in the wrong order gets rejected by the foreign key
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
