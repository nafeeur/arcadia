//! Agentic exploration for Ask-the-Data semantic mappings: reads a mounted source's schema
//! plus the KB's existing concepts, and has the LLM propose mappings from
//! "business concept (Metric/Dimension entity) → data-asset definition".
//!
//! Proposals are written into `concept_mappings` (status = proposed) → they get their own tab
//! on the Review page; after Confirm / Reject, status becomes confirmed, and Ask-the-Data only
//! reads the confirmed ones.
//!
//! **This used to be a `mapped_to` fact at 0.6 confidence**, riding along under the
//! "low-confidence fact" tab. See 0011 for why it moved out: it isn't an assertion about the
//! world, it's configuration — and "confirming" it used to mean `UPDATE facts SET confidence =
//! 1.0`, an in-place edit to a table that isn't supposed to allow in-place edits.
//! The agent only proposes; the human holds the power to make a mapping take effect — the same
//! philosophy as resolution's "when unsure, keep separate".

use crate::llm_util;
use crate::state::AppState;
use uuid::Uuid;

const MAX_SCHEMA_CHARS: usize = 12_000;

/// Exploration turns quantities and dimensions from the schema into Metric / Dimension
/// entities, but those two types aren't in any built-in ontology pack — since 0009, a new KB
/// no longer ships with its own types. Without them, the `type_id` lookup below finds nothing,
/// every proposal gets silently swallowed by `continue`, and the page just says "queued" with
/// no follow-up (#223). So exploration backfills the two types first: builtin, with a
/// description fed to the extraction prompt, editable afterwards on the ontology page
async fn ensure_concept_types(pool: &sqlx::PgPool, kb_id: Uuid) -> anyhow::Result<()> {
    for (key, label, description) in [
        (
            "metric",
            "Metric",
            "An aggregatable business quantity (revenue, order count, average ticket) that              maps to a definition in a mounted database.",
        ),
        (
            "dimension",
            "Dimension",
            "A group-by attribute (region, month, product line) that maps to a column in a              mounted database.",
        ),
    ] {
        sqlx::query(
            "INSERT INTO entity_types (id, kb_id, key, label, builtin, description)
             SELECT $1, $2, $3, $4, TRUE, $5
             WHERE NOT EXISTS (SELECT 1 FROM entity_types WHERE kb_id = $2 AND key = $3)",
        )
        .bind(Uuid::now_v7())
        .bind(kb_id)
        .bind(key)
        .bind(label)
        .bind(description)
        .execute(pool)
        .await?;
    }
    Ok(())
}

pub async fn explore_mappings(state: &AppState, kb_id: Uuid) -> anyhow::Result<()> {
    let kb = utopia_store::kbs::get(&state.pool, kb_id).await?;
    let settings = utopia_store::settings::get(&state.pool, kb.workspace_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Chat model not configured"))?;
    let client = llm_util::chat_client(&settings)
        .ok_or_else(|| anyhow::anyhow!("Chat model not configured"))?;

    let sources = utopia_store::datasources::mounted(&state.pool, kb_id).await?;
    if sources.is_empty() {
        anyhow::bail!("No data sources mounted");
    }
    ensure_concept_types(&state.pool, kb_id).await?;

    // Each source's schema (read directly from the engine to stay fresh; capped to avoid a prompt blowup)
    let mut schema_txt = String::new();
    for ds in &sources {
        let (engine, conn) = utopia_store::datasources::engine_and_conn(&state.pool, ds.id).await?;
        let cols = crate::query_engine::engine_for(&engine, &conn)?
            .fetch_schema()
            .await?;
        schema_txt.push_str(&format!("\n=== source: {} ===\n", ds.name));
        let mut current = String::new();
        for c in cols {
            let key = format!("{}.{}", c.schema, c.table);
            if key != current {
                current = key.clone();
                schema_txt.push_str(&format!("table {key}:\n"));
            }
            schema_txt.push_str(&format!(
                "  {} {}{}\n",
                c.column,
                c.data_type,
                c.comment.map(|x| format!(" -- {x}")).unwrap_or_default()
            ));
            if schema_txt.len() > MAX_SCHEMA_CHARS {
                schema_txt.push_str("(truncated)\n");
                break;
            }
        }
    }

    // 既有概念（供归并复用，避免重复起名）
    let existing: Vec<(String,)> = sqlx::query_as(
        "SELECT e.canonical_name FROM entities e
         JOIN entity_types t ON t.id = e.type_id
         WHERE e.kb_id = $1 AND e.merged_into IS NULL AND t.key IN ('metric','dimension')
         ORDER BY e.canonical_name LIMIT 100",
    )
    .bind(kb_id)
    .fetch_all(&state.pool)
    .await?;
    let existing_names: Vec<String> = existing.into_iter().map(|(n,)| n).collect();

    let prompt = format!(
        "You are building the semantic layer of a BI system. Given database schemas, propose \
         business concepts a user would ask about, each mapped to a concrete definition.\n\
         Existing concepts (reuse these names when the meaning matches): {}\n\
         Schemas:\n{}\n\
         Reply with ONLY a JSON array, each item:\n\
         {{\"name\": \"business concept name\", \"kind\": \"metric\"|\"dimension\", \
         \"source\": \"data source name\", \
         \"definition\": {{\"table\": \"schema.table\", \"expr\": \"SQL expression\", \
         \"sql\": \"full SELECT if joins are needed (optional)\", \"unit\": \"optional\"}}, \
         \"summary\": \"one line: source + expression, shown to reviewers\", \
         \"rationale\": \"why this mapping, citing column comments\"}}\n\
         Metrics are aggregatable quantities (use sum/count/avg in expr); dimensions are \
         group-by columns. Propose at most 12, only well-grounded ones.",
        if existing_names.is_empty() {
            "(none)".into()
        } else {
            existing_names.join(", ")
        },
        schema_txt
    );

    let _permit = llm_util::acquire_chat(state, &settings).await;
    let reply = client
        .chat(&[utopia_llm::ChatMessage {
            role: "user".into(),
            content: prompt,
        }])
        .await?;
    let json_str = reply
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    let proposals: Vec<serde_json::Value> = serde_json::from_str(json_str)
        .map_err(|e| anyhow::anyhow!("Mapping proposal parse error: {e}"))?;

    let source_names: Vec<&str> = sources.iter().map(|d| d.name.as_str()).collect();
    let mut accepted = 0usize;
    for p in proposals.iter().take(12) {
        let name = p["name"].as_str().map(str::trim).unwrap_or("");
        let kind = p["kind"].as_str().unwrap_or("");
        let source = p["source"].as_str().map(str::trim).unwrap_or("");
        if name.is_empty()
            || !matches!(kind, "metric" | "dimension")
            || !source_names.iter().any(|s| s.eq_ignore_ascii_case(source))
        {
            continue;
        }
        let type_id: Option<(Uuid,)> =
            sqlx::query_as("SELECT id FROM entity_types WHERE kb_id = $1 AND key = $2")
                .bind(kb_id)
                .bind(kind)
                .fetch_optional(&state.pool)
                .await?;
        let Some((type_id,)) = type_id else { continue };

        // 概念实体：走消解（同名归并；无向量上下文按 v1 兼容归并）。
        // 没有块原文可给——这些名字来自数据源的 schema 探索，不是从文档句子里抽的
        let resolved = utopia_store::resolution::resolve_mention(
            &state.pool,
            kb_id,
            Some(type_id),
            name,
            None,
            None,
            &[],
        )
        .await?;

        // 定义拆成列写进 concept_mappings（0011）。从前它是一份塞进
        // `object_value` 的 JSON，宾语挂在一条叫 mapped_to 的关系上——
        // 而那条关系是本体里的一行，跟 works_at 并列。**它不是关于世界的
        // 断言，是配置**，所以搬去自己的表
        let def = &p["definition"];
        if !def.is_object() {
            continue;
        }
        let s = |k: &str| {
            def[k]
                .as_str()
                .filter(|x| !x.is_empty())
                .map(str::to_string)
        };
        utopia_store::mappings::propose(
            &state.pool,
            kb_id,
            resolved.entity_id,
            source,
            s("table").as_deref(),
            s("expr").as_deref(),
            s("sql").as_deref(),
            s("unit").as_deref(),
            // summary 是给人看的那句：Review 列表与问数 prompt 都靠它
            p["summary"].as_str().or(def["summary"].as_str()),
            def["derived"].as_bool().unwrap_or(false),
        )
        .await?;
        accepted += 1;
    }

    tracing::info!(%kb_id, proposals = accepted, "映射探索完成，提议已入审核队列");
    // 一条都没提出来时页面上什么都不会变——Pending 还是 0，而"已排队"那句
    // 早就翻篇了。走告警中心说一声，人才知道该去刷新结构或给列加注释
    if accepted == 0 {
        if let Err(e) = utopia_store::alerts::raise(
            &state.pool,
            utopia_store::alerts::NewAlert {
                kb_id: Some(kb_id),
                severity: "info",
                kind: utopia_store::alerts::kind::MAPPING_EXPLORATION_EMPTY,
                min_role: utopia_core::models::Role::Editor,
                subject_type: None,
                subject_id: None,
                detail: serde_json::json!({ "proposals": 0, "sources": source_names }),
            },
        )
        .await
        {
            tracing::warn!(%kb_id, error = %e, "映射探索空结果的告警没写进去");
        }
        state.emit_alert();
    }
    state.emit_review(kb_id);
    Ok(())
}
