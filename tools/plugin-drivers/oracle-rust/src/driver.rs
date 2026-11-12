use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;
use oracle_rs::{Config, Connection};
use rds_driver_sidecar::{
    Driver, DriverError, ExecuteRequest, MetaObject, MetaObjectDetail, MetaSchema, Page,
    SessionInfo,
};
use serde_json::Value;

pub struct OracleDriver;

pub struct OracleSession {
    conn: Connection,
    config: Config,
    in_transaction: bool,
}

impl OracleSession {
    /// Oracle 服务端在语句失败后可能直接断开，`oracle-rs` 也会把连接标成不可用。
    /// 这里在失败后立刻重连，保证下一条语句可用（错误本身仍按原消息返回）。
    async fn reconnect(&mut self) -> Result<(), DriverError> {
        let conn = Connection::connect_with_config(self.config.clone())
            .await
            .map_err(|e| DriverError::connect(format!("Oracle 重连失败：{e}")))?;
        self.conn = conn;
        self.in_transaction = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rds_driver_sidecar::{FrameReader, FrameWriter, KIND_JSON, serve};
    use serde_json::json;
    use tokio::io::duplex;

    #[test]
    fn config_requires_username() {
        assert!(build_config(&json!({"host": "127.0.0.1"})).is_err());
    }

    #[test]
    fn config_uses_service_name_option_over_database() {
        let config = build_config(&json!({
            "host": "127.0.0.1",
            "port": 1521,
            "username": "scott",
            "password": "tiger",
            "database": "ORCL",
            "options": {"service_name": "ORCLPDB1"}
        }))
        .unwrap();
        assert!(config.build_connect_string().contains("ORCLPDB1"));
    }

    #[test]
    fn config_sid_takes_precedence() {
        let config = build_config(&json!({
            "host": "127.0.0.1",
            "username": "scott",
            "password": "tiger",
            "database": "ORCLPDB1",
            "options": {"sid": "ORCL"}
        }))
        .unwrap();
        assert!(config.build_connect_string().contains("ORCL"));
    }

    #[test]
    fn pagination_helpers_cover_select_forms() {
        assert!(is_paginable_select("select level from dual"));
        assert!(is_paginable_select("  -- 注释\n/* 块注释 */ WITH t AS (SELECT 1) SELECT * FROM t"));
        assert!(!is_paginable_select("update t set a = 1"));
        assert!(!is_paginable_select("select * from t for update"));
        assert!(!is_paginable_select("select * from t fetch first 10 rows only"));
        assert!(!is_paginable_select("select * from t offset 10 rows"));
        assert_eq!(
            offset_page_sql("select * from t;  ", 100, 900),
            "select * from t\nOFFSET 100 ROWS FETCH NEXT 900 ROWS ONLY"
        );
    }

    #[tokio::test]
    async fn initialize_and_shutdown_without_database() {
        let (client_tx, server_rx) = duplex(16 * 1024);
        let (server_tx, client_rx) = duplex(16 * 1024);
        let server = tokio::spawn(async move { serve(OracleDriver, server_rx, server_tx).await });

        let mut writer = FrameWriter::new(client_tx);
        let mut reader = FrameReader::new(client_rx);
        for (id, method) in [(1, "initialize"), (2, "ping"), (3, "shutdown")] {
            let params = if method == "initialize" {
                json!({"protocol": "rds-driver/1"})
            } else {
                json!({})
            };
            let frame = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
            writer
                .write_frame(KIND_JSON, frame.to_string().as_bytes())
                .await
                .unwrap();
            let reply = reader.read_frame().await.unwrap().unwrap();
            let value: Value = serde_json::from_slice(&reply.payload).unwrap();
            assert_eq!(value["id"], id);
            assert!(value.get("error").is_none(), "{method} 应答：{value}");
        }

        drop(writer);
        drop(reader);
        server.await.unwrap().unwrap();
    }
}

/// 取 SQL 的首个关键字（跳过前导空白与 `--` / `/* */` 注释）。
fn leading_keyword(sql: &str) -> Option<String> {
    let mut rest = sql.trim_start();
    loop {
        rest = rest.trim_start();
        if let Some(tail) = rest.strip_prefix("--") {
            rest = tail.split_once('\n').map(|(_, after)| after).unwrap_or("");
        } else if let Some(tail) = rest.strip_prefix("/*") {
            rest = tail.split_once("*/").map(|(_, after)| after).unwrap_or("");
        } else {
            break;
        }
    }
    let word: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    (!word.is_empty()).then(|| word.to_ascii_uppercase())
}

/// 只有「SELECT / WITH、且没自带行限制子句、不是 FOR UPDATE」的语句才做分页重查。
fn is_paginable_select(sql: &str) -> bool {
    let Some(keyword) = leading_keyword(sql) else {
        return false;
    };
    if keyword != "SELECT" && keyword != "WITH" {
        return false;
    }
    let lower = sql.to_ascii_lowercase();
    !(lower.contains("offset ")
        || lower.contains("fetch first")
        || lower.contains("fetch next")
        || lower.contains("for update"))
}

/// 拼一页 `OFFSET n ROWS FETCH NEXT m ROWS ONLY`（Oracle 12c+）。
fn offset_page_sql(sql: &str, offset: usize, limit: usize) -> String {
    let base = sql.trim().trim_end_matches(';').trim_end();
    format!("{base}\nOFFSET {offset} ROWS FETCH NEXT {limit} ROWS ONLY")
}

#[async_trait]
impl Driver for OracleDriver {
    type Session = OracleSession;

    fn driver_id(&self) -> &'static str {
        "oracle"
    }

    fn display_name(&self) -> &'static str {
        "Oracle"
    }

    async fn open(&self, params: &Value) -> Result<(Self::Session, SessionInfo), DriverError> {
        let config = build_config(params)?;
        let conn = Connection::connect_with_config(config.clone())
            .await
            .map_err(|e| DriverError::connect(format!("Oracle 连接失败：{e}")))?;
        let info = conn.server_info().await;
        let fallback = option_str(params, "username")
            .unwrap_or_default()
            .to_uppercase();
        let default_schema = crate::meta::current_schema(&conn, &fallback).await;

        let mut capabilities = BTreeMap::new();
        capabilities.insert("schemas".to_string(), true);
        capabilities.insert("transactions".to_string(), true);
        capabilities.insert("cancel".to_string(), false);
        capabilities.insert("cursor".to_string(), false);
        capabilities.insert("streaming".to_string(), false);

        let server_version = if info.version.is_empty() {
            None
        } else {
            Some(info.version)
        };
        Ok((
            OracleSession {
                conn,
                config,
                in_transaction: false,
            },
            SessionInfo {
                server_version,
                default_schema: Some(default_schema),
                capabilities,
            },
        ))
    }

    async fn close(&self, session: Self::Session) {
        let _ = session.conn.close().await;
    }

    async fn ping(&self, session: &mut Self::Session) -> Result<(), DriverError> {
        session
            .conn
            .ping()
            .await
            .map_err(|e| DriverError::connect(format!("Oracle ping 失败：{e}")))
    }

    async fn execute(
        &self,
        session: &mut Self::Session,
        req: ExecuteRequest,
    ) -> Result<Page, DriverError> {
        let bind_values: Vec<oracle_rs::Value> =
            req.params.iter().map(crate::types::bind_value).collect();
        let mut result = match session.conn.execute(&req.sql, &bind_values).await {
            Ok(result) => result,
            Err(err) => {
                let message = err.to_string();
                if let Err(reconnect) = session.reconnect().await {
                    eprintln!("[rds-oracle-sidecar] 语句失败后重连失败：{reconnect}");
                }
                return Err(DriverError::sql(message));
            }
        };

        // `oracle-rs` 的 `execute` 首屏只取 100 行（`execute_query_with_params` 里
        // prefetch 写死 100），而它的续取入口 `fetch_more` 在实测中拿不到行：
        // 首页 `cursor_id=1`，`fetch_more` 却回 `rows=0 / cursor_id=0`。
        // 同时 rust-core 的 `query.fetch` 还没接（见 server.rs），宿主也拿不到第二页。
        // 因此这里在 `max_rows` 预算内用 Oracle 12c+ 的 `OFFSET/FETCH NEXT`
        // 逐页重查并拼装，否则超过 100 行的查询会在 UI 上被静默截断。
        //
        // 局限：只对 SELECT / WITH 且未自带 `OFFSET`、`FETCH FIRST/NEXT`、`FOR UPDATE`
        // 的语句生效；其它语句仍只返回首页（不谎报 has_more）。
        let max_rows = if req.max_rows == 0 {
            1000
        } else {
            req.max_rows as usize
        };
        let mut exhausted = !is_paginable_select(&req.sql);
        while !exhausted && result.rows.len() < max_rows {
            let offset = result.rows.len();
            let need = max_rows - offset;
            let page_sql = offset_page_sql(&req.sql, offset, need);
            match session.conn.execute(&page_sql, &bind_values).await {
                Ok(more) => {
                    let added = more.rows.len();
                    if result.columns.is_empty() && !more.columns.is_empty() {
                        result.columns = more.columns.clone();
                    }
                    result.rows.extend(more.rows);
                    exhausted = added == 0;
                }
                Err(err) => {
                    // 首页已经拿到了：翻页失败不该把整条查询判死，返回已有行并留痕。
                    eprintln!(
                        "[rds-oracle-sidecar] 翻页失败（已取 {} 行）：{err}",
                        result.rows.len()
                    );
                    break;
                }
            }
        }

        let columns: Vec<_> = result
            .columns
            .iter()
            .map(crate::types::column_def)
            .collect();
        let total_rows = result.rows.len();
        let take = total_rows.min(max_rows);
        let rows: Vec<Vec<Value>> = result.rows[..take]
            .iter()
            .map(crate::types::convert_row)
            .collect();
        let has_more = result.has_more_rows || (!exhausted && total_rows >= max_rows);

        if columns.is_empty() {
            // Oracle 默认不自动提交：非事务态下的 DML/DDL 执行完立即提交。
            if !session.in_transaction {
                if let Err(err) = session.conn.commit().await {
                    let message = format!("自动提交失败：{err}");
                    if let Err(reconnect) = session.reconnect().await {
                        eprintln!("[rds-oracle-sidecar] 自动提交失败后重连失败：{reconnect}");
                    }
                    return Err(DriverError::sql(message));
                }
            }
            return Ok(Page::affected(result.rows_affected));
        }

        Ok(Page {
            columns,
            rows,
            affected_rows: Some(result.rows_affected),
            has_more,
            cursor_id: None,
            notices: Vec::new(),
        })
    }

    async fn begin(&self, session: &mut Self::Session) -> Result<(), DriverError> {
        session.in_transaction = true;
        Ok(())
    }

    async fn commit(&self, session: &mut Self::Session) -> Result<(), DriverError> {
        session
            .conn
            .commit()
            .await
            .map_err(|e| DriverError::sql(format!("COMMIT 失败：{e}")))?;
        session.in_transaction = false;
        Ok(())
    }

    async fn rollback(&self, session: &mut Self::Session) -> Result<(), DriverError> {
        session
            .conn
            .rollback()
            .await
            .map_err(|e| DriverError::sql(format!("ROLLBACK 失败：{e}")))?;
        session.in_transaction = false;
        Ok(())
    }

    /// 元数据查询也要能自愈：真机上连着跑字典查询（点导航 / 打开属性面板）到
    /// 几十次之后，`oracle-rs` 那条连接会被服务端关掉，此后每个 `meta.*` 都是
    /// `connection closed unexpectedly`，直到宿主重启。`query.execute` 早就带重连，
    /// 这里补同一条策略：失败 → 重连 → 原样重试一次，仍失败才把错误交出去。
    async fn catalogs(&self, session: &mut Self::Session) -> Result<Vec<String>, DriverError> {
        match crate::meta::catalogs(&session.conn).await {
            Ok(value) => Ok(value),
            Err(first) => {
                if session.reconnect().await.is_err() {
                    return Err(first);
                }
                crate::meta::catalogs(&session.conn).await
            }
        }
    }

    async fn schemas(&self, session: &mut Self::Session) -> Result<Vec<MetaSchema>, DriverError> {
        match crate::meta::schemas(&session.conn).await {
            Ok(value) => Ok(value),
            Err(first) => {
                if session.reconnect().await.is_err() {
                    return Err(first);
                }
                crate::meta::schemas(&session.conn).await
            }
        }
    }

    async fn objects(
        &self,
        session: &mut Self::Session,
        schema: &str,
    ) -> Result<Vec<MetaObject>, DriverError> {
        match crate::meta::objects(&session.conn, schema).await {
            Ok(value) => Ok(value),
            Err(first) => {
                if session.reconnect().await.is_err() {
                    return Err(first);
                }
                crate::meta::objects(&session.conn, schema).await
            }
        }
    }

    async fn object_detail(
        &self,
        session: &mut Self::Session,
        schema: &str,
        name: &str,
    ) -> Result<MetaObjectDetail, DriverError> {
        match crate::meta::object_detail(&session.conn, schema, name).await {
            Ok(value) => Ok(value),
            Err(first) => {
                if session.reconnect().await.is_err() {
                    return Err(first);
                }
                crate::meta::object_detail(&session.conn, schema, name).await
            }
        }
    }
}

fn build_config(params: &Value) -> Result<Config, DriverError> {
    let host = option_str(params, "host").unwrap_or_else(|| "localhost".to_string());
    let port = params.get("port").and_then(Value::as_u64).unwrap_or(1521) as u16;
    let username = option_str(params, "username")
        .or_else(|| option_str(params, "user"))
        .ok_or_else(|| DriverError::connect("Oracle 连接参数缺少 username"))?;
    let password = option_str(params, "password").unwrap_or_default();
    let database = option_str(params, "database").unwrap_or_default();

    let sid = option_from_map(params, "sid");
    let service = option_from_map(params, "service_name")
        .or_else(|| option_from_map(params, "service"))
        .unwrap_or(database);

    let mut config = match sid {
        Some(sid) if !sid.is_empty() => Config::with_sid(host, port, sid, username, password),
        _ => Config::new(host, port, service, username, password),
    };

    if let Some(secs) = params.get("connect_timeout").and_then(Value::as_u64) {
        if secs > 0 {
            config.connect_timeout = Duration::from_secs(secs);
        }
    }
    if let Some(size) = option_from_map(params, "stmtcachesize").and_then(|v| v.parse().ok()) {
        config = config.with_statement_cache_size(size);
    }
    if option_from_map(params, "ssl")
        .or_else(|| option_from_map(params, "tls"))
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false)
    {
        config = config
            .with_tls()
            .map_err(|e| DriverError::connect(format!("Oracle TLS 配置失败：{e}")))?;
    }
    Ok(config)
}

fn option_from_map(params: &Value, key: &str) -> Option<String> {
    params
        .get("options")
        .and_then(Value::as_object)
        .and_then(|map| map.get(key))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn option_str(params: &Value, key: &str) -> Option<String> {
    params.get(key).and_then(Value::as_str).map(str::to_string)
}

#[allow(dead_code)]
fn _assert_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<OracleDriver>();
}
