//! DuckDB 导出的真机能力台账（B7 切片二）
//!
//! 问的是「结果集 → Parquet / XLSX 到底能不能落盘、落了的东西能不能读回来」。
//! 这两条决定界面上怎么说话、失败原因怎么写。
//!
//! ```text
//! cargo test -p rds-engine --test duckdb_export_probe -- --nocapture --test-threads=1
//! # 要连安装那一步一起验（会联网下载 ~30MB 的 excel 扩展）：
//! RDS_TEST_EXCEL_INSTALL=1 cargo test -p rds-engine --test duckdb_export_probe -- --nocapture
//! ```
//!
//! **Parquet 不需要任何环境与扩展**（DuckDB 内核自带）：这条必须过，它是离线回归。
//! **XLSX 要 `excel` 扩展**：默认只试 `LOAD`（离线）；装不上就**如实打印并跳过**（不算失败）——
//! `INSTALL` 会下载 ~30MB，不该由默认的 `cargo test` 触发（要验安装路径就设上面那个环境变量）。
//!
//! ## 已核实的结论（2026-09-18 实跑）
//!
//! | 问题 | 结论 |
//! | --- | --- |
//! | Parquet 能不能写 | ✅ `COPY (SELECT * FROM <临时表>) TO 'x.parquet' (FORMAT parquet)` 落盘（3 行 / 350 字节 / 24 ms），`read_parquet` 读回来列名 / 行序 / NULL 全对 |
//! | XLSX 能不能写 | ✅ 走 `LOAD`→`INSTALL`→`LOAD` 三步（首次联网 3–11 秒）后写得出（3756 字节） |
//! | XLSX 表头 | ❌ 不传选项时**不写表头**（`read_xlsx(…, header = true)` 会把 `1` 当列名）→ 写时必须显式 `HEADER true` |
//! | XLSX 数字 | 读回来是**双精度**（`1` → `Number(1.0)`）：xlsx 的存储就是 double，不是我们的取舍 |
//! | 扩展装在哪 | `extension_directory` 下的 `<版本>/<平台>/`（本机是 `…/extensions/v1.5.5/windows_amd64/`），与 `paths::extensions_dir()` 同一棵目录 |

use serde_json::{Value, json};

use rds_engine::services::duckdb_service::DuckDbService;
use rds_engine::services::execution_service::{
    DuckDbExportFormat, export_rows_via_duckdb,
};

/// 探测用的三行（含一个 NULL）：数字 / 文本 / 空
fn probe_rows() -> (Vec<String>, Vec<Vec<Value>>) {
    (
        vec!["id".to_string(), "name".to_string()],
        vec![
            vec![json!(1), json!("alpha")],
            vec![json!(2), json!("beta")],
            vec![json!(3), Value::Null],
        ],
    )
}

fn probe_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rds_export_probe_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("建探测目录");
    dir
}

/// Parquet：内核自带，**离线必须过**（写出 → 读回 → 值/NULL/列名逐个断言）
#[test]
fn parquet_export_lands_and_reads_back() {
    let dir = probe_dir();
    let path = dir.join("probe.parquet");
    std::fs::remove_file(&path).ok();

    let (columns, rows) = probe_rows();
    let stats = export_rows_via_duckdb(&columns, &rows, &path, DuckDbExportFormat::Parquet)
        .expect("Parquet 导出（内核自带，不需要扩展）");
    let size = std::fs::metadata(&path).expect("文件应当写出来").len();
    assert!(size > 0, "Parquet 文件不该是空的");
    println!("✅ Parquet：{} 行 / {size} 字节 / {} ms", stats.rows, stats.elapsed_ms);

    let duckdb = DuckDbService::get_or_create_duckdb().expect("内存库");
    let mut conn = duckdb.lock().expect("锁");
    let file = path.to_string_lossy().replace('\\', "/");
    let (cols, got) = DuckDbService::query_duckdb(
        &mut conn,
        &format!("SELECT * FROM read_parquet('{file}') ORDER BY id"),
    )
    .expect("读回 Parquet");
    assert_eq!(cols, vec!["id".to_string(), "name".to_string()]);
    assert_eq!(got.len(), 3);
    assert_eq!(got[0], vec![json!(1), json!("alpha")]);
    assert_eq!(got[2], vec![json!(3), Value::Null], "NULL 要真是 NULL");
    println!("✅ Parquet 读回：{got:?}");

    std::fs::remove_file(&path).ok();
    std::fs::remove_dir_all(&dir).ok();
}

/// 扩展到底装在哪、装没装上：这句话要写进文档与失败文案（“离线时把文件放哪”）
#[test]
fn excel_extension_state_is_visible() {
    let duckdb = DuckDbService::get_or_create_duckdb().expect("内存库");
    let mut conn = duckdb.lock().expect("锁");

    let (_, dir) = DuckDbService::query_duckdb(
        &mut conn,
        "SELECT current_setting('extension_directory')",
    )
    .expect("扩展目录");
    println!("扩展目录：{dir:?}");

    let _ = conn.execute_batch("LOAD excel");
    let (cols, rows) = DuckDbService::query_duckdb(
        &mut conn,
        "SELECT extension_name, installed, loaded FROM duckdb_extensions() \
         WHERE extension_name IN ('excel', 'parquet') ORDER BY extension_name",
    )
    .expect("扩展状态");
    println!("列 = {cols:?}");
    for row in rows {
        println!("  {row:?}");
    }
}

/// XLSX：要 `excel` 扩展。默认只试 `LOAD`（离线）；`INSTALL` 得显式要（它要下载 ~30MB）
#[test]
fn xlsx_export_lands_and_reads_back() {
    let install_allowed = std::env::var_os("RDS_TEST_EXCEL_INSTALL").is_some();
    if !install_allowed {
        // 预检只用离线那一步：没装就跳过，不触发下载
        let duckdb = DuckDbService::get_or_create_duckdb().expect("内存库");
        let conn = duckdb.lock().expect("锁");
        if conn.execute_batch("LOAD excel").is_err() {
            println!(
                "⏭️ XLSX 跳过：excel 扩展没装（设 RDS_TEST_EXCEL_INSTALL=1 才走 INSTALL，会联网下载）"
            );
            return;
        }
    }

    let dir = probe_dir();
    let path = dir.join("probe.xlsx");
    std::fs::remove_file(&path).ok();

    let (columns, rows) = probe_rows();
    let stats = match export_rows_via_duckdb(&columns, &rows, &path, DuckDbExportFormat::Xlsx) {
        Ok(stats) => stats,
        Err(error) => {
            println!("⏭️ XLSX 跳过（excel 扩展不可用，环境相关，不算失败）：{error}");
            return;
        }
    };
    let size = std::fs::metadata(&path).expect("文件应当写出来").len();
    assert!(size > 0, "XLSX 文件不该是空的");
    println!("✅ XLSX：{} 行 / {size} 字节 / {} ms", stats.rows, stats.elapsed_ms);

    // 读回来（同一个扩展负责读）：`header = true` 才会把第一行当列名
    // （不传的话列名是 `A1` / `A2` 这种单元格坐标——探针第一版就摔在这里）
    let duckdb = DuckDbService::get_or_create_duckdb().expect("内存库");
    let mut conn = duckdb.lock().expect("锁");
    let file = path.to_string_lossy().replace('\\', "/");
    let (cols, got) = DuckDbService::query_duckdb(
        &mut conn,
        &format!("SELECT * FROM read_xlsx('{file}', header = true) ORDER BY id"),
    )
    .expect("读回 XLSX");
    println!("✅ XLSX 读回：列 = {cols:?}，行 = {got:?}");
    assert_eq!(cols, vec!["id".to_string(), "name".to_string()], "导出要带表头");
    assert_eq!(got.len(), 3, "三行都要在");
    // 数字在 xlsx 里是双精度（表格软件的存储就是 double），所以按数值比而不是按字面
    assert_eq!(got[0][0].as_f64(), Some(1.0));
    assert_eq!(got[0][1], json!("alpha"));
    assert_eq!(got[2][1], Value::Null, "NULL 要真是 NULL");

    std::fs::remove_file(&path).ok();
    std::fs::remove_dir_all(&dir).ok();
}
