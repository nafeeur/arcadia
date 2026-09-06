//! #268: deleting a document is an event on the cognitive axis, not subtraction.
//!
//! Previously `documents::delete` was a single DELETE, and foreign keys cascaded
//! away the chunks and evidence with it: facts stayed in the graph, still live, but
//! with no source left. Now the document gets a tombstone, its chunks get marked
//! superseded, **no content is ever cleared**, and only facts whose every source has
//! been deleted are invalidated. Five things are guarded here:
//!
//! 1. **Only invalidate facts with no other source.** A fact vouched for by both
//!    documents A and B is untouched when A is deleted; a fact vouched for only by A
//!    is invalidated. A's chunks are marked superseded but the body text stays; the
//!    document no longer appears in the library listing.
//! 2. **Deleting twice is an error.**
//! 3. **Restoring reverses it exactly.** The document, the chunks marked superseded
//!    this time, and the facts invalidated this time all come back; a fact that was
//!    already invalidated before the deletion is not on that list and is not
//!    mistakenly revived. Restoring a document that was never deleted is an error.
//! 4. **Re-uploading identical content revives the tombstone**: the same id comes
//!    back instead of a unique-index "already exists" error.
//! 5. **The test is the document, not the chunk.** Evidence still pinned to an old
//!    chunk version is stale, not sourceless — it is not invalidated.
//!
//! Skips rather than fails when `UTOPIA_DATABASE_URL` is unset. Builds and tears
//! down its own data; never touches an existing database.

use sqlx::PgPool;
use utopia_store::documents;
use uuid::Uuid;

struct Fx {
    org: Uuid,
    user: Uuid,
    kb: Uuid,
    src: Uuid,
    etype: Uuid,
    rel: Uuid,
}

async fn seed(pool: &PgPool) -> anyhow::Result<Fx> {
    let (org, ws, kb, user) = (
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
    );
    let (src, etype, rel) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    sqlx::query("INSERT INTO organizations (id, name) VALUES ($1, 'deletion-test')")
        .bind(org)
        .execute(pool)
        .await?;
    sqlx::query("INSERT INTO workspaces (id, org_id, name) VALUES ($1, $2, 'deletion-test')")
        .bind(ws)
        .bind(org)
        .execute(pool)
        .await?;
    sqlx::query(
        "INSERT INTO users (id, org_id, email, display_name, password_hash)
         VALUES ($1, $2, $1 || '@deletion.test', 'd', 'x')",
    )
    .bind(user)
    .bind(org)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO knowledge_bases (id, workspace_id, name) VALUES ($1, $2, 'deletion-test')",
    )
    .bind(kb)
    .bind(ws)
    .execute(pool)
    .await?;
    sqlx::query("INSERT INTO sources (id, kb_id, kind, name) VALUES ($1, $2, 'folder', 'f')")
        .bind(src)
        .bind(kb)
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
        "INSERT INTO relation_types (id, kb_id, key, label) VALUES ($1, $2, 'knows', 'knows')",
    )
    .bind(rel)
    .bind(kb)
    .execute(pool)
    .await?;
    Ok(Fx {
        org,
        user,
        kb,
        src,
        etype,
        rel,
    })
}

async fn document(pool: &PgPool, f: &Fx, name: &str, sha: &str) -> anyhow::Result<Uuid> {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO documents (id, kb_id, source_id, filename, sha256, status)
         VALUES ($1, $2, $3, $4, $5, 'ready')",
    )
    .bind(id)
    .bind(f.kb)
    .bind(f.src)
    .bind(name)
    .bind(sha)
    .execute(pool)
    .await?;
    Ok(id)
}

async fn chunk(pool: &PgPool, f: &Fx, doc: Uuid, version: i32) -> anyhow::Result<Uuid> {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO chunks (id, kb_id, document_id, seq, text, doc_version, superseded_at)
         VALUES ($1, $2, $3, 0, 'the text stays', $4, CASE WHEN $4 = 1 THEN NULL ELSE now() END)",
    )
    .bind(id)
    .bind(f.kb)
    .bind(doc)
    .bind(version)
    .execute(pool)
    .await?;
    Ok(id)
}

async fn entity(pool: &PgPool, f: &Fx, name: &str) -> anyhow::Result<Uuid> {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO entities (id, kb_id, type_id, canonical_name) VALUES ($1, $2, $3, $4)",
    )
    .bind(id)
    .bind(f.kb)
    .bind(f.etype)
    .bind(name)
    .execute(pool)
    .await?;
    Ok(id)
}

/// 一条事实，出处是给定的几个分块
async fn fact(pool: &PgPool, f: &Fx, s: Uuid, o: Uuid, evidence: &[Uuid]) -> anyhow::Result<Uuid> {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO facts (id, kb_id, subject_id, predicate_id, object_id, confidence)
         VALUES ($1, $2, $3, $4, $5, 0.9)",
    )
    .bind(id)
    .bind(f.kb)
    .bind(s)
    .bind(f.rel)
    .bind(o)
    .execute(pool)
    .await?;
    for c in evidence {
        sqlx::query("INSERT INTO fact_evidence (fact_id, chunk_id, quote) VALUES ($1, $2, 'q')")
            .bind(id)
            .bind(c)
            .execute(pool)
            .await?;
    }
    Ok(id)
}

async fn live(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    Ok(
        sqlx::query_scalar("SELECT invalidated_at IS NULL FROM facts WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await?,
    )
}

async fn chunk_live(pool: &PgPool, id: Uuid) -> anyhow::Result<(bool, String)> {
    Ok(
        sqlx::query_as("SELECT superseded_at IS NULL, text FROM chunks WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await?,
    )
}

async fn listed(pool: &PgPool, f: &Fx, id: Uuid) -> anyhow::Result<bool> {
    Ok(documents::list(pool, f.kb)
        .await?
        .iter()
        .any(|d| d.id == id))
}

#[tokio::test]
async fn a_deletion_is_an_event() -> anyhow::Result<()> {
    let Some(url) = utopia_store::test_db::url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    let f = seed(&pool).await?;

    let run = async {
        let a = document(&pool, &f, "a.md", "sha-a").await?;
        let b = document(&pool, &f, "b.md", "sha-b").await?;
        let a1 = chunk(&pool, &f, a, 1).await?;
        let b1 = chunk(&pool, &f, b, 1).await?;
        let (x, y, z, w) = (
            entity(&pool, &f, "X").await?,
            entity(&pool, &f, "Y").await?,
            entity(&pool, &f, "Z").await?,
            entity(&pool, &f, "W").await?,
        );
        // 只有甲作证；甲乙都作证；甲作证但删之前就作废了
        let only_a = fact(&pool, &f, x, y, &[a1]).await?;
        let both = fact(&pool, &f, x, z, &[a1, b1]).await?;
        let already_gone = fact(&pool, &f, x, w, &[a1]).await?;
        sqlx::query("UPDATE facts SET invalidated_at = now() WHERE id = $1")
            .bind(already_gone)
            .execute(&pool)
            .await?;

        // 1. 删甲
        let report = documents::delete(&pool, f.kb, a, Some(f.user)).await?;
        assert_eq!(
            report.invalidated_facts, 1,
            "only the fact with no other source"
        );
        assert_eq!(report.superseded_chunks, 1);
        assert!(!live(&pool, only_a).await?);
        assert!(live(&pool, both).await?, "b still vouches for it");
        assert!(!listed(&pool, &f, a).await?, "gone from the library");
        assert!(listed(&pool, &f, b).await?);
        let (a1_live, text) = chunk_live(&pool, a1).await?;
        assert!(!a1_live);
        assert_eq!(text, "the text stays", "nothing is cleared");
        assert!(
            documents::get(&pool, a).await?.deleted_at.is_some(),
            "a tombstone, not a hole"
        );
        let (facts, chunks, reverted): (
            Vec<Uuid>,
            Vec<Uuid>,
            Option<chrono::DateTime<chrono::Utc>>,
        ) = sqlx::query_as(
            "SELECT invalidated_facts, superseded_chunks, reverted_at
                   FROM document_deletions WHERE document_id = $1",
        )
        .bind(a)
        .fetch_one(&pool)
        .await?;
        assert_eq!(facts, vec![only_a]);
        assert_eq!(chunks, vec![a1]);
        assert!(reverted.is_none());

        // 2. 删两次
        assert!(documents::delete(&pool, f.kb, a, Some(f.user))
            .await
            .is_err());

        // 3. 撤销原路回来；删之前就作废的不被误救
        let doc = documents::restore(&pool, f.kb, a).await?;
        assert!(doc.deleted_at.is_none());
        assert!(live(&pool, only_a).await?);
        assert!(live(&pool, both).await?);
        assert!(
            !live(&pool, already_gone).await?,
            "retired before the deletion, so not on the list"
        );
        assert!(chunk_live(&pool, a1).await?.0);
        assert!(listed(&pool, &f, a).await?);
        let reverted: Option<chrono::DateTime<chrono::Utc>> =
            sqlx::query_scalar("SELECT reverted_at FROM document_deletions WHERE document_id = $1")
                .bind(a)
                .fetch_one(&pool)
                .await?;
        assert!(reverted.is_some());
        assert!(documents::restore(&pool, f.kb, a).await.is_err());

        // 4. 删掉再传同一份内容：同一个 id 回来
        documents::delete(&pool, f.kb, a, Some(f.user)).await?;
        assert!(!live(&pool, only_a).await?);
        let back = documents::create(
            &pool,
            f.kb,
            "a-again.md",
            "text/markdown",
            14,
            "sha-a",
            Some(f.src),
            None,
            None,
        )
        .await?;
        assert_eq!(back.id, a, "the same document, revived");
        assert!(back.deleted_at.is_none());
        assert!(live(&pool, only_a).await?);
        assert!(chunk_live(&pool, a1).await?.0);

        // 5. 判据看文档：乙的证据停在旧版分块上，删甲时乙还活着，事实不作废
        let b0 = chunk(&pool, &f, b, 0).await?;
        let stale_but_sourced = fact(&pool, &f, y, z, &[a1, b0]).await?;
        let report = documents::delete(&pool, f.kb, a, Some(f.user)).await?;
        assert_eq!(
            report.invalidated_facts, 1,
            "only_a again; the stale one keeps its source"
        );
        assert!(
            live(&pool, stale_but_sourced).await?,
            "an old version of b is still b, not a missing source"
        );
        Ok::<_, anyhow::Error>(())
    }
    .await;

    // 组织删除不级联到库：库自己删，别在共用的测试库里留墓碑
    let _ = sqlx::query("DELETE FROM knowledge_bases WHERE id = $1")
        .bind(f.kb)
        .execute(&pool)
        .await;
    let _ = sqlx::query("DELETE FROM organizations WHERE id = $1")
        .bind(f.org)
        .execute(&pool)
        .await;
    run
}
