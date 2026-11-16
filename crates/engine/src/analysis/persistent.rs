//! 项目分析库 · 持久表（结果集落库产物）的清单与删除口径。
//!
//! 为什么单独立一层：`res_` 表的生命周期由**用户**决定（他存的，就得他能删），
//! 而落库那一侧（`duckdb::temp_table::promote`）只管把表写进去。把「列 / 删」
//! 收在这里，前缀守卫只有一个出处，不会出现「落库按一个前缀写、删除按另一个前缀认」。
//!
//! # 边界
//!
//! - 只碰 `PERSIST_PREFIX`（`res_`）开头的表：这个库同时装着项目自建的分析表，
//!   名字判断错了就是删掉别人的数据；
//! - **不是回收站**：删除是真删（DuckDB 里 `DROP TABLE` 没有回收站）。
//!   要「可撤销」得先把表登记进资产库（`analytics_resources`）走它的 trash，
//!   那是另一层语义（本体归谁、版本怎么算），不是这里该顺手决定的；
//! - 库不存在 / 表不存在一律当「没有」，不报错：调用方（列表、清理）都是幂等语义。

use std::path::{Path, PathBuf};

use shared::error::{CommonError, CoreError};

use crate::duckdb::temp_table::promote::PERSIST_PREFIX;

/// 项目分析库路径：`<项目>/.RSmeta/analytics.duckdb`（与资产库侧同一口径）。
pub fn project_analysis_db_path(project_root: &Path) -> PathBuf {
    project_root.join(".RSmeta").join("analytics.duckdb")
}

/// 列项目分析库里的**持久表**（`res_` 前缀），按名字排序。
///
/// 库不存在时返回空：项目刚建、还没落过任何表是正常状态，不是错误。
pub fn list_persistent_tables(project_root: &Path) -> Result<Vec<String>, CoreError> {
    let path = project_analysis_db_path(project_root);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let conn = duckdb::Connection::open(&path)
        .map_err(|e| invalid(format!("打开项目分析库失败（可能正被占用）: {e}")))?;
    let mut stmt = conn
        .prepare("select table_name from duckdb_tables() order by table_name")
        .map_err(|e| invalid(format!("查表清单失败: {e}")))?;
    let names = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| invalid(format!("查表清单失败: {e}")))?
        .filter_map(Result::ok)
        .filter(|name| name.starts_with(PERSIST_PREFIX))
        .collect();
    Ok(names)
}

/// 删一张持久表；返回是否真的删掉了（不存在 → `false`，不是错误）。
///
/// 前缀守卫在**开库之前**：名字不对就不该碰这个文件。
pub fn drop_persistent_table(project_root: &Path, name: &str) -> Result<bool, CoreError> {
    if !name.starts_with(PERSIST_PREFIX) {
        return Err(invalid(format!(
            "只允许删除持久表（{PERSIST_PREFIX} 开头）：{name}"
        )));
    }
    let path = project_analysis_db_path(project_root);
    if !path.exists() {
        return Ok(false);
    }
    let conn = duckdb::Connection::open(&path)
        .map_err(|e| invalid(format!("打开项目分析库失败（可能正被占用）: {e}")))?;
    let exists = conn
        .prepare("select count(*) from duckdb_tables() where table_name = ?")
        .and_then(|mut stmt| stmt.query_row([name], |row| row.get::<_, i64>(0)))
        .map_err(|e| invalid(format!("查表是否存在失败: {e}")))?;
    if exists == 0 {
        return Ok(false);
    }
    let quoted = name.replace('"', "\"\"");
    conn.execute(&format!("drop table \"{quoted}\""), [])
        .map_err(|e| invalid(format!("删除表失败（可能正被别的会话占用）: {e}")))?;
    Ok(true)
}

fn invalid(message: String) -> CoreError {
    CoreError::common(CommonError::General(message))
}

#[cfg(test)]
mod tests {
    use super::{drop_persistent_table, list_persistent_tables, project_analysis_db_path};

    /// 造一个「项目根 + 分析库」：`.RSmeta/analytics.duckdb`。
    fn project_with_tables(tag: &str, tables: &[(&str, &str)]) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("rds_persistent_probe_{tag}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".RSmeta")).expect("目录");
        let conn =
            duckdb::Connection::open(project_analysis_db_path(&root)).expect("建库");
        for (name, ddl) in tables {
            conn.execute(ddl, []).expect("建表");
            assert!(ddl.contains(name), "建表语句与表名要对上：{ddl} / {name}");
        }
        root
    }

    #[test]
    fn only_persisted_prefixed_tables_are_listed() {
        let root = project_with_tables(
            "list",
            &[
                ("res_orders_20261115103000", "create table res_orders_20261115103000 (a int)"),
                ("数据修复", "create table \"数据修复\" (a int)"),
                ("tmp_q_1", "create table tmp_q_1 (a int)"),
            ],
        );
        let names = list_persistent_tables(&root).expect("列清单");
        assert_eq!(names, vec!["res_orders_20261115103000".to_string()]);
    }

    #[test]
    fn missing_database_is_empty_not_an_error() {
        let root = std::env::temp_dir().join("rds_persistent_probe_missing");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("目录");
        assert!(list_persistent_tables(&root).expect("空清单").is_empty());
        assert!(!drop_persistent_table(&root, "res_anything").expect("幂等"));
    }

    #[test]
    fn drop_is_idempotent_and_guarded_by_prefix() {
        let root = project_with_tables(
            "drop",
            &[
                ("res_keep_20261115103000", "create table res_keep_20261115103000 (a int)"),
                ("数据修复", "create table \"数据修复\" (a int)"),
            ],
        );
        assert!(drop_persistent_table(&root, "res_keep_20261115103000").expect("第一次删"));
        assert!(
            !drop_persistent_table(&root, "res_keep_20261115103000").expect("再删一次"),
            "已删掉的表再删不该报已删除"
        );
        // 非持久表前缀：守卫在开库前就拒，绝不能碰项目自建的分析表
        let err = drop_persistent_table(&root, "数据修复").unwrap_err();
        assert!(err.to_string().contains("只允许删除持久表"), "{err}");
        assert_eq!(
            list_persistent_tables(&root).expect("清单"),
            Vec::<String>::new()
        );
        // 项目自建的分析表还在
        let conn = duckdb::Connection::open(project_analysis_db_path(&root)).expect("打开");
        let n: i64 = conn
            .prepare("select count(*) from duckdb_tables() where table_name = ?")
            .and_then(|mut stmt| stmt.query_row(["数据修复"], |row| row.get(0)))
            .expect("查在不在");
        assert_eq!(n, 1, "非 res_ 的表必须原封不动");
    }
}
