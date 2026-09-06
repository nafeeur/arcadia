//! Query engine for ask-your-data: a trait seam (same approach as BlobStore) +
//! an engine-agnostic safety gate.
//!
//! Engines extend by protocol family, not by product name: the postgres wire
//! protocol (`postgres.rs`), the MySQL wire protocol (`mysql.rs`, one protocol
//! that also covers TiDB / OceanBase / Doris / StarRocks / MariaDB), then the
//! HTTP family -- `trino.rs` alone covers the whole Iceberg / Delta / Hive
//! lakehouse ecosystem, `databricks.rs` and `snowflake.rs` each speak their own
//! SQL REST API. The mount model and the registry are engine-agnostic; adding
//! an engine only loosens one CHECK constraint. The connection string is the
//! only input: the engine is decided by scheme ([`engine_from_conn`]), the rest
//! is parsed per engine (`conn.rs`), and credentials only ever flow server-side.
//!
//! Safety gate (defense in depth, the model isn't trusted):
//! 1. sqlparser parses it: only a single SELECT/WITH (CTEs included) passes;
//!    DML/DDL/multi-statement/SELECT INTO are rejected. The dialect is chosen
//!    per engine; sqlparser has no Trino dialect, so Generic is used as its superset
//! 2. A LIMIT layer is forced on from outside (cap+1 to detect truncation)
//! 3. Session-level read-only + statement timeout (each engine's own mechanism,
//!    so even if something slips past the parser it can't be written).
//!    The HTTP family has no session, only a statement timeout -- read-only
//!    relies on layer 1 there, which is the layer they have less of than the
//!    wire-protocol engines
//! 4. Results are unified into JSON Lines: PG lets the database do the
//!    conversion itself; for the HTTP family, column names and values are
//!    assembled here, preserving column order

mod conn;
mod databricks;
mod mysql;
mod postgres;
mod snowflake;
mod trino;

use sqlparser::ast::Statement;
use sqlparser::dialect::{
    DatabricksDialect, GenericDialect, MySqlDialect, PostgreSqlDialect, SnowflakeDialect,
};
use sqlparser::parser::Parser;
use std::time::Duration;

/// Row cap (LIMIT cap+1 forced on from outside; row 201 only serves to detect truncation).
pub const ROW_CAP: usize = 200;
pub(crate) const STATEMENT_TIMEOUT_SECS: u32 = 10;
/// HTTP family: the timeout for a single request, and the poll budget for the
/// whole statement from submission to a finished result
pub(crate) const HTTP_REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
pub(crate) const HTTP_POLL_BUDGET: Duration = Duration::from_secs(30);

/// Values allowed in the registry's `engine` column. Must stay in sync with the CHECK in the migration
pub const ENGINES: &[&str] = &["postgres", "mysql", "trino", "databricks", "snowflake"];

#[derive(Debug)]
pub struct QueryResult {
    /// One JSON object per row, as text (key order = query's column order)
    pub rows: Vec<String>,
    pub truncated: bool,
}

#[derive(Debug)]
pub struct SchemaColumn {
    pub schema: String,
    pub table: String,
    pub column: String,
    pub data_type: String,
    pub comment: Option<String>,
}

#[async_trait::async_trait]
pub trait QueryEngine: Send + Sync {
    async fn test(&self) -> anyhow::Result<()>;
    async fn fetch_schema(&self) -> anyhow::Result<Vec<SchemaColumn>>;
    /// Execute a SELECT that already passed the gate. The implementation must
    /// still force a read-only session and a timeout itself (defense in depth).
    async fn execute(&self, sql: &str) -> anyhow::Result<QueryResult>;
}

/// scheme -> engine name. The UI has just one connection-string input box; this is its only dispatch point.
pub fn engine_from_conn(conn: &str) -> Option<&'static str> {
    let scheme = conn.trim().split("://").next()?.to_ascii_lowercase();
    match scheme.as_str() {
        "postgres" | "postgresql" => Some("postgres"),
        // One wire protocol also covers TiDB / OceanBase / Doris / StarRocks --
        // they all speak the MySQL protocol, so the connection string is written
        // as mysql:// as-is
        "mysql" | "mariadb" => Some("mysql"),
        "trino" | "presto" => Some("trino"),
        "databricks" => Some("databricks"),
        "snowflake" => Some("snowflake"),
        _ => None,
    }
}

/// Engine factory. Connection credentials only ever flow server-side.
pub fn engine_for(engine: &str, conn: &str) -> anyhow::Result<Box<dyn QueryEngine>> {
    match engine {
        "postgres" => Ok(Box::new(postgres::PostgresEngine::new(conn))),
        "mysql" => Ok(Box::new(mysql::MysqlEngine::new(conn))),
        "trino" => Ok(Box::new(trino::TrinoEngine::new(conn::TrinoConn::parse(
            conn,
        )?))),
        "databricks" => Ok(Box::new(databricks::DatabricksEngine::new(
            conn::DatabricksConn::parse(conn)?,
        ))),
        "snowflake" => Ok(Box::new(snowflake::SnowflakeEngine::new(
            conn::SnowflakeConn::parse(conn)?,
        ))),
        other => anyhow::bail!("Unsupported engine: {other}"),
    }
}

/// Safety gate layer 1: parse and validate per the engine's dialect, returning the normalized statement text.
pub fn guard_sql_for(engine: &str, sql: &str) -> anyhow::Result<String> {
    let cleaned = sql.trim().trim_end_matches(';').trim();
    if cleaned.is_empty() {
        anyhow::bail!("Empty SQL");
    }
    let parsed = match engine {
        "databricks" => Parser::parse_sql(&DatabricksDialect {}, cleaned),
        "snowflake" => Parser::parse_sql(&SnowflakeDialect {}, cleaned),
        "trino" => Parser::parse_sql(&GenericDialect {}, cleaned),
        "mysql" => Parser::parse_sql(&MySqlDialect {}, cleaned),
        _ => Parser::parse_sql(&PostgreSqlDialect {}, cleaned),
    };
    let statements = parsed.map_err(|e| anyhow::anyhow!("SQL parse error: {e}"))?;
    if statements.len() != 1 {
        anyhow::bail!("Exactly one statement is allowed");
    }
    match &statements[0] {
        Statement::Query(_) => Ok(cleaned.to_string()),
        other => anyhow::bail!(
            "Read-only: only SELECT/WITH queries are allowed (got {})",
            statement_kind(other)
        ),
    }
}

fn statement_kind(s: &Statement) -> &'static str {
    match s {
        Statement::Insert { .. } => "INSERT",
        Statement::Update { .. } => "UPDATE",
        Statement::Delete { .. } => "DELETE",
        Statement::CreateTable { .. } => "CREATE TABLE",
        Statement::Drop { .. } => "DROP",
        Statement::AlterTable { .. } => "ALTER TABLE",
        Statement::Truncate { .. } => "TRUNCATE",
        _ => "a non-SELECT statement",
    }
}

/// Layer 2: LIMIT forced on from outside. All three HTTP engines accept this syntax; PG has its own row_to_json version
pub(crate) fn wrap_limit(sql: &str) -> String {
    format!("SELECT * FROM ( {sql} ) AS _q LIMIT {}", ROW_CAP + 1)
}

/// Row 201 exists only to detect truncation; it's never handed to the model
pub(crate) fn truncate_rows<T>(mut rows: Vec<T>) -> (Vec<T>, bool) {
    let truncated = rows.len() > ROW_CAP;
    rows.truncate(ROW_CAP);
    (rows, truncated)
}

/// Shared across the HTTP family: assembles "column names + row values" into
/// JSON Lines. Built by hand rather than with `serde_json::Map`, since the
/// latter sorts by key unless `preserve_order` is enabled, and column order is
/// the order the query wrote them in -- which the model relies on to read the table
pub(crate) fn rows_to_json_lines(
    columns: &[String],
    rows: &[Vec<serde_json::Value>],
) -> Vec<String> {
    rows.iter()
        .map(|row| {
            let mut line = String::from("{");
            for (i, col) in columns.iter().enumerate() {
                if i > 0 {
                    line.push(',');
                }
                line.push_str(&serde_json::to_string(col).unwrap_or_else(|_| "\"?\"".into()));
                line.push(':');
                let value = row.get(i).cloned().unwrap_or(serde_json::Value::Null);
                line.push_str(&value.to_string());
            }
            line.push('}');
            line
        })
        .collect()
}

/// Databricks's JSON_ARRAY and Snowflake's data give every value back as a
/// string (or null). Numbers and booleans are restored per column type; the
/// rest stays a string -- the model's arithmetic on `"42"` vs `42` differs
pub(crate) fn coerce(type_name: &str, raw: &serde_json::Value) -> serde_json::Value {
    let serde_json::Value::String(s) = raw else {
        return raw.clone();
    };
    let ty = type_name.to_ascii_uppercase();
    const NUMERIC: &[&str] = &[
        "INT", "LONG", "SHORT", "BYTE", "FLOAT", "DOUBLE", "DECIMAL", "NUMBER", "FIXED", "REAL",
        "NUMERIC",
    ];
    // INTERVAL also contains "INT": if it doesn't parse as a number it's left as-is, so no harm done
    if NUMERIC.iter().any(|k| ty.contains(k)) {
        if let Ok(n) = s.parse::<i64>() {
            return n.into();
        }
        if let Ok(f) = s.parse::<f64>() {
            if let Some(n) = serde_json::Number::from_f64(f) {
                return serde_json::Value::Number(n);
            }
        }
    }
    if ty.starts_with("BOOL") {
        match s.as_str() {
            "true" | "TRUE" => return true.into(),
            "false" | "FALSE" => return false.into(),
            _ => {}
        }
    }
    raw.clone()
}

/// Escaping for a single-quoted literal: a schema name going into information_schema's WHERE clause
pub(crate) fn sql_literal(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Client shared across the HTTP family.
///
/// **The proxy policy is explicit**: loopback addresses and hosts in
/// `NO_PROXY` connect directly, everything else goes through `HTTPS_PROXY` /
/// `HTTP_PROXY` / `ALL_PROXY`. reqwest's system proxy detection isn't used --
/// on Windows it reads the registry, and the registry doesn't fully understand
/// bypass syntax like `127.*`, so a local stand-in service would get routed
/// through the proxy and come back with a 502. A service process should read
/// environment variables; this rule matches what docker-compose does
pub(crate) fn http() -> anyhow::Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(HTTP_REQUEST_TIMEOUT)
        .user_agent("utopia")
        .proxy(reqwest::Proxy::custom(|url: &reqwest::Url| proxy_for(url)))
        .build()?)
}

fn proxy_for(url: &reqwest::Url) -> Option<reqwest::Url> {
    let host = url.host_str()?;
    let loopback = host.eq_ignore_ascii_case("localhost")
        || host
            .trim_matches(|c| c == '[' || c == ']')
            .parse::<std::net::IpAddr>()
            .map(|ip| ip.is_loopback())
            .unwrap_or(false);
    if loopback || no_proxy_matches(host) {
        return None;
    }
    let keys: &[&str] = if url.scheme() == "https" {
        &["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"]
    } else {
        &["HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy"]
    };
    keys.iter()
        .find_map(|k| std::env::var(k).ok())
        .filter(|v| !v.trim().is_empty())
        .and_then(|v| reqwest::Url::parse(v.trim()).ok())
}

/// The common `NO_PROXY=localhost,127.0.0.1,.internal,corp.example` syntax:
/// an exact name match, or a dot-prefixed suffix match
fn no_proxy_matches(host: &str) -> bool {
    let raw = std::env::var("NO_PROXY")
        .or_else(|_| std::env::var("no_proxy"))
        .unwrap_or_default();
    raw.split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty() && *p != "*")
        .any(|p| {
            let p = p.trim_start_matches('.');
            host.eq_ignore_ascii_case(p)
                || host
                    .to_ascii_lowercase()
                    .ends_with(&format!(".{}", p.to_ascii_lowercase()))
        })
        || raw.split(',').any(|p| p.trim() == "*")
}

#[cfg(test)]
mod tests {
    use super::{coerce, engine_from_conn, guard_sql_for, rows_to_json_lines};
    use serde_json::json;

    fn guard_sql(sql: &str) -> anyhow::Result<String> {
        guard_sql_for("postgres", sql)
    }

    #[test]
    fn allows_select_and_cte() {
        assert!(guard_sql("SELECT region, sum(amount) FROM orders GROUP BY 1").is_ok());
        assert!(guard_sql("WITH t AS (SELECT 1 AS x) SELECT * FROM t;").is_ok());
    }

    #[test]
    fn rejects_writes_and_ddl() {
        for bad in [
            "UPDATE orders SET amount = 0",
            "DELETE FROM orders",
            "INSERT INTO orders (region) VALUES ('east')",
            "DROP TABLE orders",
            "TRUNCATE orders",
            "CREATE TABLE t (id int)",
            "ALTER TABLE orders ADD COLUMN x int",
        ] {
            assert!(guard_sql(bad).is_err(), "should reject: {bad}");
        }
    }

    #[test]
    fn rejects_multi_statement() {
        assert!(guard_sql("SELECT 1; DROP TABLE orders").is_err());
        assert!(guard_sql("").is_err());
    }

    #[test]
    fn every_dialect_keeps_the_same_gate() {
        for engine in ["postgres", "mysql", "trino", "databricks", "snowflake"] {
            assert!(
                guard_sql_for(engine, "SELECT a FROM t WHERE b > 1").is_ok(),
                "{engine}"
            );
            assert!(guard_sql_for(engine, "DELETE FROM t").is_err(), "{engine}");
            assert!(
                guard_sql_for(engine, "SELECT 1; SELECT 2").is_err(),
                "{engine}"
            );
        }
        // Each dialect's own quirks -- backticks, double-colon casts -- must all pass
        assert!(guard_sql_for("databricks", "SELECT `region` FROM main.sales.orders").is_ok());
        assert!(guard_sql_for("snowflake", "SELECT amount::number FROM db.public.orders").is_ok());
        assert!(guard_sql_for("trino", "SELECT count(*) FROM hive.default.orders").is_ok());
        // MySQL's backticks aren't compatible with the PG dialect; only its own dialect lets this pass
        assert!(guard_sql_for("mysql", "SELECT `region` FROM `sales`.`orders`").is_ok());
    }

    #[test]
    fn engine_follows_the_scheme() {
        assert_eq!(engine_from_conn("postgres://u:p@h/db"), Some("postgres"));
        assert_eq!(engine_from_conn("postgresql://u:p@h/db"), Some("postgres"));
        assert_eq!(engine_from_conn("trino://u@h:8443/hive"), Some("trino"));
        assert_eq!(engine_from_conn("presto://u@h/hive"), Some("trino"));
        assert_eq!(
            engine_from_conn("databricks://:t@h/sql/1.0/warehouses/x"),
            Some("databricks")
        );
        assert_eq!(
            engine_from_conn("snowflake://:t@a.snowflakecomputing.com/db"),
            Some("snowflake")
        );
        assert_eq!(engine_from_conn("mysql://u:p@h:3306/db"), Some("mysql"));
        // Another spelling of the same protocol; the engine rewrites it to mysql:// before handing it to the driver
        assert_eq!(engine_from_conn("mariadb://u@h/db"), Some("mysql"));
        assert_eq!(engine_from_conn("garbage"), None);
    }

    #[test]
    fn json_lines_keep_column_order() {
        let cols = vec!["zeta".to_string(), "alpha".to_string()];
        let rows = vec![vec![json!(1), json!("x")], vec![json!(null)]];
        assert_eq!(
            rows_to_json_lines(&cols, &rows),
            vec![r#"{"zeta":1,"alpha":"x"}"#, r#"{"zeta":null,"alpha":null}"#]
        );
    }

    #[test]
    fn strings_come_back_as_numbers_when_the_column_says_so() {
        assert_eq!(coerce("DOUBLE", &json!("12.5")), json!(12.5));
        assert_eq!(coerce("fixed", &json!("42")), json!(42));
        assert_eq!(coerce("BOOLEAN", &json!("true")), json!(true));
        assert_eq!(coerce("STRING", &json!("42")), json!("42"));
        assert_eq!(coerce("INTERVAL", &json!("1 day")), json!("1 day"));
        assert_eq!(coerce("DOUBLE", &json!(null)), json!(null));
    }
}
