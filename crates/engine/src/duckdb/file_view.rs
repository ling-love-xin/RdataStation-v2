//! 外部文件加载进**进程单例** DuckDB（`GLOBAL_DUCKDB`）
//!
//! 数据对象面板的「加源」走这里：二次分析与洞察本来就在这个实例里跑，
//! 把文件建成视图放进来，两处立刻都能查，不用各自 COPY 一份。
//!
//! 视图不物化：文件改了，下次查询结果跟着变。要“稳定快照”才需要物化成
//! 临时表（`tmp_q_`），那是另一条路。

use duckdb::Connection;
use shared::error::CommonError;

use crate::CoreError;

/// 文件视图的命名前缀：面板靠它把「我加载进来的文件」与别的视图区分开
pub const FILE_VIEW_PREFIX: &str = "f_";

/// 文件名 → 视图名（`f_<ascii 小写>`）
///
/// 视图名要能直接写进 SQL，所以只留 ASCII 字母数字，其余折成下划线：
/// 这是表名尺子的同一套口径（`temp_table::sanitize_description`），中文名折掉是
/// 已知代价，界面上要把“实际视图名”显示出来。
pub fn alias_for(path: &str) -> String {
    let stem = std::path::Path::new(path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("file");
    let cleaned: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('_');
    if trimmed.is_empty() {
        format!("{FILE_VIEW_PREFIX}file")
    } else {
        format!("{FILE_VIEW_PREFIX}{trimmed}")
    }
}

/// 在一个连接上把文件挂成视图（返回实际使用的视图名）
pub fn load_file(conn: &Connection, path: &str) -> Result<String, CoreError> {
    let alias = alias_for(path);
    let sql = crate::duckdb::federation::session::file_view_sql(path, &alias)
        .map_err(|error| CoreError::common(CommonError::General(error)))?;
    conn.execute_batch(&sql)
        .map_err(|error| CoreError::common(CommonError::General(format!("加载文件失败：{error}"))))?;
    Ok(alias)
}

/// 在进程单例上加载（事件路径调用：首次读文件会有 I/O）
pub fn load_file_global(path: &str) -> Result<String, CoreError> {
    let conn = crate::duckdb::manager::DuckDBManager::global()
        .lock()
        .map_err(|_| CoreError::common(CommonError::General("内存库连接锁被污染".to_string())))?;
    load_file(&conn, path)
}

/// 是不是**数据库文件**（`.duckdb` / `.sqlite`）：这类不走 reader 建视图，走
/// [`load_database`] `ATTACH` 挂成 catalog——表在库里，挂进来就能按
/// `别名.schema.表` 引用（2026-11-18 用户拍板：库文件走 ATTACH）。
pub fn is_database_file(path: &str) -> bool {
    matches!(
        std::path::Path::new(path)
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref(),
        Some("duckdb") | Some("sqlite") | Some("sqlite3") | Some("db")
    )
}

/// 把数据库文件 `ATTACH` 成连接上的 catalog（返回挂载别名，`f_` 前缀与视图同一套）
///
/// - `.duckdb` 文件是内核原生能力，直接 `ATTACH … (READ_ONLY)`；
/// - `.sqlite` 要 `sqlite` 扩展（与联邦源同一套装机路径，可离线预置），带 `TYPE sqlite`；
/// - 只读挂载与联邦源同一纪律：文件是外部的，不从这条连接写它。
pub fn load_database(conn: &Connection, path: &str) -> Result<String, CoreError> {
    let alias = alias_for(path);
    let is_sqlite = std::path::Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            ext.eq_ignore_ascii_case("sqlite") || ext.eq_ignore_ascii_case("sqlite3") || ext.eq_ignore_ascii_case("db")
        });
    if is_sqlite {
        // 装扩展在 mount_lock 内做（INSTALL 写扩展目录，并发会撞文件——与联邦同规）
        let _mount = crate::duckdb::accel::mount_lock();
        crate::duckdb::accel::ensure_extension(conn, crate::duckdb::accel::AccelKind::Sqlite)
            .map_err(|reason| CoreError::common(CommonError::General(reason)))?;
    }
    let quoted_alias = alias.replace('"', "\"\"");
    // 同名重复挂载先卸旧的：`ATTACH` 对已存在的别名直接报错（不像
    // `CREATE OR REPLACE VIEW` 天然幂等）。换路径重挂 = 覆盖语义，与视图重建同口径；
    // `DETACH` 不存在（首次挂载）会报错，忽略即可。默认 catalog 指向该别名时
    // DuckDB 拒绝卸载——单例的默认 catalog 恒为 memory，不走那个分支
    let _ = conn.execute_batch(&format!("USE memory; DETACH \"{quoted_alias}\""));
    let quoted_path = path.replace('\'', "''");
    let sql = if is_sqlite {
        format!("ATTACH '{quoted_path}' AS \"{quoted_alias}\" (TYPE sqlite, READ_ONLY)")
    } else {
        format!("ATTACH '{quoted_path}' AS \"{quoted_alias}\" (READ_ONLY)")
    };
    conn.execute_batch(&sql).map_err(|error| {
        CoreError::common(CommonError::General(format!("挂载数据库文件失败：{error}")))
    })?;
    Ok(alias)
}

/// 在进程单例上挂载数据库文件（事件路径调用）
pub fn load_database_global(path: &str) -> Result<String, CoreError> {
    let conn = crate::duckdb::manager::DuckDBManager::global()
        .lock()
        .map_err(|_| CoreError::common(CommonError::General("内存库连接锁被污染".to_string())))?;
    load_database(&conn, path)
}

/// 列出**进程单例**上某个挂载别名（数据库文件）里的表 / 视图
///
/// 与联邦 / 加速源同一份列举实现（`duckdb_tables()` / `duckdb_views()` 按别名过滤，
/// 限定名三段）——数据对象面板「源行展开」不分挂的是网络源还是本机库文件。
pub fn list_tables_global(
    alias: &str,
) -> Result<Vec<crate::duckdb::accel::MountedTable>, String> {
    let conn = crate::duckdb::manager::DuckDBManager::global()
        .lock()
        .map_err(|_| "内存库连接锁被污染".to_string())?;
    crate::duckdb::accel::list_mounted_tables(
        &conn,
        alias,
        crate::duckdb::accel::AccelKind::DuckDb,
    )
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{alias_for, is_database_file, load_database, load_database_global, load_file, list_tables_global};
    use std::path::PathBuf;

    fn workdir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rds_file_view_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建目录");
        dir
    }

    #[test]
    fn the_alias_keeps_ascii_and_folds_the_rest() {
        assert_eq!(alias_for("/tmp/orders_2026.csv"), "f_orders_2026");
        assert_eq!(alias_for("/tmp/经营月报.xlsx"), "f_file");
    }

    #[test]
    fn a_csv_is_loadable_into_the_instance() {
        let dir = workdir("csv");
        let path = dir.join("orders.csv");
        std::fs::write(&path, "id,name\n1,a\n2,b\n").expect("写 csv");

        let conn = duckdb::Connection::open_in_memory().expect("内存库");
        let alias = load_file(&conn, path.to_str().expect("路径")).expect("加载");
        assert_eq!(alias, "f_orders");
        // 视图建好了就能查：查不存在会报错，这里只看“不报错”
        conn.execute_batch("SELECT count(*) FROM f_orders")
            .expect("视图应能查");
    }

    /// 库文件按扩展分流：duckdb / sqlite 是库（ATTACH），csv 是数据（视图）
    #[test]
    fn database_files_are_recognized_by_extension() {
        assert!(is_database_file("D:/data/sales.duckdb"));
        assert!(is_database_file("D:/data/old.sqlite"));
        assert!(is_database_file("D:/data/x.db"));
        assert!(!is_database_file("D:/data/sales.csv"));
        assert!(!is_database_file("D:/data/report.parquet"));
    }

    /// 库文件在**任意连接**上可挂（连接级形态先行验证，不依赖进程单例）
    #[test]
    fn a_duckdb_file_attaches_on_a_connection() {
        let dir = workdir("attach_conn");
        let source = dir.join("warehouse.duckdb");
        {
            let conn = duckdb::Connection::open(&source).expect("建源库文件");
            conn.execute_batch("CREATE TABLE facts AS SELECT i AS id FROM range(3) AS r(i)")
                .expect("建表");
        }
        let conn = duckdb::Connection::open_in_memory().expect("内存库");
        let alias = load_database(&conn, source.to_str().expect("路径")).expect("挂载");
        assert_eq!(alias, "f_warehouse");
        conn.execute_batch("SELECT count(*) FROM f_warehouse.main.facts")
            .expect("限定名可查");

        // 同名重挂（不同目录的同名文件 → 同一个别名）：ATTACH 不是幂等的，
        // 靠先 DETACH 实现覆盖语义——重挂后指向新文件
        let other_dir = dir.join("elsewhere");
        std::fs::create_dir_all(&other_dir).expect("建目录");
        let second = other_dir.join("warehouse.duckdb");
        std::fs::copy(&source, &second).expect("复制一份同名库");
        let alias_again =
            load_database(&conn, second.to_str().expect("路径")).expect("重挂要能成");
        assert_eq!(alias_again, "f_warehouse", "别名稳定（覆盖语义）");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 进程单例路径：挂载后面板的列举口能看到表（含可粘贴的限定名）
    #[test]
    fn a_duckdb_file_mounts_and_lists_on_the_singleton() {
        let dir = workdir("attach_global");
        let source = dir.join("archive.duckdb");
        {
            let conn = duckdb::Connection::open(&source).expect("建源库文件");
            conn.execute_batch("CREATE TABLE dims AS SELECT i AS id FROM range(2) AS r(i)")
                .expect("建表");
        }
        let path = source.to_str().expect("路径");
        let alias = load_database_global(path).expect("挂到单例");
        assert_eq!(alias, "f_archive");
        let tables = list_tables_global("f_archive").expect("列举");
        let dims = tables.iter().find(|t| t.name == "dims").expect("有 dims");
        assert_eq!(dims.schema, "main");
        assert!(dims.qualified.contains("f_archive"), "{}", dims.qualified);
        // 收尾：单例是进程级的，别把挂载留给其他用例
        {
            let singleton = crate::duckdb::manager::DuckDBManager::global();
            let guard = singleton.lock().expect("单例锁");
            let _ = guard.execute_batch("DETACH f_archive");
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
