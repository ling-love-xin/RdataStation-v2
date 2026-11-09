//! Quick Open 纯逻辑单元用例——按被测主题分组。
//!
//! [`query_rank`]：解析 / 门槛 / 命中区间 / 评分 / 高亮切段；
//! [`groups`]：分组装配 / 三源行映射 / 上限 / 命令目录；
//! [`goto`]：Goto 档（锁档 / 排序 / 上限）。

mod goto;
mod groups;
mod query_rank;

/// 造两条连接源（真实形状：`Vec<(名称, 驱动)>`）。
fn conns() -> Vec<(String, String)> {
    vec![
        ("营销分析（生产）".to_string(), "postgres".to_string()),
        ("分析库".to_string(), "duckdb".to_string()),
    ]
}

/// 造一条索引命中（真实形状：`database::nav_jobs::SearchHit`）。
fn hit(object_type: &str, name: &str, parent: Option<&str>) -> database::nav_jobs::SearchHit {
    database::nav_jobs::SearchHit {
        conn_id: "P_conn".to_string(),
        conn_label: "营销分析（生产）".to_string(),
        driver: "postgres".to_string(),
        object_type: object_type.to_string(),
        object_name: name.to_string(),
        parent_name: parent.map(|p| p.to_string()),
        catalog: Some("main".to_string()),
        schema: Some("public".to_string()),
        snippet: None,
        hit_source: database::nav_jobs::HitSource::Name,
    }
}

/// 造一份草稿箱文件（真实形状：`scratchpad::FlatFile` 经 `file_object` 映射）。
fn draft(relative_path: &str) -> FileObject {
    let (folder, name) = match relative_path.rsplit_once('/') {
        Some((dir, name)) => (dir.to_string(), name.to_string()),
        None => (String::new(), relative_path.to_string()),
    };
    file_object(&scratchpad::FlatFile {
        relative_path: relative_path.to_string(),
        folder,
        name,
        path: std::path::PathBuf::from("C:/proj/scratchpad").join(relative_path),
    })
}

use crate::quick_open::model::{FileObject, file_object};
