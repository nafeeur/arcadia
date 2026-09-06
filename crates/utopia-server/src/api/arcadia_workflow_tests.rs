//! HTTP acceptance with real SQL and a deterministic local model fixture.
use crate::{auth, state::AppState};
use serde_json::{json, Value};
use std::sync::Arc;
use utopia_core::config::AppConfig;
use utopia_store::{arcadia, conversations};
use uuid::Uuid;

#[tokio::test]
async fn permissions_and_scenario_replay_over_http() -> anyhow::Result<()> {
    let Some(url) = utopia_store::test_db::url() else {
        return Ok(());
    };
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await?;
    utopia_store::db::migrate(&pool).await?;
    let org = Uuid::now_v7();
    let ws = Uuid::now_v7();
    let kb = Uuid::now_v7();
    let admin = Uuid::now_v7();
    let editor = Uuid::now_v7();
    let viewer = Uuid::now_v7();
    let doc = Uuid::now_v7();
    let chunk = Uuid::now_v7();
    sqlx::query("INSERT INTO organizations(id,name) VALUES($1,'arcadia-http')")
        .bind(org)
        .execute(&pool)
        .await?;
    sqlx::query("INSERT INTO workspaces(id,org_id,name) VALUES($1,$2,'arcadia-http')")
        .bind(ws)
        .bind(org)
        .execute(&pool)
        .await?;
    sqlx::query("INSERT INTO knowledge_bases(id,workspace_id,name) VALUES($1,$2,'arcadia-http')")
        .bind(kb)
        .bind(ws)
        .execute(&pool)
        .await?;
    for (id, role) in [(admin, "admin"), (editor, "editor"), (viewer, "viewer")] {
        sqlx::query("INSERT INTO users(id,org_id,email,password_hash,display_name) VALUES($1,$2,$3,'unused','fixture')")
            .bind(id).bind(org).bind(format!("{id}@example.test")).execute(&pool).await?;
        sqlx::query("INSERT INTO kb_members(kb_id,user_id,role) VALUES($1,$2,$3)")
            .bind(kb)
            .bind(id)
            .bind(role)
            .execute(&pool)
            .await?;
    }
    sqlx::query("INSERT INTO documents(id,kb_id,filename,sha256,status) VALUES($1,$2,'price.txt',$3,'ready')")
        .bind(doc).bind(kb).bind(format!("fixture-{doc}")).execute(&pool).await?;
    sqlx::query("INSERT INTO chunks(id,kb_id,document_id,seq,text) VALUES($1,$2,$3,0,'Cobalt price twelve')")
        .bind(chunk).bind(kb).bind(doc).execute(&pool).await?;
    let conv = conversations::create(&pool, kb, admin, "Cobalt").await?;
    let trace = conversations::append_message(&pool,conv,"assistant","Twelve [1]", &conversations::TurnRecord {
        metadata:json!({"question":"Cobalt","model":"fixture"}),
        sources:json!([{"n":1,"document_id":doc,"chunk_id":chunk,"filename":"price.txt","text":"Cobalt price twelve"}]),
        ..conversations::TurnRecord::empty()
    }).await?;
    let model = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/chat/completions"))
        .respond_with(|r: &wiremock::Request| {
            let body: Value = r.body_json().unwrap();
            let text = body["messages"][0]["content"].as_str().unwrap();
            let answer = if text.contains("Cobalt price fourteen") {
                "Fourteen [1]"
            } else {
                "Twelve [1]"
            };
            wiremock::ResponseTemplate::new(200)
                .set_body_json(json!({"choices":[{"message":{"content":answer}}]}))
        })
        .mount(&model)
        .await;
    utopia_store::settings::upsert(
        &pool,
        ws,
        Some(&model.uri()),
        None,
        Some("fixture"),
        None,
        None,
        None,
        None,
    )
    .await?;
    let tmp = std::env::temp_dir().join(format!("arcadia-http-{org}"));
    let cfg = AppConfig {
        data_dir: tmp.to_string_lossy().into(),
        web_dist: tmp.join("no-web").to_string_lossy().into(),
        ..AppConfig::default()
    };
    let index = Arc::new(utopia_search::SearchIndex::open(&tmp.join("index"))?);
    let state = AppState::new(pool.clone(), &cfg, index, "fixture-jwt-only".into());
    let app = super::router(state.clone(), &cfg);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}/api/v1/kbs/{kb}/arcadia", listener.local_addr()?);
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let http = reqwest::Client::new();
    let admin_token = auth::issue_token(&state, admin)?;
    let editor_token = auth::issue_token(&state, editor)?;
    let viewer_token = auth::issue_token(&state, viewer)?;
    let result: anyhow::Result<()> = async {
        assert_eq!(http.get(format!("{base}/overview")).send().await?.status(),401);
        assert_eq!(http.get(format!("{base}/traces/{trace}")).bearer_auth(&viewer_token).send().await?.status(),404);
        let proposal = json!({"document_id":doc,"title":"New agreement","reason":"Signed amendment","content":"Cobalt price fourteen"});
        assert_eq!(http.post(format!("{base}/changes")).bearer_auth(&viewer_token).json(&proposal).send().await?.status(),403);
        let change: Value = http.post(format!("{base}/changes")).bearer_auth(&editor_token).json(&proposal).send().await?.error_for_status()?.json().await?;
        let id = change["id"].as_str().unwrap();
        assert_eq!(http.post(format!("{base}/changes/{id}/decide")).bearer_auth(&editor_token).json(&json!({"approve":true})).send().await?.status(),403);
        let historical:Value = http.post(format!("{base}/traces/{trace}/replay")).bearer_auth(&admin_token)
            .json(&json!({"as_of":chrono::Utc::now().to_rfc3339()})).send().await?.error_for_status()?.json().await?;
        assert_eq!(historical["answer"],"Twelve [1]");
        let preview:Value = http.post(format!("{base}/traces/{trace}/replay")).bearer_auth(&admin_token)
            .json(&json!({"change_id":id})).send().await?.error_for_status()?.json().await?;
        assert_eq!(preview["answer"],"Fourteen [1]");
        assert_eq!(preview["evidence"][0]["proposed"],true);
        assert_eq!(preview["metadata"]["validation"]["semantic_accuracy"],"not_evaluated");
        assert_eq!(arcadia::change(&pool,kb,id.parse()?).await?["status"],"pending");
        assert_eq!(arcadia::replays(&pool,trace).await?.len(),2);
        http.post(format!("{base}/changes/{id}/decide")).bearer_auth(&admin_token)
            .json(&json!({"approve":true,"note":"Reviewed"})).send().await?.error_for_status()?;
        assert_eq!(arcadia::change(&pool,kb,id.parse()?).await?["status"],"approved");
        Ok(())
    }.await;
    server.abort();
    sqlx::query("DELETE FROM organizations WHERE id=$1")
        .bind(org)
        .execute(&pool)
        .await?;
    drop(state);
    let _ = std::fs::remove_dir_all(tmp);
    result
}
