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

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{load_file, alias_for};
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
}
