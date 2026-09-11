//! DuckDB Secret 本地加速通道（M3 核心差异化能力）
//!
//! 相对 DBeaver 的本地加速核心：将数据源凭据注册为 DuckDB Secret，
//! 使 DuckDB 分析引擎可直接联邦访问源库（Postgres/MySQL/SQLite 等），
//! 分析计算在本地 DuckDB 侧完成，避免应用层逐行转发——"一次注册、到处可用"。
//!
//! ```text
//! 应用层                  DuckDB 分析引擎                    源数据库
//! ConnectionService ──→  CREATE SECRET conn_001 ──────────→  Postgres/MySQL/...
//!                        (TYPE POSTGRES, HOST, USERNAME,     (DuckDB 直连联邦查询)
//!                         PASSWORD, DATABASE)
//! ```
//!
//! 说明：Secret 类型（POSTGRES/MYSQL/SQLITE 等）由 DuckDB 扩展提供；
//! 本模块负责凭据生命周期（注册/列出/删除），扩展加载与联邦查询属集成层。

use serde::{Deserialize, Serialize};

/// DuckDB Secret 支持的数据库类型（字符串直通 DuckDB 类型系统）
pub const SECRET_TYPE_POSTGRES: &str = "POSTGRES";
pub const SECRET_TYPE_MYSQL: &str = "MYSQL";
pub const SECRET_TYPE_SQLITE: &str = "SQLITE";
pub const SECRET_TYPE_DUCKDB: &str = "DUCKDB";
pub const SECRET_TYPE_S3: &str = "S3";

/// 数据库凭据（注册 DuckDB Secret 所需的最小凭据集）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseCredential {
    /// Secret 名称（一般取连接 ID，如 `conn_001`）
    pub name: String,
    /// 数据库类型（POSTGRES / MYSQL / SQLITE / DUCKDB / S3 ...）
    pub secret_type: String,
    /// 主机
    pub host: String,
    /// 端口
    pub port: u16,
    /// 用户名
    pub username: String,
    /// 密码
    pub password: String,
    /// 数据库名
    pub database: String,
}

/// Secret 摘要（来自 `duckdb_secrets()`）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretInfo {
    pub name: String,
    pub secret_type: String,
    pub storage: String,
}

/// Secret 管理错误
#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    #[error("DuckDB 连接失败: {0}")]
    DuckDb(#[from] duckdb::Error),
    #[error("DuckDB 语句执行失败: {0}")]
    Sql(String),
    #[error("Secret 不存在: {0}")]
    NotFound(String),
    #[error("参数无效: {0}")]
    Invalid(String),
}

/// 本地加速结果
pub type SecretResult<T> = Result<T, SecretError>;

/// DuckDB Secret 管理器：注册 / 列出 / 删除数据源凭据
///
/// Secret 以 `PERSISTENT` 方式注册，落盘于 Secret 存储目录：
/// 默认是 DuckDB 的 `~/.duckdb/stored_secrets`，可用 `open_with_dir` 指定
/// 应用可控目录（避免写用户主目录，并支持测试隔离）。
pub struct SecretManager {
    conn: duckdb::Connection,
}

impl SecretManager {
    /// 打开已存在的 DuckDB 数据库文件作为 Secret 存储（默认 Secret 目录）。
    pub fn open(db_path: impl AsRef<std::path::Path>) -> SecretResult<Self> {
        Self::open_with_dir(db_path, None)
    }

    /// 打开 DuckDB 数据库文件作为 Secret 存储，并指定 Secret 落盘目录。
    ///
    /// `secret_dir` 为 `None` 时用 DuckDB 默认目录；指定后每次打开都需传同一目录
    /// 才能看到已注册的 Secret（目录是会话级设置）。
    pub fn open_with_dir(
        db_path: impl AsRef<std::path::Path>,
        secret_dir: Option<&std::path::Path>,
    ) -> SecretResult<Self> {
        let conn = duckdb::Connection::open(db_path)?;
        if let Some(dir) = secret_dir {
            std::fs::create_dir_all(dir)
                .map_err(|e| SecretError::Invalid(format!("创建 Secret 目录失败: {e}")))?;
            conn.execute_batch(&format!(
                "SET secret_directory = '{}'",
                escape_path(dir)
            ))
            .map_err(|e| SecretError::Sql(e.to_string()))?;
        }
        Ok(Self { conn })
    }

    /// 内存 DuckDB（适合临时 Secret / 测试）
    pub fn in_memory() -> SecretResult<Self> {
        Ok(Self {
            conn: duckdb::Connection::open_in_memory()?,
        })
    }

    /// 注册凭据为 DuckDB Secret（持久化到当前数据库）
    ///
    /// 等价 SQL：
    /// ```sql
    /// CREATE OR REPLACE PERSISTENT SECRET <name> (
    ///   TYPE POSTGRES,
    ///   HOST 'h', PORT 5432, USERNAME 'u', PASSWORD 'p', DATABASE 'd'
    /// );
    /// ```
    ///
    /// 必须用 `PERSISTENT`：默认 `CREATE SECRET` 为会话级（storage = memory，
    /// 随连接关闭即丢失），无法支撑"一次注册、跨会话可用"的联邦加速。
    pub fn register(&self, cred: &DatabaseCredential) -> SecretResult<()> {
        let sql = format!(
            "CREATE OR REPLACE PERSISTENT SECRET {name} (TYPE {ty}, HOST '{host}', PORT {port}, \
             USERNAME '{user}', PASSWORD '{pass}', DATABASE '{db}')",
            name = cred.name,
            ty = cred.secret_type,
            host = escape_sql(cred.host.as_str()),
            port = cred.port,
            user = escape_sql(cred.username.as_str()),
            pass = escape_sql(cred.password.as_str()),
            db = escape_sql(cred.database.as_str()),
        );
        self.conn
            .execute_batch(&sql)
            .map_err(|e| SecretError::Sql(e.to_string()))?;
        Ok(())
    }

    /// 列出全部已注册 Secret（读 `duckdb_secrets()`）
    pub fn list(&self) -> SecretResult<Vec<SecretInfo>> {
        let mut stmt = self.conn.prepare(
            "SELECT name, type, storage FROM duckdb_secrets() ORDER BY name",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(SecretInfo {
                name: row.get(0)?,
                secret_type: row.get(1)?,
                storage: row.get(2)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// 按名称删除 Secret
    pub fn remove(&self, name: &str) -> SecretResult<()> {
        // 先确认存在，给出明确错误
        let exists = self
            .list()?
            .iter()
            .any(|s| s.name.eq_ignore_ascii_case(name));
        if !exists {
            return Err(SecretError::NotFound(name.to_string()));
        }
        self.conn
            .execute_batch(&format!("DROP SECRET IF EXISTS {name}"))
            .map_err(|e| SecretError::Sql(e.to_string()))?;
        Ok(())
    }
}

/// SQL 字符串转义（单引号加倍），防注入
fn escape_sql(s: &str) -> String {
    s.replace('\'', "''")
}

/// 路径转义：反斜杠转正斜杠（SQL 字面量中 Windows 路径安全），单引号加倍。
fn escape_path(p: &std::path::Path) -> String {
    p.to_string_lossy().replace('\\', "/").replace('\'', "''")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cred(name: &str, pass: &str) -> DatabaseCredential {
        DatabaseCredential {
            name: name.to_string(),
            secret_type: SECRET_TYPE_POSTGRES.to_string(),
            host: "localhost".to_string(),
            port: 5432,
            username: "admin".to_string(),
            password: pass.to_string(),
            database: "analytics".to_string(),
        }
    }

    /// 隔离的 Secret 存储：临时目录 + 专用 Secret 目录。
    ///
    /// 不触碰 DuckDB 默认存储（`~/.duckdb/stored_secrets`），避免测试污染用户环境。
    fn isolated(tag: &str) -> (SecretManager, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("rds_secret_{}_{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let mgr = SecretManager::open_with_dir(
            dir.join("store.duckdb"),
            Some(&dir.join("secrets")),
        )
        .expect("open isolated secret store");
        (mgr, dir)
    }

    #[test]
    fn test_register_list_remove_roundtrip() {
        let (mgr, dir) = isolated("roundtrip");

        // 注册（含特殊字符密码：单引号/分号应被转义而非注入）
        let c = cred("conn_001", "p@ss'word; DROP TABLE x; --");
        mgr.register(&c).unwrap();

        let list = mgr.list().unwrap();
        assert_eq!(list.len(), 1, "应注册 1 个 Secret");
        assert_eq!(list[0].name, "conn_001");
        assert_eq!(list[0].secret_type.to_uppercase(), "POSTGRES");
        assert_eq!(list[0].storage, "local_file", "应为持久存储：{list:?}");

        // 删除
        mgr.remove("conn_001").unwrap();
        assert!(mgr.list().unwrap().is_empty(), "删除后应为空");

        // 删除不存在的 → NotFound
        let err = mgr.remove("no_such").unwrap_err();
        assert!(matches!(err, SecretError::NotFound(_)));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_register_override_same_name() {
        let (mgr, dir) = isolated("override");
        let a = cred("dup", "first");
        let b = DatabaseCredential {
            database: "other".to_string(),
            ..cred("dup", "second")
        };
        mgr.register(&a).unwrap();
        // CREATE OR REPLACE：同名覆盖，不产生第二条
        mgr.register(&b).unwrap();
        assert_eq!(mgr.list().unwrap().len(), 1, "同名 Secret 应覆盖");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_persistent_secret_survives_reopen() {
        // 回归：默认 CREATE SECRET 为会话级（storage = memory），
        // 必须用 PERSISTENT 才能跨会话支撑联邦加速。
        let dir = std::env::temp_dir().join(format!("rds_secret_persist_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join("secrets.duckdb");
        let secret_dir = dir.join("store");

        {
            let mgr = SecretManager::open_with_dir(&path, Some(&secret_dir)).unwrap();
            mgr.register(&cred("conn_p", "pw")).unwrap();
            let list = mgr.list().unwrap();
            assert_eq!(list.len(), 1);
            assert_eq!(list[0].storage, "local_file", "应为持久存储：{list:?}");
        }

        {
            let mgr = SecretManager::open_with_dir(&path, Some(&secret_dir)).unwrap();
            let list = mgr.list().unwrap();
            assert_eq!(list.len(), 1, "PERSISTENT Secret 应跨会话保留：{list:?}");
            assert_eq!(list[0].name, "conn_p");
            mgr.remove("conn_p").unwrap();
        }

        {
            let mgr = SecretManager::open_with_dir(&path, Some(&secret_dir)).unwrap();
            assert!(mgr.list().unwrap().is_empty(), "删除应持久化");
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_unknown_secret_type_errors() {
        let (mgr, dir) = isolated("unknown_type");
        let c = DatabaseCredential {
            secret_type: "NO_SUCH_TYPE_XYZ".to_string(),
            ..cred("bad", "x")
        };
        let res = mgr.register(&c);
        assert!(res.is_err(), "未知 Secret 类型应报错");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
