//! 工作台概览卡片的计数快照。
//!
//! 「工作台」tab 顶部四张卡读的是**真实业务数据**，不是模块能力清单：
//!
//! | 卡 | 数字来源 | 库 |
//! | --- | --- | --- |
//! | 连接 | `Shared::connections`（内存里的连接清单 + 连接状态） | 内存 |
//! | 项目 | 元数据表 / 列行数 | SQLite `{项目}/.RSmeta/project.db` 的 `tables` / `columns` |
//! | 洞察 | 已存画像报表条数（表画像 + 结构画像） | DuckDB `{项目}/.RSmeta/analytics.duckdb` |
//! | Mock | 生成器种类（能力）+ 分析库表数（`mock` 落库产物都在分析库里） | 代码目录 + DuckDB |
//!
//! 取数一律在这里（**后台线程**）完成，`render` 只读 [`OverviewCounts`] 快照——
//! gpui-kit 规范要求 render 是纯读路径，不做 I/O。
//!
//! 每一项都允许失败：项目没初始化、库文件不存在、表还没建出来时对应字段保持 `None`
//! （卡片显示「—」），失败原因收进 [`OverviewCounts::problems`] 由卡片提示条呈现，
//! 不让单库异常把整页渲染带崩。

use std::path::Path;

/// 概览卡片的一次取数结果。`None` = 这一项取不到（卡片显示「—」）。
#[derive(Clone, Debug, Default)]
pub struct OverviewCounts {
    /// 当前项目名（未打开项目时为 None：卡片显示「未打开项目」）。
    pub project_name: Option<String>,
    /// 连接总数（当前作用域）。
    pub connections: usize,
    /// 其中处于已连接状态的个数。
    pub connected: usize,
    /// 项目元数据表数。
    pub project_tables: Option<usize>,
    /// 项目元数据列数。
    pub project_columns: Option<usize>,
    /// 已存洞察画像条数（表画像 + 结构画像）。
    pub insight_reports: Option<usize>,
    /// 分析库里的表数（mock 落库产物也在这里）。
    pub analysis_tables: Option<usize>,
    /// mock 生成器种类数（能力口径，与项目无关，恒定）。
    pub mock_generators: usize,
    /// 取数失败原因（每条一句话，卡片提示条展示）。
    pub problems: Vec<String>,
}

/// 取数入口（**在后台线程调用**）。
///
/// `project` = `(显示名, 项目根)`；未打开项目时项目侧三项保持 `None`——那不是错误
/// （应用本来就允许不开项目），所以不进 `problems`。
pub fn load(project: Option<(&str, &Path)>, connected_flags: &[bool]) -> OverviewCounts {
    let mut counts = OverviewCounts {
        project_name: project.map(|(name, _)| name.to_string()),
        connections: connected_flags.len(),
        connected: connected_flags.iter().filter(|c| **c).count(),
        mock_generators: mock::generator_catalog::all_specs().len(),
        ..Default::default()
    };

    let Some((_, root)) = project else {
        return counts;
    };

    match metadata_counts(root) {
        Ok((tables, columns)) => {
            counts.project_tables = Some(tables);
            counts.project_columns = Some(columns);
        }
        Err(problem) => counts.problems.push(problem),
    }

    match analysis_counts(root) {
        Ok((reports, tables)) => {
            counts.insight_reports = Some(reports);
            counts.analysis_tables = Some(tables);
        }
        Err(problem) => counts.problems.push(problem),
    }

    counts
}

/// 项目元数据表 / 列行数（SQLite 只读打开：与主写连接并发读是安全的）。
fn metadata_counts(root: &Path) -> Result<(usize, usize), String> {
    let db = root.join(".RSmeta").join("project.db");
    if !db.is_file() {
        return Err("项目库未初始化：缺 .RSmeta/project.db".to_string());
    }
    let conn = rusqlite::Connection::open_with_flags(
        &db,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|e| format!("打开项目库失败：{e}"))?;

    let count = |table: &str| -> Result<usize, String> {
        let sql = format!("SELECT COUNT(*) FROM \"{table}\"");
        conn.query_row(&sql, [], |row| row.get::<_, i64>(0))
            .map(|n| n.max(0) as usize)
            .map_err(|e| format!("统计 {table} 失败：{e}"))
    };
    let tables = count("tables")?;
    let columns = count("columns")?;
    Ok((tables, columns))
}

/// 分析库里的洞察画像条数 + 表数（DuckDB）。
///
/// 画像表的建表语句在 `insight/src/store/body.rs`（`insight_table_reports` /
/// `insight_schema_reports`）：表还没建出来（本项目没跑过洞察）时按 0 计，不算失败。
fn analysis_counts(root: &Path) -> Result<(usize, usize), String> {
    let db = engine::persistence::project_db::ProjectDatabaseManager::analysis_db_path(root);
    if !db.is_file() {
        return Err("分析库还不存在：本项目还没有跑过查询 / 洞察 / mock".to_string());
    }
    let conn = duckdb::Connection::open(&db).map_err(|e| format!("打开分析库失败：{e}"))?;

    let mut reports = 0usize;
    for table in ["insight_table_reports", "insight_schema_reports"] {
        let sql = format!("SELECT COUNT(*) FROM {table}");
        // 表不存在 → err（本项目没跑过该类洞察），按 0 计，不打断另一张表的统计。
        if let Ok(n) = conn.query_row(&sql, [], |row| row.get::<_, i64>(0)) {
            reports += n.max(0) as usize;
        }
    }

    let tables = crate::services::mock_generator::existing_tables_at(&db).len();
    Ok((reports, tables))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 未打开项目：只用内存里的连接清单 + 代码目录里的生成器数，项目三项保持 None。
    #[test]
    fn counts_connections_without_project() {
        let counts = load(None, &[true, false, true]);
        assert_eq!(counts.connections, 3);
        assert_eq!(counts.connected, 2);
        assert_eq!(
            counts.mock_generators,
            mock::generator_catalog::all_specs().len()
        );
        assert!(counts.project_name.is_none());
        assert!(counts.project_tables.is_none());
        assert!(counts.project_columns.is_none());
        assert!(counts.insight_reports.is_none());
        // 未开项目不是错误：提示条必须为空。
        assert!(counts.problems.is_empty());
    }

    /// 造一个最小项目库（`.RSmeta/project.db` + `tables`/`columns` 两表）验证 SQLite 侧读数。
    ///
    /// 表结构只要够 `COUNT(*)` 即可：本模块不做列级查询，用真实 schema 反而会把测试
    /// 绑死在别处的迁移上。
    #[test]
    fn reads_project_metadata_from_sqlite() {
        let root = std::env::temp_dir().join(format!("rds-overview-{}", uuid::Uuid::new_v4()));
        let meta = root.join(".RSmeta");
        std::fs::create_dir_all(&meta).expect("create meta dir");

        {
            let conn = rusqlite::Connection::open(meta.join("project.db")).expect("open sqlite");
            conn.execute_batch(
                "CREATE TABLE tables (id TEXT); CREATE TABLE columns (id TEXT);",
            )
            .expect("create tables");
            conn.execute_batch(
                "INSERT INTO tables (id) VALUES ('t1'), ('t2');
                 INSERT INTO columns (id) VALUES ('c1'), ('c2'), ('c3');",
            )
            .expect("seed rows");
        }

        let counts = load(Some(("demo", root.as_path())), &[]);
        assert_eq!(counts.project_name.as_deref(), Some("demo"));
        assert_eq!(counts.project_tables, Some(2));
        assert_eq!(counts.project_columns, Some(3));
        // 分析库没建：该项按「取不到」处理并给出可读原因，而不是 panic / 阻塞。
        assert_eq!(counts.insight_reports, None);
        assert_eq!(counts.analysis_tables, None);
        assert!(
            counts.problems.iter().any(|p| p.contains("分析库")),
            "应有一条分析库缺失说明：{:?}",
            counts.problems
        );

        std::fs::remove_dir_all(&root).ok();
    }
}
