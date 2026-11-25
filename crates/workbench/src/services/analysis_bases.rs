//! 分析底座接线：把「项目分析库 + 全局分析库」发布给联邦会话。
//!
//! 两块底座（`analytics` / `rds_global`）在建会话时由引擎**只读挂载**（`ATTACH ... READ_ONLY`），
//! 这样「源库 SQL 切联邦」时，`analytics` 里的 `res_*` 和全局的 `G_*` 本来就可见——
//! 用户不用先手动载入，SQL 也不用改写。铁律第 4 条：底座**不允许卸载**（不是会话级加源）。
//!
//! 分工：路径解析是宿主的事（`RDS_HOME` / 项目 `.RSmeta`），引擎只管挂。
//! 调用时机：打开 / 关闭项目、切项目（会话指纹含底座，变化即重建会话）。

use std::path::Path;

use engine::duckdb::federation::session::{
    BaseMount, GLOBAL_BASE_ALIAS, PROJECT_BASE_ALIAS,
};

/// 按项目根发布底座清单（`None` = 没有打开的项目，只剩全局库）。
///
/// 缺文件的底座不预建（`attach_bases` 自己跳过）：**不在这里造 `.RSmeta/analytics.duckdb`**，
/// 建库是「第一次要写分析表」时的动作，不是打开项目时的副作用。
pub fn sync(project_root: Option<&Path>) {
    let mut bases = Vec::new();
    if let Some(root) = project_root {
        bases.push(BaseMount {
            alias: PROJECT_BASE_ALIAS.to_string(),
            path: engine::analysis::persistent::project_analysis_db_path(root)
                .to_string_lossy()
                .into_owned(),
        });
    }
    // 全局库路径依赖 `RDS_HOME`：解析不出来就只挂项目库，别把整份清单拖垮
    if let Ok(path) = engine::analysis::global::global_analysis_db_path() {
        bases.push(BaseMount {
            alias: GLOBAL_BASE_ALIAS.to_string(),
            path: path.to_string_lossy().into_owned(),
        });
    }
    engine::duckdb::federation::session::set_bases(bases);
}
