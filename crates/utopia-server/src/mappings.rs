//! Agentic exploration for ask-data semantic mappings: reads mounted sources' schema + the
//! KB's existing concepts, and has the LLM propose "business concept (Metric/Dimension
//! entity) → data asset definition" mappings.
//!
//! Proposals are written to `concept_mappings` (status = proposed) → land in their own
//! category on the Review page; after Confirm / Reject, status becomes confirmed, and
//! ask-data only reads the confirmed ones.
//!
//! **This used to be a `mapped_to` fact at 0.6 confidence**, surfacing under the
//! "low-confidence facts" category. See 0011 for why it moved out: it isn't an assertion
//! about the world, it's configuration — and "confirming" it used to mean
//! `UPDATE facts SET confidence = 1.0`, an in-place edit to a table that isn't supposed to
//! allow in-place edits. The agent only proposes; the human holds the authority to make a
//! definition effective — same philosophy as resolution's "split rather than merge".

use crate::llm_util;
use crate::state::AppState;
use uuid::Uuid;

const MAX_SCHEMA_CHARS: usize = 12_000;

/// Exploration turns quantities and dimensions from the schema into Metric / Dimension
/// entities, but neither class is in any built-in ontology pack — since 0009, creating a KB
/// no longer ships classes by default. Without them, the `type_id` lookup below fails,
/// every proposal gets swallowed by `continue`, and the page just says "queued" with no
/// follow-up (#223). So we backfill both classes before exploring: builtin, with a
/// description meant for the extraction prompt, editable from the ontology page
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

    // Schema per source (read live from the engine to stay fresh; capped to avoid prompt blowup)
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

    // Existing concepts (for merge/reuse, to avoid re-naming the same thing)
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

        // Concept entity: goes through resolution (same-name merge; with no vector context,
        // merges per v1-compatible behavior).
        // No chunk text to provide — these names come from data source schema exploration,
        // not extracted from document sentences
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

        // The definition is split into columns and written to concept_mappings (0011). It
        // used to be a JSON blob stuffed into `object_value`, with the object hanging off a
        // relation called mapped_to — a relation that was just a row in the ontology,
        // sitting alongside works_at. **It's not an assertion about the world, it's
        // configuration**, so it moved to its own table
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
            // summary is the human-facing line: both the Review list and the ask-data prompt rely on it
            p["summary"].as_str().or(def["summary"].as_str()),
            def["derived"].as_bool().unwrap_or(false),
        )
        .await?;
        accepted += 1;
    }

    tracing::info!(%kb_id, proposals = accepted, "映射探索完成，提议已入审核队列");
    // When nothing gets proposed, nothing on the page changes — Pending stays 0, and the
    // "queued" message is long gone. Raise it through the alert center instead, so someone
    // knows to go refresh the schema or add column comments
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
            tracing::warn!(%kb_id, error = %e, "映射探索空结果的告警没写进去"); // failed to persist the empty-result alert for mapping exploration
        }
        state.emit_alert();
    }
    state.emit_review(kb_id);
    Ok(())
}
