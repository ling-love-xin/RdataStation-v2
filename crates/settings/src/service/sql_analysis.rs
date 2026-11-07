//! SQL 分析设置项的写入处理（`apply_slot`，由 `SettingsService::apply_by_key` 分发）
//! 与读写访问器。
//!
//! 与 `service/insight.rs` 同构：写值 → 落盘 → 通过 sink 推给真正的消费方。
//! SQL 分析的消费方是 `rds_sql::analysis`（进程内原子量），装配层
//! （`crates/app/src/main.rs`）把 `set_prefs` 接在 [`install_sql_analysis_sink`] 上。
use std::sync::RwLock;

use gpui_kit::App;

use crate::SettingsService;
use crate::model::{Settings, SqlAnalysis};
use crate::persistence::persist;
use crate::registry::{SettingValue, Slot};

/// 偏好出口（三开关；由装配层注册一次）
static SQL_ANALYSIS_SINK: RwLock<Option<fn(bool, bool, bool)>> = RwLock::new(None);

/// 注册偏好出口（装配层调用一次；重复调用覆盖）
pub fn install_sql_analysis_sink(sink: fn(bool, bool, bool)) {
    if let Ok(mut guard) = SQL_ANALYSIS_SINK.write() {
        *guard = Some(sink);
    }
}

fn notify_sql_analysis(prefs: SqlAnalysis) {
    if let Some(sink) = SQL_ANALYSIS_SINK
        .read()
        .ok()
        .and_then(|guard| *guard)
    {
        sink(
            prefs.semantic_diagnostics,
            prefs.predicate_pushdown,
            prefs.hover_scope,
        );
    }
}

/// 本域 slot 的标量写入处理；返回 `false` 表示拒绝（值形态不符 / 非本域 slot）。
pub(crate) fn apply_slot(
    slot: Slot,
    value: SettingValue,
    _window: Option<&mut gpui_kit::Window>,
    cx: &mut App,
) -> bool {
    let Some(on) = value.as_bool() else {
        return false;
    };
    match slot {
        Slot::SqlAnalysisSemanticDiagnostics => {
            SettingsService::set_sql_semantic_diagnostics(on, cx);
        }
        Slot::SqlAnalysisPredicatePushdown => {
            SettingsService::set_sql_predicate_pushdown(on, cx);
        }
        Slot::SqlAnalysisHoverScope => {
            SettingsService::set_sql_hover_scope(on, cx);
        }
        _ => return false,
    }
    true
}

impl SettingsService {
    /// 当前 SQL 分析三开关（快照）。
    pub fn sql_analysis(cx: &App) -> SqlAnalysis {
        cx.global::<Settings>().sql_analysis.clone()
    }

    /// 设置并持久化打字期语义诊断开关。
    pub fn set_sql_semantic_diagnostics(on: bool, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.sql_analysis.semantic_diagnostics = on;
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
        notify_sql_analysis(settings.sql_analysis);
    }

    /// 设置并持久化「谓词下推改写动作」开关。
    pub fn set_sql_predicate_pushdown(on: bool, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.sql_analysis.predicate_pushdown = on;
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
        notify_sql_analysis(settings.sql_analysis);
    }

    /// 设置并持久化「悬停显示来源表」开关。
    pub fn set_sql_hover_scope(on: bool, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.sql_analysis.hover_scope = on;
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
        notify_sql_analysis(settings.sql_analysis);
    }
}
