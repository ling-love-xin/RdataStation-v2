//! SQL Server 驱动实现：连接、执行、事务与元数据。

use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;
use rds_driver_sidecar::{
    Canonical, Driver, DriverError, ExecuteRequest, MetaColumn, MetaIndex, MetaObject,
    MetaObjectDetail, MetaSchema, Page, SessionInfo,
};
use serde_json::Value;
use tiberius::{AuthMethod, Client, Config, EncryptionLevel, Query};
use tokio::net::TcpStream;
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt};

use crate::meta;
use crate::types;

pub type MssqlClient = Client<Compat<TcpStream>>;

pub struct MssqlDriver;

pub struct MssqlSession {
    pub client: MssqlClient,
    pub default_schema: String,
    pub in_transaction: bool,
}

#[async_trait]
impl Driver for MssqlDriver {
    type Session = MssqlSession;

    fn driver_id(&self) -> &'static str {
        "mssql"
    }

    fn display_name(&self) -> &'static str {
        "SQL Server"
    }

    /// SQL Server 的方括号标识符无法用一个闭合符号表达，统一走 ANSI 双引号。
    fn identifier_quote(&self) -> Option<&'static str> {
        Some("\"")
    }

    async fn open(&self, params: &Value) -> Result<(Self::Session, SessionInfo), DriverError> {
        let config = build_config(params)?;
        let addr = config.get_addr();
        let timeout = params
            .get("connect_timeout")
            .and_then(Value::as_u64)
            .filter(|v| *v > 0)
            .unwrap_or(15);

        let connect = async {
            let tcp = TcpStream::connect(&addr)
                .await
                .map_err(|err| DriverError::connect(format!("连接 {addr} 失败：{err}")))?;
            tcp.set_nodelay(true)
                .map_err(|err| DriverError::connect(format!("设置 TCP_NODELAY 失败：{err}")))?;
            Client::connect(config, tcp.compat_write())
                .await
                .map_err(|err| {
                    DriverError::connect(format!("SQL Server 登录失败：{err}{}", login_hint(params)))
                })
        };
        let client = tokio::time::timeout(Duration::from_secs(timeout), connect)
            .await
            .map_err(|_| DriverError::connect(format!("连接超时（{timeout} 秒）")))??;

        let mut session = MssqlSession {
            client,
            default_schema: "dbo".to_string(),
            in_transaction: false,
        };
        let (version, schema) = meta::server_info(&mut session.client).await;
        session.default_schema = schema.clone();

        let mut capabilities = BTreeMap::new();
        capabilities.insert("schemas".to_string(), true);
        capabilities.insert("transactions".to_string(), true);
        capabilities.insert("cancel".to_string(), false);
        capabilities.insert("cursor".to_string(), false);
        capabilities.insert("streaming".to_string(), false);

        Ok((
            session,
            SessionInfo {
                server_version: version,
                default_schema: Some(schema),
                capabilities,
            },
        ))
    }

    async fn close(&self, session: Self::Session) {
        let _ = session.client.close().await;
    }

    async fn ping(&self, session: &mut Self::Session) -> Result<(), DriverError> {
        let row = query_rows(&mut session.client, "SELECT 1", Vec::new())
            .await?
            .into_iter()
            .next();
        match row {
            Some(_) => Ok(()),
            None => Err(DriverError::protocol("ping 未返回结果")),
        }
    }

    async fn execute(
        &self,
        session: &mut Self::Session,
        req: ExecuteRequest,
    ) -> Result<Page, DriverError> {
        let mut query = Query::new(req.sql.clone());
        bind_params(&mut query, &req.params);

        if !returns_rows(&req.sql) {
            let result = query
                .execute(&mut session.client)
                .await
                .map_err(|err| DriverError::sql(err.to_string()))?;
            return Ok(Page::affected(result.total()));
        }

        let mut stream = query
            .query(&mut session.client)
            .await
            .map_err(|err| DriverError::sql(err.to_string()))?;
        let columns: Vec<tiberius::Column> = stream
            .columns()
            .await
            .map_err(|err| DriverError::sql(err.to_string()))?
            .unwrap_or_default()
            .to_vec();
        let results = stream
            .into_results()
            .await
            .map_err(|err| DriverError::sql(err.to_string()))?;

        let defs: Vec<_> = columns.iter().map(types::column_def).collect();
        let rows: Vec<Vec<Value>> = results
            .into_iter()
            .next()
            .unwrap_or_default()
            .iter()
            .map(types::convert_row)
            .collect();

        let max_rows = if req.max_rows == 0 { 1000 } else { req.max_rows };
        let total = rows.len();
        let mut page = Page::rows(defs, rows.into_iter().take(max_rows).collect());
        page.has_more = total > max_rows;
        Ok(page)
    }

    async fn begin(&self, session: &mut Self::Session) -> Result<(), DriverError> {
        if session.in_transaction {
            return Err(DriverError::sql("事务已开启"));
        }
        query_rows(&mut session.client, "BEGIN TRANSACTION", Vec::new()).await?;
        session.in_transaction = true;
        Ok(())
    }

    async fn commit(&self, session: &mut Self::Session) -> Result<(), DriverError> {
        if !session.in_transaction {
            return Err(DriverError::sql("当前没有开启事务"));
        }
        query_rows(&mut session.client, "COMMIT TRANSACTION", Vec::new()).await?;
        session.in_transaction = false;
        Ok(())
    }

    async fn rollback(&self, session: &mut Self::Session) -> Result<(), DriverError> {
        if !session.in_transaction {
            return Err(DriverError::sql("当前没有开启事务"));
        }
        query_rows(&mut session.client, "ROLLBACK TRANSACTION", Vec::new()).await?;
        session.in_transaction = false;
        Ok(())
    }

    async fn catalogs(&self, session: &mut Self::Session) -> Result<Vec<String>, DriverError> {
        meta::catalogs(&mut session.client).await
    }

    async fn schemas(&self, session: &mut Self::Session) -> Result<Vec<MetaSchema>, DriverError> {
        meta::schemas(&mut session.client).await
    }

    async fn objects(
        &self,
        session: &mut Self::Session,
        schema: &str,
    ) -> Result<Vec<MetaObject>, DriverError> {
        meta::objects(&mut session.client, schema).await
    }

    async fn object_detail(
        &self,
        session: &mut Self::Session,
        schema: &str,
        name: &str,
    ) -> Result<MetaObjectDetail, DriverError> {
        meta::object_detail(&mut session.client, schema, name).await
    }
}

pub fn build_config(params: &Value) -> Result<Config, DriverError> {
    let host = option_str(params, "host")
        .ok_or_else(|| DriverError::connect("缺少 host"))?;
    let port = params
        .get("port")
        .and_then(Value::as_u64)
        .and_then(|v| u16::try_from(v).ok())
        .unwrap_or(1433);
    let username = option_str(params, "username")
        .or_else(|| option_str(params, "user"))
        .unwrap_or("");
    let password = option_str(params, "password").unwrap_or("");
    let database = option_str(params, "database");

    let mut config = Config::new();
    config.host(host);
    config.port(port);
    config.application_name("RdataStation");
    if let Some(database) = database {
        config.database(database);
    }
    if username.is_empty() {
        #[cfg(windows)]
        config.authentication(AuthMethod::Integrated);
        #[cfg(not(windows))]
        return Err(DriverError::connect("缺少 username（本平台未启用 Windows 集成认证）"));
    } else {
        config.authentication(AuthMethod::sql_server(username, password));
    }

    let options = params.get("options");
    let encrypt = options
        .and_then(|o| o.get("encrypt"))
        .and_then(|v| match v {
            Value::Bool(b) => Some(*b),
            Value::String(s) => match s.to_ascii_lowercase().as_str() {
                "true" | "on" | "required" | "yes" => Some(true),
                "false" | "off" | "no" => Some(false),
                _ => None,
            },
            _ => None,
        })
        .unwrap_or(false);
    config.encryption(if encrypt {
        EncryptionLevel::Required
    } else {
        EncryptionLevel::Off
    });

    // 自签证书在本地实例上很常见；显式传 trust_cert=false 才走严格校验。
    let trust_cert = options
        .and_then(|o| o.get("trust_cert"))
        .and_then(Value::as_bool)
        .unwrap_or(true);
    if trust_cert {
        config.trust_cert();
    }

    if let Some(instance) = options
        .and_then(|o| o.get("instance_name"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        config.instance_name(instance);
    }

    Ok(config)
}

pub fn option_str<'a>(params: &'a Value, key: &str) -> Option<&'a str> {
    params.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
}

/// 登录失败时补一句「下一步怎么办」：真机上集成认证常报
/// `SEC_E_NO_CREDENTIALS`，只回协议错误用户看不出该填什么。
fn login_hint(params: &Value) -> &'static str {
    let integrated = option_str(params, "username")
        .or_else(|| option_str(params, "user"))
        .is_none();
    if integrated {
        "（当前按 Windows 集成认证连接，进程没有可用的域凭据；请填写 SQL 认证的用户名/密码）"
    } else {
        "（请检查用户名、密码，以及实例是否启用了 SQL 登录）"
    }
}

fn bind_params(query: &mut Query<'static>, params: &[Value]) {
    for param in params {
        match param {
            Value::Null => query.bind(Option::<&str>::None),
            Value::Bool(v) => query.bind(*v),
            Value::Number(v) => match types::bind_number(v) {
                types::BoundNumber::Int(n) => query.bind(n),
                types::BoundNumber::Float(f) => query.bind(f),
                types::BoundNumber::Text(s) => query.bind(s),
            },
            Value::String(v) => query.bind(v.clone()),
            other => query.bind(serde_json::to_string(other).unwrap_or_default()),
        }
    }
}

fn returns_rows(sql: &str) -> bool {
    let first = sql
        .trim_start_matches(|c: char| c.is_whitespace() || c == ';')
        .split(|c: char| c.is_whitespace() || c == '(' || c == ';')
        .next()
        .unwrap_or("")
        .to_ascii_uppercase();
    !matches!(
        first.as_str(),
        "INSERT"
            | "UPDATE"
            | "DELETE"
            | "MERGE"
            | "TRUNCATE"
            | "CREATE"
            | "ALTER"
            | "DROP"
            | "GRANT"
            | "REVOKE"
            | "DENY"
    )
}

/// 元数据查询统一入口：参数全部按字符串绑定，不拼 SQL。
pub async fn query_rows(
    client: &mut MssqlClient,
    sql: &str,
    params: Vec<String>,
) -> Result<Vec<tiberius::Row>, DriverError> {
    let mut query = Query::new(sql.to_string());
    for param in params {
        query.bind(param);
    }
    let results = query
        .query(client)
        .await
        .map_err(|err| DriverError::sql(err.to_string()))?
        .into_results()
        .await
        .map_err(|err| DriverError::sql(err.to_string()))?;
    Ok(results.into_iter().flatten().collect())
}

/// 驱动内共用的 canonical 推断（元数据查询里没有 ColumnType）。
pub fn canonical_for_type(type_name: &str) -> Canonical {
    match type_name.to_ascii_lowercase().as_str() {
        "bit" => Canonical::Boolean,
        "tinyint" | "smallint" | "int" | "bigint" => Canonical::Integer,
        "real" | "float" | "money" | "smallmoney" | "decimal" | "numeric" => Canonical::Float,
        "date" => Canonical::Date,
        "time" => Canonical::Time,
        "datetime" | "datetime2" | "smalldatetime" | "datetimeoffset" => Canonical::Timestamp,
        "binary" | "varbinary" | "image" | "timestamp" | "rowversion" => Canonical::Binary,
        "xml" => Canonical::Json,
        _ => Canonical::Text,
    }
}

#[allow(dead_code)]
pub fn empty_column(name: &str) -> MetaColumn {
    MetaColumn {
        name: name.to_string(),
        type_raw: "unknown".to_string(),
        canonical: Some(Canonical::Text.as_str().to_string()),
        nullable: Some(true),
        is_pk: None,
        is_fk: None,
        default: None,
        format: None,
        comment: None,
    }
}

#[allow(dead_code)]
pub fn empty_index(name: &str) -> MetaIndex {
    MetaIndex {
        name: name.to_string(),
        unique: None,
        columns: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rds_driver_sidecar::{FrameReader, FrameWriter, KIND_JSON, serve};
    use serde_json::json;
    use tokio::io::duplex;

    #[test]
    fn config_reads_host_port_and_database() {
        let params = json!({
            "host": "127.0.0.1",
            "port": 14330,
            "database": "master",
            "username": "sa",
            "password": "secret",
            "options": {"encrypt": true, "trust_cert": false}
        });
        let config = build_config(&params).unwrap();
        assert_eq!(config.get_addr(), "127.0.0.1:14330");
    }

    #[test]
    fn config_requires_credentials_on_unix_only() {
        if cfg!(windows) {
            // Windows 下缺用户名会走集成认证，配置仍然合法。
            assert!(build_config(&json!({"host": "127.0.0.1"})).is_ok());
        } else {
            assert!(build_config(&json!({"host": "127.0.0.1"})).is_err());
        }
    }

    #[test]
    fn statement_classification() {
        assert!(!returns_rows("INSERT INTO t VALUES (1)"));
        assert!(!returns_rows("  update t set a = 1"));
        assert!(returns_rows("SELECT 1"));
        assert!(returns_rows("with x as (select 1) select * from x"));
        assert!(returns_rows("EXEC sp_who"));
    }

    #[test]
    fn login_hint_points_at_sql_auth_or_credentials() {
        let integrated = login_hint(&json!({"host": "127.0.0.1"}));
        assert!(integrated.contains("集成认证"));
        assert!(integrated.contains("用户名/密码"));

        let sql_auth = login_hint(&json!({"username": "sa", "password": "x"}));
        assert!(sql_auth.contains("检查用户名、密码"));
        assert!(!sql_auth.contains("集成认证"));
    }

    #[tokio::test]
    async fn initialize_and_shutdown_without_database() {
        let (client_tx, server_rx) = duplex(16 * 1024);
        let (server_tx, client_rx) = duplex(16 * 1024);
        let server = tokio::spawn(async move { serve(MssqlDriver, server_rx, server_tx).await });

        let mut writer = FrameWriter::new(client_tx);
        let mut reader = FrameReader::new(client_rx);
        for (id, method, params) in [
            (1, "initialize", json!({"protocol": "rds-driver/1"})),
            (2, "ping", json!({})),
            (3, "shutdown", json!({})),
        ] {
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
