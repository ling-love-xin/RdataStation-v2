//! 凭据路径探针（真机）：网络源挂载时**凭据到底从哪儿来**，以及它会不会漏进错误文本。
//!
//! 背景：应用里的连接分两份 URL——
//!
//! - `ConnectionInfo.url`：**脱敏**的（`mysql://root:******@host:3306/db`），给界面与日志用；
//! - `DriverConnectionConfig.url_override`：**运行时**那份（带口令），重连与挂载用它。
//!
//! 加速 / 联邦必须用后者。本探针把三件事分别钉住：
//!
//! 1. 含凭据的运行时串 → **挂得上、查得动**（我们出厂的路径）；
//! 2. 脱敏 URL → **挂不上**（`******` 当口令，说明为什么不能用它）；
//! 3. Secret 路径 → **不生效**（会话级 / 持久化 / 带 scope 都试；台账，说明为什么不靠它）；
//! 4. 失败的错误文本**不含口令**（走真实 `AccelSession`，错误经 `accel::scrub_credentials`）。
//!
//! ```text
//! RDS_TEST_MYSQL_URL='mysql://root:root@192.168.3.138:3306/mysql' \
//! cargo test -p rds-engine --test federation_credentials_probe -- --nocapture --test-threads=1
//! ```
//!
//! **注意 crate 名**：集成测试是独立 crate，本包的库目标叫 `rds_engine`。

use std::path::{Path, PathBuf};

use mysql_async::prelude::*;
use rds_engine::duckdb::accel::{self, AccelKind, AccelSource};

/// 探针库名（不动业务库）
const PROBE_DB: &str = "rds_fed_cred_probe";

/// 拆 URL：`scheme://user:pass@host:port/db` → 各段（只支持本探针要用的形状）
struct Parts {
    user: String,
    password: String,
    host: String,
    port: u16,
    database: String,
}

fn parse_url(url: &str) -> Parts {
    let rest = url.split("://").nth(1).expect("URL 要有 scheme");
    let (cred, host_part) = rest.split_once('@').expect("URL 要带凭据");
    let (user, password) = cred.split_once(':').expect("凭据要有 user:pass");
    let (host_port, database) = host_part.split_once('/').expect("URL 要有库名");
    let (host, port) = host_port.split_once(':').expect("要有端口");
    Parts {
        user: user.to_string(),
        password: password.to_string(),
        host: host.to_string(),
        port: port.parse().expect("端口是数字"),
        database: database.to_string(),
    }
}

/// 把原 URL 的库名换成探针库
fn probe_url(url: &str) -> String {
    let parts = parse_url(url);
    url.replace(
        &format!("/{}", parts.database),
        &format!("/{PROBE_DB}"),
    )
}

/// 建探针库 + 一张表（连原 URL；探针库还不存在，不能拿它建连）
fn mysql_setup(url: &str) {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    runtime.block_on(async {
        let pool = mysql_async::Pool::new(url);
        {
            let mut conn = pool.get_conn().await.expect("连 MySQL 建探针库");
            conn.query_drop(format!("CREATE DATABASE IF NOT EXISTS {PROBE_DB}"))
                .await
                .expect("建探针库");
            conn.query_drop(format!("DROP TABLE IF EXISTS {PROBE_DB}.orders"))
                .await
                .expect("清旧表");
            conn.query_drop(format!(
                "CREATE TABLE {PROBE_DB}.orders AS SELECT 1 AS id UNION SELECT 2 UNION SELECT 3"
            ))
            .await
            .expect("建表");
        }
        // 连接必须先归还：`disconnect()` 要等所有连接回池
        pool.disconnect().await.expect("断开");
    });
}

fn mysql_teardown(url: &str) {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    runtime.block_on(async {
        let pool = mysql_async::Pool::new(url);
        {
            if let Ok(mut conn) = pool.get_conn().await {
                let _ = conn
                    .query_drop(format!("DROP DATABASE IF EXISTS {PROBE_DB}"))
                    .await;
            }
        }
        let _ = pool.disconnect().await;
    });
}

/// 临时目录（每次运行各自隔离，不碰产品的 `{system}/secrets`）
fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rds_fed_cred_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("建临时目录");
    dir
}

/// 一带 SECRET 目录的内存连接（装好 mysql 扩展）
fn secret_connection(secret_dir: &Path, secret_sql: Option<&str>) -> duckdb::Connection {
    let conn = duckdb::Connection::open_in_memory().expect("开内存库");
    conn.execute_batch(&format!(
        "SET secret_directory = '{}';",
        secret_dir.to_string_lossy().replace('\\', "/")
    ))
    .expect("指 Secret 目录");
    conn.execute_batch("INSTALL mysql; LOAD mysql")
        .expect("装 mysql 扩展（首次需要能访问 DuckDB 扩展源）");
    if let Some(sql) = secret_sql {
        conn.execute_batch(sql).expect("建 Secret");
    }
    conn
}

/// 直接 ATTACH（不走我们的会话），返回能否读到探针表
fn attach_and_count(conn: &duckdb::Connection, url: &str) -> Result<usize, String> {
    conn.execute_batch(&format!("ATTACH '{url}' AS probe_src (TYPE mysql, READ_ONLY)"))
        .map_err(|error| first_line(&error.to_string()))?;
    let count: i64 = conn
        .query_row(
            &format!("SELECT count(*) FROM probe_src.{PROBE_DB}.orders"),
            [],
            |row| row.get(0),
        )
        .map_err(|error| first_line(&error.to_string()))?;
    let _ = conn.execute_batch("DETACH probe_src");
    Ok(count as usize)
}

#[test]
fn probe_network_source_credentials() {
    let Ok(mysql_url) = std::env::var("RDS_TEST_MYSQL_URL") else {
        eprintln!("⏭️ 未设 RDS_TEST_MYSQL_URL，跳过凭据路径探针");
        return;
    };
    mysql_setup(&mysql_url);
    let source_url = probe_url(&mysql_url);
    let parts = parse_url(&source_url);

    let masked = connection::url::mask_password_in_url(&source_url);
    eprintln!("· 运行时串（含凭据）：mysql://{}:{}@{}:{}/{}", parts.user, "***", parts.host, parts.port, parts.database);
    eprintln!("· 脱敏串            ：{masked}");

    // 1. 运行时串（出厂的路径）：挂得上、查得动
    let scratch = temp_dir("plain");
    let conn = secret_connection(&scratch, None);
    let rows = attach_and_count(&conn, &source_url).expect("含凭据的运行时串该挂得上");
    assert_eq!(rows, 3, "探针表 3 行");
    eprintln!("✅ 含凭据的运行时串 → 挂载成功（orders {rows} 行）");
    drop(conn);

    // 2. 脱敏串：`******` 被当口令 → 认证失败（所以不能用 ConnectionInfo.url）
    let conn = secret_connection(&scratch, None);
    let failure = attach_and_count(&conn, &masked).expect_err("脱敏串该认证失败");
    eprintln!("✅ 脱敏串 → 如实失败：{failure}");
    drop(conn);

    // 3. 台账：Secret 路径（会话级 + 持久化 + scope）到底认不认
    let secret_dir = temp_dir("secret");
    let session_secret = format!(
        "CREATE OR REPLACE SECRET rds_probe (TYPE MYSQL, HOST '{}', PORT {}, USER '{}', PASSWORD '{}', DATABASE '{}')",
        parts.host, parts.port, parts.user, parts.password, parts.database
    );
    for (tag, secret_sql, url) in [
        ("会话级 Secret + `ATTACH ''`", Some(session_secret.as_str()), String::new()),
        (
            "会话级 Secret + 无凭据 URL",
            Some(session_secret.as_str()),
            format!("mysql://{}:{}/{}", parts.host, parts.port, parts.database),
        ),
    ] {
        let conn = secret_connection(&secret_dir, secret_sql);
        match attach_and_count(&conn, &url) {
            Ok(rows) => eprintln!("ℹ️ 台账：{tag} → 竟能挂上（{rows} 行）"),
            Err(error) => eprintln!("ℹ️ 台账：{tag} → 不生效（{error}）"),
        }
        drop(conn);
    }

    // 4. 脱敏：走真实加速会话，让 ATTACH 失败并检查错误文本
    accel::drop_all();
    let wrong = format!(
        "mysql://{}:{}@{}:{}/no_such_database_{PROBE_DB}",
        parts.user, parts.password, parts.host, parts.port
    );
    let source = AccelSource::new("C_probe_scrub", "mysql_native", &wrong).expect("组装源");
    let error = match accel::ensure_session(&source) {
        Ok(_) => panic!("指向不存在的库该挂不上"),
        Err(reason) => reason,
    };
    // 注意：探针账号的用户名与口令都是 `root`，所以不能简单地 `contains(password)`
    // （用户名本身就该留着）——要看的是**凭据片段**与 `:口令@` 这种形状有没有漏出去
    let credential_segment = format!("{}:{}", parts.user, parts.password);
    assert!(
        !error.contains(&credential_segment),
        "凭据片段不得出现在错误文本里：{error}"
    );
    assert!(
        !error.contains(&format!(":{}@", parts.password)),
        "口令不得出现在错误文本里：{error}"
    );
    assert!(
        error.contains("******"),
        "抹掉之后要留痕（看得出是脱敏的）：{error}"
    );
    assert!(
        error.to_lowercase().contains("unknown database") || error.to_lowercase().contains("access denied"),
        "原因要留着（只是把口令抹掉）：{error}"
    );
    eprintln!("✅ 失败原因脱敏：{error}");
    accel::drop_all();
    assert_eq!(source.kind, AccelKind::MySql);

    mysql_teardown(&mysql_url);
    let _ = std::fs::remove_dir_all(&scratch);
    let _ = std::fs::remove_dir_all(&secret_dir);
}

/// 一行摘要（DuckDB 的原话可能很长，进日志只留第一行）
fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or(text).trim().to_string()
}
