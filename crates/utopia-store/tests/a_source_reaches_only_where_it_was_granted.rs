//! Data source authorization (0014).
//!
//! The hole this patches: registration is a deployment-level action, while
//! the mount guard is `require_kb(kb_id, Role::Admin)` — admin of the
//! requester's own knowledge base. But the mountable list returns every
//! source in the whole deployment. So any knowledge base's admin could mount
//! any production database into their own base, and once mounted every
//! Viewer of that base could run read-only SQL against it via `query_data`.
//!
//! This pins down three things:
//!
//! - **Invisible**: an unauthorized source doesn't appear in the mountable
//!   list
//! - **Also unmountable**: the list filter only blocks "visible", while the
//!   mount endpoint is called by id — the guard must be on both sides, and
//!   this test covers the endpoint side
//! - **Revocation actually revokes**: revoking authorization unmounts
//!   whatever was already mounted along with it. Deleting only the grant row
//!   would leave `kb_data_sources` as what query answers still read from,
//!   i.e. the revocation wouldn't take effect — an ineffective permission
//!   revocation is more dangerous than none at all

use sqlx::PgPool;
use utopia_store::datasources;
use uuid::Uuid;

struct Fixture {
    org: Uuid,
    /// The workspace authorized for it
    ours: Uuid,
    /// The unauthorized workspace — its knowledge base shouldn't be able to reach this source
    theirs: Uuid,
    our_kb: Uuid,
    their_kb: Uuid,
    source: Uuid,
    actor: Uuid,
}

async fn seed(pool: &PgPool) -> anyhow::Result<Fixture> {
    let org = Uuid::now_v7();
    let (ours, theirs) = (Uuid::now_v7(), Uuid::now_v7());
    let (our_kb, their_kb) = (Uuid::now_v7(), Uuid::now_v7());
    let (source, actor) = (Uuid::now_v7(), Uuid::now_v7());

    sqlx::query("INSERT INTO organizations (id, name) VALUES ($1, 'grant-test')")
        .bind(org)
        .execute(pool)
        .await?;
    for (id, name) in [(ours, "ours"), (theirs, "theirs")] {
        sqlx::query("INSERT INTO workspaces (id, org_id, name) VALUES ($1, $2, $3)")
            .bind(id)
            .bind(org)
            .bind(name)
            .execute(pool)
            .await?;
    }
    for (kb, ws, name) in [(our_kb, ours, "our-kb"), (their_kb, theirs, "their-kb")] {
        sqlx::query("INSERT INTO knowledge_bases (id, workspace_id, name) VALUES ($1, $2, $3)")
            .bind(kb)
            .bind(ws)
            .bind(name)
            .execute(pool)
            .await?;
    }
    sqlx::query(
        "INSERT INTO users (id, org_id, email, password_hash, display_name, is_admin)
         VALUES ($1, $2, $3, 'x', 'Grant Test', TRUE)",
    )
    .bind(actor)
    .bind(org)
    .bind(format!("{actor}@grant.test"))
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO data_sources (id, name, engine, conn_string, created_by)
         VALUES ($1, $2, 'postgres', 'postgres://u:p@db.test:5432/w', $3)",
    )
    .bind(source)
    .bind(format!("warehouse-{source}"))
    .bind(actor)
    .execute(pool)
    .await?;

    Ok(Fixture {
        org,
        ours,
        theirs,
        our_kb,
        their_kb,
        source,
        actor,
    })
}

#[tokio::test]
async fn a_source_reaches_only_where_it_was_granted() -> anyhow::Result<()> {
    let Some(url) = utopia_store::test_db::url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    let f = seed(&pool).await?;

    let run = async {
        // ---- 1. Unauthorized = nobody can reach it
        assert!(
            datasources::granted_to_workspace(&pool, f.ours)
                .await?
                .is_empty(),
            "没授权过，可挂载列表就该是空的"
        );
        assert!(
            !datasources::is_granted(&pool, f.our_kb, f.source).await?,
            "没授权，挂载端点的守卫必须说不"
        );

        // ---- 2. Authorizing one workspace doesn't affect the other
        datasources::grant(&pool, f.source, f.ours, f.actor).await?;
        let ours = datasources::granted_to_workspace(&pool, f.ours).await?;
        assert_eq!(ours.len(), 1, "授权过的工作区看得见它");
        assert!(
            !ours[0].summary.contains("p@"),
            "列表里只能有 host:port/db 摘要，凭据不出服务端"
        );
        assert!(
            datasources::granted_to_workspace(&pool, f.theirs)
                .await?
                .is_empty(),
            "**授权是逐工作区的**：给了一个不等于给了全部署"
        );

        // ---- 3. The endpoint-side guard: the list filter only blocks "visible"
        assert!(datasources::is_granted(&pool, f.our_kb, f.source).await?);
        assert!(
            !datasources::is_granted(&pool, f.their_kb, f.source).await?,
            "没授权的工作区，就算自己拼一个 uuid 打过来也挂不上"
        );

        // ---- 4. A source can be authorized to multiple workspaces (many-to-many, not one-to-many)
        datasources::grant(&pool, f.source, f.theirs, f.actor).await?;
        assert_eq!(
            datasources::grants_for_source(&pool, f.source).await?.len(),
            2,
            "同一个数仓要能同时服务多个工作区"
        );
        datasources::grant(&pool, f.source, f.theirs, f.actor).await?;
        assert_eq!(
            datasources::grants_for_source(&pool, f.source).await?.len(),
            2,
            "重复授权是幂等的"
        );

        // ---- 5. Revoking also revokes the mount
        datasources::mount(&pool, f.our_kb, f.source).await?;
        datasources::mount(&pool, f.their_kb, f.source).await?;
        let unmounted = datasources::revoke(&pool, f.source, f.theirs).await?;
        assert_eq!(unmounted, 1, "撤授权要把那个工作区里已挂上的一起卸掉");
        assert!(
            datasources::mounted(&pool, f.their_kb).await?.is_empty(),
            "**留着挂载 = 撤销不生效**：问数读的是 kb_data_sources"
        );
        assert_eq!(
            datasources::mounted(&pool, f.our_kb).await?.len(),
            1,
            "另一个工作区的挂载不受牵连"
        );
        Ok::<_, anyhow::Error>(())
    }
    .await;

    sqlx::query("DELETE FROM data_sources WHERE id = $1")
        .bind(f.source)
        .execute(&pool)
        .await?;
    sqlx::query("DELETE FROM organizations WHERE id = $1")
        .bind(f.org)
        .execute(&pool)
        .await?;
    run
}
