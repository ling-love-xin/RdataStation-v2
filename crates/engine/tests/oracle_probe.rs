//! Oracle 真机探针（L2 社区 scanner）：`oracle_scanner` 的用法形态与边界
//!
//! 这里问的是「能不能把 Oracle 作为联邦源参与跨源查询」，结论直接决定
//! `federation/session.rs` 要不要为它写一条**特殊挂载路径**（见
//! `docs/architecture/federation/federation-architecture.md` §8 #2）。
//!
//! ```text
//! RDS_TEST_ORACLE_URL='oracle://devuser:Dev2026123@192.168.3.138:1521/XEPDB1' \
//! RDS_TEST_SQLITE_PATH='D:\FossilT\T.fossil' \
//! cargo test -p rds-engine -j 2 --test oracle_probe -- --nocapture --test-threads=1
//! ```
//!
//! **给源库留痕**：探针会建一张自己的表 `RDS_PROBE_ORDERS` 并在**断言之前**删掉它
//! （断言失败也不留垃圾）——**不碰**用户已有的对象。
//!
//! ## 实测结论（2026-09-18，oracle_scanner 0.2.2 / DuckDB 1.5.5）
//!
//! | 问题 | 结论 |
//! | --- | --- |
//! | 扩展能不能装 | ✅ `INSTALL oracle_scanner FROM community` + `LOAD` 成功 |
//! | 凭据怎么给 | **只能走 Secret**：`oracle_query('<secret>', …)` 的第一参数是 **secret 名**，不是连接串；`CREATE SECRET (TYPE ORACLE, HOST/PORT/USER/PASSWORD/SERVICE_NAME)`；会话级（不落盘）即可 |
//! | 服务名 | 这台实例是 **XEPDB1**（XE 报 `ORA-01017`，其它名字报监听器 redirect） |
//! | 怎么查数据 | 两条路都通：**ATTACH 目录** `ATTACH '<secret>' AS ora (TYPE oracle_scanner)` → 用 **两段名** `ora.<表>`（表挂在 `main` schema 下；三段名 `ora.<OWNER>.<表>` **不行**，会报 schema 不存在）；**表函数** `SELECT * FROM oracle_query('<secret>', '<Oracle SQL>')` |
//! | 目录里能看到什么 | 表与列（`duckdb_tables()` / `duckdb_columns()`）；表清单**在 ATTACH 时定型**（新表要重挂，与加速档同语义）；且只列**当前账号自己的**表（空 schema 时目录看着“什么都没有”是正常的） |
//! | 只读 | ❌ **不支持** `READ_ONLY`（扩展原话：*Oracle ATTACH does not accept option 'read_only' yet*）→ 引擎侧写保护对它**不成立**，只有编辑器闸门 + 只读账号两道 |
//! | 写 / DDL | `oracle_execute` **只接受 INSERT / UPDATE / DELETE**；DDL 要走 `oracle_call_auto(secret, 'DBMS_UTILITY.EXEC_DDL_STATEMENT', ['…'])`（探针靠它建/删自己的表） |
//! | 并行扫描 | `oracle_scan_parallel('<secret>', '<表名>', '<分片键列>', shards := N)`；**需要额外权限**（`SYS.DBMS_FLASHBACK` 的 EXECUTE + 表上的 FLASHBACK），普通账号跑不了 → 默认路径用 `oracle_query` |
//! | 过滤下推 | ⚠️ **过滤不推**：两条路的 `EXPLAIN` 里 `FILTER` 都在扫描节点（`ORACLE_QUERY` / `ORACLE_ATTACHED_SCAN`）**之上**——列投影会推（`Projections:`），谓词不会；要快得把谓词写进 Oracle SQL（表函数路径） |
//! | 类型映射 | ⚠️ 无精度约束的 `NUMBER` → **VARCHAR**；`NUMBER(10,2)` → `DECIMAL(10,2)`；跨源比较要留意 |

use std::path::PathBuf;

/// 探针自己的表（跑完删；不碰用户对象）
const PROBE_TABLE: &str = "RDS_PROBE_ORDERS";

/// 拆 `oracle://user:pass@host:port/service`（本探针只需要这形状）
struct Parts {
    user: String,
    password: String,
    host: String,
    port: u16,
    service: String,
}

fn parse_url(url: &str) -> Parts {
    let rest = url.split("://").nth(1).expect("URL 要有 scheme");
    let (cred, host_part) = rest.split_once('@').expect("URL 要带凭据");
    let (user, password) = cred.split_once(':').expect("凭据要有 user:pass");
    let (host_port, service) = host_part.split_once('/').expect("URL 要有 service name");
    let (host, port) = host_port.split_once(':').expect("要有端口");
    Parts {
        user: user.to_string(),
        password: password.to_string(),
        host: host.to_string(),
        port: port.parse().expect("端口是数字"),
        service: service.to_string(),
    }
}

fn first(text: &str) -> String {
    text.lines().next().unwrap_or(text).trim().to_string()
}

/// 装好扩展、建好**会话级** Secret 的内存库（凭据不落盘）
fn oracle_session(parts: &Parts) -> Result<duckdb::Connection, String> {
    let conn = duckdb::Connection::open_in_memory().map_err(|e| e.to_string())?;
    // 扩展目录隔离（与其它探针同款；不碰产品目录，也不碰 ~/.duckdb）
    let ext = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/duckdb-ext-scratch");
    std::fs::create_dir_all(&ext).map_err(|e| e.to_string())?;
    conn.execute_batch(&format!(
        "SET extension_directory = '{}';
         SET autoinstall_known_extensions = false;
         SET autoload_known_extensions = true;",
        ext.to_string_lossy().replace('\\', "/")
    ))
    .map_err(|e| e.to_string())?;
    conn.execute_batch("INSTALL oracle_scanner FROM community; LOAD oracle_scanner")
        .map_err(|e| first(&e.to_string()))?;
    conn.execute_batch(&format!(
        "CREATE OR REPLACE SECRET rds_ora (TYPE ORACLE, HOST '{}', PORT {}, \
         USER '{}', PASSWORD '{}', SERVICE_NAME '{}')",
        parts.host, parts.port, parts.user, parts.password, parts.service
    ))
    .map_err(|e| first(&e.to_string()))?;
    Ok(conn)
}

/// 跑一句把它整行读出来（`Value` 原样，便于看类型）
fn rows_of(conn: &duckdb::Connection, sql: &str) -> Result<Vec<Vec<duckdb::types::Value>>, String> {
    let mut stmt = conn.prepare(sql).map_err(|e| first(&e.to_string()))?;
    let mut rows = stmt.query([]).map_err(|e| first(&e.to_string()))?;
    let mut out: Vec<Vec<duckdb::types::Value>> = Vec::new();
    while let Some(row) = rows.next().map_err(|e| first(&e.to_string()))? {
        let mut values = Vec::new();
        for index in 0.. {
            match row.get::<usize, duckdb::types::Value>(index) {
                Ok(value) => values.push(value),
                Err(_) => break,
            }
        }
        out.push(values);
    }
    Ok(out)
}

/// 单值（先 `CAST(... AS VARCHAR)` 比较稳妥：Oracle 侧一律回 VARCHAR）
///
/// `duckdb::types::Value` 没有 `Display`，这里只把常见几种还原成人读的形式。
fn value_text(value: &duckdb::types::Value) -> String {
    match value {
        duckdb::types::Value::Null => "NULL".to_string(),
        duckdb::types::Value::Text(text) => text.clone(),
        duckdb::types::Value::BigInt(n) => n.to_string(),
        duckdb::types::Value::Int(n) => n.to_string(),
        duckdb::types::Value::Double(n) => n.to_string(),
        duckdb::types::Value::Boolean(b) => b.to_string(),
        other => format!("{other:?}"),
    }
}

fn scalar(conn: &duckdb::Connection, sql: &str) -> Result<String, String> {
    rows_of(conn, sql)?
        .first()
        .and_then(|row| row.first())
        .map(value_text)
        .ok_or_else(|| "没有返回行".to_string())
}

/// `EXPLAIN` 的整段计划文本（它回两列：`explain_key` / `explain_value`，这里全拼上）
fn explain_text(conn: &duckdb::Connection, sql: &str) -> Result<String, String> {
    Ok(rows_of(conn, sql)?
        .iter()
        .map(|row| {
            row.iter()
                .map(value_text)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join(" "))
}

/// DDL 只能这样跑：`DBMS_UTILITY.EXEC_DDL_STATEMENT`
fn exec_ddl(conn: &duckdb::Connection, ddl: &str) -> Result<(), String> {
    conn.execute_batch(&format!(
        "SELECT * FROM oracle_call_auto('rds_ora', 'DBMS_UTILITY.EXEC_DDL_STATEMENT', ['{ddl}'])"
    ))
    .map_err(|e| first(&e.to_string()))
}

#[test]
fn probe_oracle_as_a_federation_source() {
    let Ok(url) = std::env::var("RDS_TEST_ORACLE_URL") else {
        eprintln!("⏭️ 未设 RDS_TEST_ORACLE_URL，跳过 Oracle 探针");
        return;
    };
    let parts = parse_url(&url);
    let conn = oracle_session(&parts).expect("装扩展 + 建 Secret");

    // 台账：ATTACH 只读选项被拒（写保护对 L2 不成立）
    let read_only_refused = conn
        .execute_batch("ATTACH 'rds_ora' AS ora_ro (TYPE oracle_scanner, READ_ONLY)")
        .err()
        .map(|e| first(&e.to_string()))
        .unwrap_or_default();

    // 建探针表 → 插三行（每一步都先收结果，最后统一断言）
    let mut steps: Vec<(&str, Result<String, String>)> = Vec::new();
    steps.push((
        "建表",
        exec_ddl(
            &conn,
            &format!(
                "CREATE TABLE {PROBE_TABLE} (id NUMBER, name VARCHAR2(50), amount NUMBER(10,2))"
            ),
        )
        .map(|_| "ok".to_string()),
    ));
    for (id, name, amount) in [(1, "alpha", "10.5"), (2, "beta", "20.25"), (3, "gamma", "30.75")] {
        steps.push((
            "插行",
            conn.execute_batch(&format!(
                "SELECT * FROM oracle_execute('rds_ora', 'INSERT INTO {PROBE_TABLE} \
                 VALUES ({id}, ''{name}'', {amount})')"
            ))
            .map(|_| "ok".to_string())
            .map_err(|e| first(&e.to_string())),
        ));
    }

    // 表函数读三行
    steps.push((
        "oracle_query 三行",
        scalar(
            &conn,
            &format!(
                "SELECT count(*) FROM oracle_query('rds_ora', \
                 'SELECT id, name, amount FROM {PROBE_TABLE}')"
            ),
        ),
    ));
    // 类型映射：`id` 是不是回成 VARCHAR
    steps.push((
        "id 的 DuckDB 类型",
        scalar(
            &conn,
            &format!(
                "SELECT typeof(id) FROM oracle_query('rds_ora', 'SELECT id FROM {PROBE_TABLE}') LIMIT 1"
            ),
        ),
    ));
    // 并行扫描（第三参数是表名、第四是分片键列）：**需要额外权限**，普通用户跑不了
    let scan_parallel = scalar(
        &conn,
        &format!(
            "SELECT count(*) FROM oracle_scan_parallel('rds_ora', '{PROBE_TABLE}', 'ID', shards := 2)"
        ),
    );
    // 谓词写进 Oracle SQL 里才真的下推
    steps.push((
        "谓词写进 Oracle SQL",
        scalar(
            &conn,
            &format!(
                "SELECT count(*) FROM oracle_query('rds_ora', \
                 'SELECT id FROM {PROBE_TABLE} WHERE id >= 2')"
            ),
        ),
    ));
    // 下推观察：表函数路径的 EXPLAIN 里 FILTER 在 ORACLE_QUERY 之上
    steps.push((
        "EXPLAIN 表函数计划",
        explain_text(
            &conn,
            &format!(
                "EXPLAIN SELECT * FROM oracle_query('rds_ora', 'SELECT id FROM {PROBE_TABLE}') WHERE id = '2'"
            ),
        ),
    ));

    // ATTACH 目录形态：建得起来，能看到什么？（探针表此刻存在，正好用来看目录）
    let catalog = (|| -> Result<String, String> {
        conn.execute_batch("ATTACH 'rds_ora' AS ora (TYPE oracle_scanner)")
            .map_err(|e| first(&e.to_string()))?;
        let schemas = scalar(
            &conn,
            "SELECT coalesce(string_agg(schema_name, ','), '（空）') FROM duckdb_schemas() \
             WHERE database_name = 'ora'",
        )?;
        let tables = scalar(
            &conn,
            "SELECT coalesce(string_agg(schema_name || '.' || table_name, ','), '（空）') \
             FROM duckdb_tables() WHERE database_name = 'ora'",
        )?;
        let columns = scalar(
            &conn,
            "SELECT coalesce(string_agg(column_name || ':' || data_type, ','), '（空）') \
             FROM duckdb_columns() WHERE database_name = 'ora'",
        )?;
        // 从目录里真读一次（限定名 / 不带 schema 两种写法都试）
        let qualified = scalar(&conn, &format!("SELECT count(*) FROM ora.DEVUSER.{PROBE_TABLE}"));
        let two_part = scalar(&conn, &format!("SELECT count(*) FROM ora.{PROBE_TABLE}"));
        // 目录上带过滤（看会不会下推到 Oracle）
        let plan = explain_text(&conn, &format!("EXPLAIN SELECT * FROM ora.{PROBE_TABLE} WHERE ID = '2'"));
        let _ = conn.execute_batch("DETACH ora");
        Ok(format!(
            "schema=[{schemas}] 表=[{tables}] 列=[{columns}]\n    \
             三段名读={qualified:?}\n    两段名读={two_part:?}\n    计划={plan:?}"
        ))
    })();

    // 跨源：Oracle（**目录路径**）× SQLite（附加 catalog）——一条 SQL 同时读两边
    let cross_source = std::env::var("RDS_TEST_SQLITE_PATH")
        .ok()
        .map(|path| -> Result<String, String> {
            conn.execute_batch(&format!(
                "ATTACH '{}' AS sqlite_src (TYPE sqlite, READ_ONLY)",
                path.replace('\\', "/")
            ))
            .map_err(|e| first(&e.to_string()))?;
            // ① 表函数路径（不依赖目录）
            let via_function = scalar(
                &conn,
                &format!(
                    "SELECT count(*) FROM oracle_query('rds_ora', 'SELECT id FROM {PROBE_TABLE}') o \
                     JOIN sqlite_src.main.blob b ON 1 = 1"
                ),
            )?;
            // ② 目录路径（先 ATTACH 再两段名）
            conn.execute_batch("ATTACH 'rds_ora' AS ora (TYPE oracle_scanner)")
                .map_err(|e| first(&e.to_string()))?;
            let via_catalog = scalar(
                &conn,
                &format!(
                    "SELECT count(*) FROM ora.{PROBE_TABLE} o \
                     JOIN sqlite_src.main.blob b ON 1 = 1"
                ),
            )?;
            let _ = conn.execute_batch("DETACH ora");
            Ok(format!("表函数={via_function} 行 · 目录={via_catalog} 行"))
        });

    // ===== 先把源库恢复原样，再断言（断言失败也不留垃圾表）=====
    let dropped = exec_ddl(&conn, &format!("DROP TABLE {PROBE_TABLE}"));
    let leftover = scalar(
        &conn,
        &format!(
            "SELECT count(*) FROM oracle_query('rds_ora', \
             'SELECT table_name FROM user_tables WHERE table_name = ''{PROBE_TABLE}''')"
        ),
    );

    // ===== 断言 =====
    let by_tag = |tag: &str| -> String {
        steps
            .iter()
            .find(|(name, _)| *name == tag)
            .map(|(_, outcome)| match outcome {
                Ok(text) => text.clone(),
                Err(reason) => panic!("{tag} 失败：{reason}"),
            })
            .unwrap_or_default()
    };

    assert_eq!(by_tag("oracle_query 三行"), "3", "表函数要能读到三行");
    let id_type = by_tag("id 的 DuckDB 类型").to_uppercase();
    assert!(
        id_type.contains("VARCHAR") || id_type.contains("TEXT"),
        "Oracle 的 NUMBER 回来是文本（台账：类型映射一律 VARCHAR）：{id_type}"
    );
    match &scan_parallel {
        Ok(count) => {
            assert_eq!(count, "3", "并行扫描要能扫到三行");
            eprintln!("✅ 台账：oracle_scan_parallel 可用（该账号有 DBMS_FLASHBACK + FLASHBACK 权限）");
        }
        Err(reason) => {
            // 这台实例的 devuser 没有那两个权限：如实记下，不当失败
            assert!(
                reason.contains("DBMS_FLASHBACK") || reason.contains("FLASHBACK"),
                "并行扫描失败该是权限问题（否则要重看台账）：{reason}"
            );
            eprintln!(
                "ℹ️ 台账：oracle_scan_parallel 不可用（缺 SYS.DBMS_FLASHBACK 的 EXECUTE 与表上的 FLASHBACK 权限）——默认路径用 oracle_query"
            );
        }
    }
    assert_eq!(by_tag("谓词写进 Oracle SQL"), "2", "≥2 的行有两行");
    let plan = by_tag("EXPLAIN 表函数计划");
    assert!(plan.contains("ORACLE_QUERY"), "计划里该有 ORACLE_QUERY：{plan}");
    assert!(
        plan.contains("FILTER"),
        "表函数路径的过滤留在 DuckDB 侧（未下推）——这就是“要快得自己把谓词写进 Oracle SQL”的依据：{plan}"
    );

    assert!(
        read_only_refused.contains("read_only"),
        "ATTACH 只读选项该被扩展拒绝（写保护靠别的两道闸）：{read_only_refused}"
    );
    let catalog = catalog.expect("ATTACH 目录形态该能建起来");
    eprintln!("ℹ️ 台账 · ATTACH 目录形态：\n    {catalog}");
    assert!(
        catalog.contains(&format!("main.{PROBE_TABLE}")),
        "目录里该能列举到表（挂在 main schema 下）：{catalog}"
    );
    assert!(
        catalog.contains(&format!("两段名读=Ok(\"3\")")),
        "两段名（别名.表）该能读到三行：{catalog}"
    );

    if let Some(result) = cross_source {
        let text = result.expect("跨源查询该成功");
        assert!(
            text.contains("表函数=3 行") && text.contains("目录=3 行"),
            "两条路径都该出三行（Oracle 3 行 × SQLite 1 行）：{text}"
        );
        eprintln!("✅ 跨源：Oracle × SQLite → {text}");
    } else {
        eprintln!("⏭️ 未设 RDS_TEST_SQLITE_PATH，跳过跨源部分");
    }

    dropped.expect("探针表该被删掉");
    assert_eq!(leftover.expect("该查得到计数"), "0", "探针表不该留在源库里");
    eprintln!("✅ Oracle 探针通过（探针表已清理；Oracle 只读靠编辑器闸门 + 只读账号）");
}
