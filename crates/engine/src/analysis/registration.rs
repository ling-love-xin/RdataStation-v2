//! 分析源登记的**幂等写入**：「连接开关有值 → 项目登记表里必须有一行」的唯一落点。
//!
//! 为什么放在 engine 而不是 store 或 UI：
//! - 登记要分配别名（`federation::registry::default_alias`），那个函数在 engine 的
//!   duckdb 模块里；放 `store` 会造出 store → engine 的反向依赖。
//! - 两个入口（数据对象面板「加源」、连接对话框保存 / 只翻开关）都要写这一行，
//!   放在任一边都会逼另一个跨 crate 调 UI 层；放 engine 则谁都能调、且只有一处实现。
//!
//! 只写**意图**（项目级、切项目还在），不碰运行态挂载——挂载是会话事实，
//! 由 `AnalysisInstance` 承担（见 `docs/architecture/database/analysis-instance-sessions.md`）。

use std::path::Path;

/// 把一条连接登记进项目库 `analysis_sources`（**幂等**：同一 `conn_id` 已登记就不再插）。
///
/// 返回 `Some(alias)` = 本次新插（alias 是分配出来的展示名）；`None` = 已登记，未重复写。
///
/// 展示名去重看**全表**（`alias` 是树根的显示名，撞了会出两行同名源），
/// 但加速档的别名不占挂载名（迁移 035 按档位分域）——这里只保证展示名不撞。
pub fn ensure_conn_registered(
    project_root: &Path,
    conn_id: &str,
    name: &str,
) -> Result<Option<String>, String> {
    let conn = crate::persistence::analysis_sources::open_in_project(project_root)
        .map_err(|error| format!("打开项目库失败：{error}"))?;
    let rows = crate::persistence::analysis_sources::list(&conn)
        .map_err(|error| format!("读已有登记失败：{error}"))?;
    // 同一份登记表可能被多个入口各登记一次（面板「加源」/ 对话框保存 / 右键「登记为项目源」），
    // 写的都是同一个 connection_id —— 不该出现第二行（面板会出两条同名源、别名被挤成 `shop_2`）
    if rows
        .iter()
        .any(|row| row.connection_id.as_deref() == Some(conn_id))
    {
        return Ok(None);
    }
    let taken: Vec<String> = rows.iter().map(|row| row.alias.clone()).collect();
    let alias = crate::duckdb::federation::registry::default_alias(name, &taken);
    let now = chrono::Utc::now().to_rfc3339();
    let row = crate::persistence::analysis_sources::AnalysisSourceRow {
        id: format!("src_{}", uuid::Uuid::new_v4().simple()),
        name: name.to_string(),
        kind: crate::persistence::analysis_sources::AnalysisSourceKind::Conn,
        connection_id: Some(conn_id.to_string()),
        path: None,
        alias: alias.clone(),
        // 登记只是「这条连接可以当分析源」的意图；挂成联邦还是加速由执行时定。
        // 老代码写 `Accel` 是错的：加速档不占源段、且别名对它是展示名。
        mount_kind: crate::persistence::analysis_sources::AnalysisMountKind::Federation,
        created_at: now.clone(),
        updated_at: now,
    };
    crate::persistence::analysis_sources::insert(&conn, &row)
        .map_err(|error| format!("登记失败：{error}"))?;
    Ok(Some(alias))
}
