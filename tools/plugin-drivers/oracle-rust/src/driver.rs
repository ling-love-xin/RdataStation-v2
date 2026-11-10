use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;
use oracle_rs::{Config, Connection};
use rds_driver_sidecar::{
    Driver, DriverError, ExecuteRequest, MetaObject, MetaObjectDetail, MetaSchema, Page, SessionInfo,
};
use serde_json::Value;

pub struct OracleDriver;

pub struct OracleSession {
    conn: Connection,
    in_transaction: bool,
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
        let conn = Connection::connect_with_config(config)
            .await
            .map_err(|e| DriverError::connect(format!("Oracle 连接失败：{e}")))?;
        let info = conn.server_info().await;
        let fallback = option_str(params, "username").unwrap_or_default().to_uppercase();
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
        let result = session
            .conn
            .execute(&req.sql, &bind_values)
            .await
            .map_err(|e| DriverError::sql(e.to_string()))?;

        let columns: Vec<_> = result.columns.iter().map(crate::types::column_def).collect();
        let total_rows = result.rows.len();
        let take = total_rows.min(req.max_rows);
        let rows: Vec<Vec<Value>> = result.rows[..take]
            .iter()
            .map(crate::types::convert_row)
            .collect();
        let has_more = result.has_more_rows || total_rows > req.max_rows;

        if columns.is_empty() {
            // Oracle 默认不自动提交：非事务态下的 DML/DDL 执行完立即提交。
            if !session.in_transaction {
                if let Err(err) = session.conn.commit().await {
                    return Err(DriverError::sql(format!("自动提交失败：{err}")));
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

    async fn schemas(
        &self,
        session: &mut Self::Session,
    ) -> Result<Vec<MetaSchema>, DriverError> {
        crate::meta::schemas(&session.conn).await
    }

    async fn objects(
        &self,
        session: &mut Self::Session,
        schema: &str,
    ) -> Result<Vec<MetaObject>, DriverError> {
        crate::meta::objects(&session.conn, schema).await
    }

    async fn object_detail(
        &self,
        session: &mut Self::Session,
        schema: &str,
        name: &str,
    ) -> Result<MetaObjectDetail, DriverError> {
        crate::meta::object_detail(&session.conn, schema, name).await
    }
}

fn build_config(params: &Value) -> Result<Config, DriverError> {
    let host = option_str(params, "host").unwrap_or_else(|| "localhost".to_string());
    let port = params
        .get("port")
        .and_then(Value::as_u64)
        .unwrap_or(1521) as u16;
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
