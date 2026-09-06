//! Real database acceptance: temporal lexical recall, owner isolation, stale approval,
//! atomic version/job creation and answer evidence capture.
use chrono::{DateTime, Utc};
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use utopia_store::{arcadia, conversations, documents};
use uuid::Uuid;
fn t(s: &str) -> DateTime<Utc> {
    s.parse().unwrap()
}

#[tokio::test]
async fn historical_keyword_search_and_review_workflow() -> anyhow::Result<()> {
    let Some(url) = utopia_store::test_db::url() else {
        return Ok(());
    };
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await?;
    utopia_store::db::migrate(&pool).await?;
    let org = Uuid::now_v7();
    let ws = Uuid::now_v7();
    let kb = Uuid::now_v7();
    let user = Uuid::now_v7();
    let other = Uuid::now_v7();
    let doc = Uuid::now_v7();
    let old = Uuid::now_v7();
    let current = Uuid::now_v7();
    sqlx::query("INSERT INTO organizations(id,name) VALUES($1,'arcadia-test')")
        .bind(org)
        .execute(&pool)
        .await?;
    let result: anyhow::Result<()>=async {
 sqlx::query("INSERT INTO workspaces(id,org_id,name) VALUES($1,$2,'arcadia-test')").bind(ws).bind(org).execute(&pool).await?;
 sqlx::query("INSERT INTO knowledge_bases(id,workspace_id,name) VALUES($1,$2,'arcadia-test')").bind(kb).bind(ws).execute(&pool).await?;
 for id in [user,other]{sqlx::query("INSERT INTO users(id,org_id,email,password_hash,display_name) VALUES($1,$2,$3,'unused','tester')").bind(id).bind(org).bind(format!("{id}@example.test")).execute(&pool).await?;}
 sqlx::query("INSERT INTO documents(id,kb_id,filename,sha256,status,created_at) VALUES($1,$2,'contract.txt',$3,'ready',$4)").bind(doc).bind(kb).bind(format!("base-{doc}")).bind(t("2026-01-01T00:00:00Z")).execute(&pool).await?;
 for (id,text,start,end) in [(old,"Cobalt price ten",t("2026-01-01T00:00:00Z"),Some(t("2026-03-01T00:00:00Z"))),(current,"Cobalt price twelve",t("2026-03-01T00:00:00Z"),None)] {
  sqlx::query("INSERT INTO chunks(id,kb_id,document_id,seq,text,created_at,superseded_at) VALUES($1,$2,$3,0,$4,$5,$6)").bind(id).bind(kb).bind(doc).bind(text).bind(start).bind(end).execute(&pool).await?;
 }
 sqlx::query("UPDATE chunks SET embedding='[1,0,0]'::vector WHERE document_id=$1").bind(doc).execute(&pool).await?;
 assert_eq!(documents::vector_search(&pool,kb,&[1.0,0.0,0.0],20,Some(t("2026-02-01T00:00:00Z"))).await?,vec![old]);
 assert_eq!(documents::lexical_search(&pool,kb,"Cobalt",20,Some(t("2026-02-01T00:00:00Z"))).await?,vec![old]);
 assert_eq!(documents::lexical_search(&pool,kb,"Cobalt",20,Some(t("2026-03-01T00:00:00Z"))).await?,vec![current]);
 assert!(documents::lexical_search(&pool,Uuid::now_v7(),"Cobalt",20,None).await?.is_empty());
 let conv=conversations::create(&pool,kb,user,"Price?").await?;
 conversations::append_message(&pool,conv,"user","What is the Cobalt price?",&conversations::TurnRecord::empty()).await?;
 let trace=conversations::append_message(&pool,conv,"assistant","Twelve [1]",&conversations::TurnRecord{
  metadata:json!({"model":"fixture"}),steps:json!([]),resolved:json!([]),tool_exchange:json!([]),
  sources:json!([{"n":1,"chunk_id":current,"document_id":doc,"text":"Cobalt price twelve"}])
 }).await?;
 assert_eq!(arcadia::trace(&pool,kb,user,trace).await?["answer"],"Twelve [1]");
 assert!(arcadia::trace(&pool,kb,other,trace).await.is_err());
 assert!(arcadia::traces(&pool,kb,other,0).await?.is_empty());
 assert_eq!(arcadia::impact(&pool,kb,user,doc).await?["answers"].as_array().unwrap().len(),1);
 assert!(arcadia::impact(&pool,kb,other,doc).await?["answers"].as_array().unwrap().is_empty());
 let first=arcadia::propose(&pool,kb,user,doc,"Update price","New agreement","Cobalt price fourteen").await?;
 let second=arcadia::propose(&pool,kb,user,doc,"Competing change","Other agreement","Cobalt price sixteen").await?;
 arcadia::decide(&pool,kb,user,first,true,"Reviewed",&format!("new-{doc}")).await?;
 assert_eq!(arcadia::change(&pool,kb,first).await?["status"],"approved");
 assert_eq!(arcadia::change(&pool,kb,first).await?["before"],"Cobalt price twelve");
 let audits:i64=sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE target_id=$1 AND action='arcadia.change_approved'").bind(first).fetch_one(&pool).await?;
 assert_eq!(audits,1);
 assert!(arcadia::decide(&pool,kb,user,first,true,"Retry","duplicate").await.is_err());
 assert!(arcadia::decide(&pool,kb,user,second,true,"Stale","stale").await.is_err());
 let jobs:i64=sqlx::query_scalar("SELECT count(*) FROM jobs WHERE payload->>'document_id'=$1 AND kind='process_document'").bind(doc.to_string()).fetch_one(&pool).await?;
 assert_eq!(jobs,1);
 let versions:i64=sqlx::query_scalar("SELECT count(*) FROM document_versions WHERE document_id=$1").bind(doc).fetch_one(&pool).await?;
 assert_eq!(versions,1);
 arcadia::decide(&pool,kb,user,second,false,"Rejected","unused").await?;
 assert_eq!(arcadia::change(&pool,kb,second).await?["status"],"rejected");
 // A replay may introduce a source absent from the original answer.
 let unrelated=conversations::append_message(&pool,conv,"assistant","No original sources",&conversations::TurnRecord::empty()).await?;
 sqlx::query("INSERT INTO arcadia_replays(id,trace_id,answer,evidence,metadata) VALUES($1,$2,'fourteen',$3,'{}')")
     .bind(Uuid::now_v7()).bind(unrelated).bind(json!([{"document_id":doc,"text":"Cobalt fourteen"}])).execute(&pool).await?;
 // Purging removes copied trace evidence as well as the source's own chunks.
 sqlx::query("UPDATE documents SET deleted_at=now(),purged_at=now() WHERE id=$1").bind(doc).execute(&pool).await?;
 assert_eq!(arcadia::trace(&pool,kb,user,trace).await?["metadata"]["redacted"],true);
 assert!(arcadia::trace(&pool,kb,user,trace).await?["evidence"].as_array().unwrap().is_empty());
 assert!(arcadia::replays(&pool,unrelated).await?.is_empty());
 assert_eq!(arcadia::change(&pool,kb,first).await?["before"],"[redacted]");
 assert!(conversations::append_message(&pool,conv,"assistant","Late answer",&conversations::TurnRecord{sources:json!([{"document_id":doc}]),..conversations::TurnRecord::empty()}).await.is_err());
 Ok(())
 }.await;
    if let Err(ref e) = result {
        eprintln!("Workflow failed before cleanup: {e:#}");
    }
    sqlx::query("DELETE FROM organizations WHERE id=$1")
        .bind(org)
        .execute(&pool)
        .await?;
    result
}
