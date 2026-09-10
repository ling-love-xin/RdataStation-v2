//! Round 25：M4 数据库导航——DuckDB 分析库元数据树（表 → 列）。
//!
//! 用 engine 原生 DuckDB 驱动（`DuckDbDatabase`）读取分析引擎库，
//! 返回 表 → 列 两级导航结构，供工作台「数据库导航」区渲染。
//! DuckDB 无 schema 层级（统一 `main`），故树为两层；外部联邦库的
//! schema/表由连接建立后的元数据服务（M4 `MetadataService`）扩展。

use std::path::Path;

use engine::driver::native::duckdb::DuckDbDatabase;
use engine::Database;

/// 导航列（列名 + 类型 + 键/空标记）
#[derive(Debug, Clone)]
pub struct NavColumn {
    pub name: String,
    pub data_type: String,
    pub is_primary_key: bool,
    pub is_nullable: bool,
}

/// 导航表（表名 + 列清单）
#[derive(Debug, Clone)]
pub struct NavTable {
    pub name: String,
    pub columns: Vec<NavColumn>,
}

/// 打开 DuckDB 分析库，返回表 → 列导航树。
///
/// 打开失败 / 读取失败均返回中文错误提示；空库返回空 Vec（调用方显示空态）。
pub fn load_navigator_tree(duckdb_path: &Path) -> Result<Vec<NavTable>, String> {
    let path_str = duckdb_path.to_string_lossy();
    let db = DuckDbDatabase::new(&path_str).map_err(|e| format!("打开分析库失败: {e}"))?;

    let runtime = tokio::runtime::Runtime::new().map_err(|e| format!("无法启动异步运行时: {e}"))?;
    runtime.block_on(async {
        let tables = db
            .list_tables("main", Some("main"))
            .await
            .map_err(|e| format!("读取表清单失败: {e}"))?;

        let mut out = Vec::new();
        for t in tables {
            let cols = db
                .list_columns("main", Some("main"), &t.name)
                .await
                .map_err(|e| format!("读取表「{}」列失败: {e}", t.name))?;
            out.push(NavTable {
                name: t.name,
                columns: cols
                    .into_iter()
                    .map(|c| NavColumn {
                        name: c.name,
                        data_type: c.data_type,
                        is_primary_key: c.is_primary_key,
                        is_nullable: c.nullable,
                    })
                    .collect(),
            });
        }
        Ok(out)
    })
}
