//! DuckDB **扩展能力**探针（联邦查询选型用）：报告式，不连任何源库。
//!
//! 跑法：
//! ```text
//! cargo test -p rds-engine -j 2 --test duckdb_extensions_probe -- --nocapture --test-threads=1
//! ```
//!
//! 它回答四个问题（联邦“直连 vs 桥接”的选型全押在这些答案上）：
//!
//! 1. 我们这套**动态链接**的 libduckdb 能不能装**官方扩展**（`mysql` / `postgres` / `sqlite` …）；
//! 2. 能不能装**社区扩展**（`mssql` / `oracle_scanner` / `adbc` / `adbc_scanner` …）——
//!    社区扩展由 DuckDB 官方签名托管，但要 `INSTALL … FROM community`；
//! 3. 扩展能不能装到**我们指定的目录**（应用自带、可离线预置、不污染用户家目录）；
//! 4. 装完之后 **LOAD 是否成功**（装得上 ≠ 加载得了：还有 ABI / 版本匹配）。
//!
//! **联网一次**（下载扩展二进制）。装到 `target/duckdb-extension-probe/`（探针专用目录，
//! 不碰 `~/.duckdb`）。报告式：只断言“不 panic”，结论靠打印看。

use duckdb::Connection;

/// 探针自己的扩展目录（不污染 `~/.duckdb`；工作区 `target/` 已在忽略规则里）
///
/// **不能用相对路径**：`cargo test` 的工作目录是**包根**（`crates/engine`），
/// `target/...` 会建到包目录里（实测 268 MB 留在 `crates/engine/target/`）。
/// 用 `CARGO_MANIFEST_DIR` 拼回工作区根的 `target/`。
fn probe_dir() -> std::path::PathBuf {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("target/duckdb-extension-probe")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(&dir).expect("建探针扩展目录");
    dir
}

fn setting(conn: &Connection, name: &str) -> String {
    conn.query_row(
        "SELECT value FROM duckdb_settings() WHERE name = ?",
        [name],
        |row| row.get::<_, String>(0),
    )
    .unwrap_or_else(|_| "<查不到>".to_string())
}

/// 装一个扩展（官方或社区），报告每一步的结果
fn probe(conn: &Connection, name: &str, from_community: bool) {
    let source = if from_community { "社区" } else { "官方" };
    let install = if from_community {
        format!("INSTALL {name} FROM community")
    } else {
        format!("INSTALL {name}")
    };
    match conn.execute_batch(&install) {
        Ok(()) => {
            match conn.execute_batch(&format!("LOAD {name}")) {
                Ok(()) => println!("✅ {name}（{source}）：INSTALL + LOAD 都成功"),
                Err(error) => println!("🟡 {name}（{source}）：装上了但 LOAD 失败 —— {error}"),
            }
        }
        Err(error) => {
            let text = error.to_string();
            let first_line = text.lines().next().unwrap_or("").to_string();
            println!("❌ {name}（{source}）：INSTALL 失败 —— {first_line}");
        }
    }
}

#[test]
fn probe_duckdb_extensions() {
    let dir = probe_dir();
    let conn = Connection::open_in_memory().expect("开内存库");

    println!("════════ DuckDB 扩展探针 ════════");
    println!("duckdb 版本：{}", setting(&conn, "version"));
    println!("默认扩展目录：{}", setting(&conn, "extension_directory"));
    println!(
        "允许社区扩展：{}",
        setting(&conn, "allow_community_extensions")
    );
    println!(
        "自动安装已知扩展：{} / 自动加载已知扩展：{}",
        setting(&conn, "autoinstall_known_extensions"),
        setting(&conn, "autoload_known_extensions")
    );

    // 把扩展目录钉到探针目录（联邦的落地方式：跟着应用走、可离线预置）
    let dir_sql = dir.display().to_string().replace('\\', "/");
    conn.execute_batch(&format!("SET extension_directory = '{dir_sql}'"))
        .expect("设置扩展目录");
    println!("已把本会话的扩展目录设为：{}", dir.display());
    println!(
        "设置后生效值：{}",
        setting(&conn, "extension_directory")
    );

    println!("\n──── 官方扩展（联邦直连的主力）────");
    for name in ["mysql", "postgres", "sqlite", "parquet", "json", "httpfs"] {
        probe(&conn, name, false);
    }

    println!("\n──── 社区扩展（Oracle / SQL Server / 通用 ADBC 桥）────");
    for name in ["mssql", "oracle_scanner", "adbc", "adbc_scanner", "firebird"] {
        probe(&conn, name, true);
    }

    println!("\n──── 装完之后，扩展目录里有什么 ────");
    let mut count = 0usize;
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            count += 1;
            println!("  {}", entry.path().display());
        }
    }
    println!("共 {count} 项（目录：{}）", dir.display());

    println!("\n──── duckdb_extensions() 里“已加载”的 ────");
    let mut stmt = conn
        .prepare("SELECT extension_name, loaded, installed FROM duckdb_extensions() WHERE loaded ORDER BY extension_name")
        .expect("查已加载扩展");
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, bool>(1)?,
                row.get::<_, bool>(2)?,
            ))
        })
        .expect("跑查询");
    for row in rows.flatten() {
        println!("  {}（loaded={} installed={}）", row.0, row.1, row.2);
    }
    println!("════════ 探针结束（结论看上面各行）════════");
}
