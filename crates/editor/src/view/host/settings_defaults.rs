//! 编辑器宿主的「设置 → 行为」读取辅助：一律“读不到就用出厂默认”，绝不 panic。
//!
//! 全部 `pub(crate)`：mod.rs 以 `pub(crate) use settings_defaults::*;` 转出，
//! 各子模块沿用既有的 `use super::*` 即可拿到同名函数，调用点零改动。

use super::*;

/// #48 读编辑器设置快照（面板构造 / render 调用）：测试环境未注册 Settings global
/// 时回退出厂默认（生产在 app 启动经 `SettingsService::init` 注册，不走此分支）。
pub(crate) fn editor_settings_or_default(cx: &App) -> settings::model::Editor {
    if cx.has_global::<settings::model::Settings>() {
        SettingsService::editor_defaults(cx)
    } else {
        settings::model::Editor::default()
    }
}

/// #48 读行号开关（同上：测试环境回退默认 true）。
pub(crate) fn editor_show_line_numbers_or_default(cx: &App) -> bool {
    if cx.has_global::<settings::model::Settings>() {
        SettingsService::show_line_numbers(cx)
    } else {
        true
    }
}

/// 【2026-10-27】读折叠开关（设置在设置页；测试环境回退默认 true）。
pub(crate) fn editor_show_folds_or_default(cx: &App) -> bool {
    if cx.has_global::<settings::model::Settings>() {
        SettingsService::show_folds(cx)
    } else {
        true
    }
}

/// 读「书签悬停显示说明」开关（设置在设置页；测试环境回退默认 true）。
///
/// 关掉时 `push_gutter_markers` 不下发说明文本，内核只画标记不弹提示。
pub(crate) fn editor_bookmark_hover_note_or_default(cx: &App) -> bool {
    if cx.has_global::<settings::model::Settings>() {
        SettingsService::bookmark_hover_note(cx)
    } else {
        true
    }
}

/// 读「常显书签列」开关（设置在设置页；测试环境回退默认 true）。
///
/// 开：代码编辑器零书签也留出一列（空行悬停出淡书签、点击即加）；关：回到
/// 「有书签才出列」。内核侧只影响布局，书签数据与快捷键入口不受影响。
pub(crate) fn editor_bookmark_column_or_default(cx: &App) -> bool {
    if cx.has_global::<settings::model::Settings>() {
        SettingsService::bookmark_column(cx)
    } else {
        true
    }
}

/// 【2026-10-28】读「基于缩进的折叠」开关（测试环境回退默认 true）。
pub(crate) fn editor_indent_folding_or_default(cx: &App) -> bool {
    if cx.has_global::<settings::model::Settings>() {
        SettingsService::indent_folding(cx)
    } else {
        true
    }
}

/// 【2026-10-28】读「自定义开块关键字」（逗号分隔字符串解析为 Vec<String>；测试环境用默认）。
pub(crate) fn editor_fold_open_keywords_or_default(cx: &App) -> Vec<String> {
    if cx.has_global::<settings::model::Settings>() {
        let s = SettingsService::fold_open_keywords(cx);
        s.split(',')
            .map(|kw| kw.trim().to_uppercase())
            .filter(|kw| !kw.is_empty())
            .collect()
    } else {
        vec![
            "CASE".to_string(),
            "BEGIN".to_string(),
            "IF".to_string(),
            "LOOP".to_string(),
            "WHILE".to_string(),
            "FOR".to_string(),
            "REPEAT".to_string(),
        ]
    }
}

/// 【2026-10-28】读「自定义闭块关键字」（逗号分隔字符串解析为 Vec<String>；测试环境用默认）。
pub(crate) fn editor_fold_close_keywords_or_default(cx: &App) -> Vec<String> {
    if cx.has_global::<settings::model::Settings>() {
        let s = SettingsService::fold_close_keywords(cx);
        s.split(',')
            .map(|kw| kw.trim().to_uppercase())
            .filter(|kw| !kw.is_empty())
            .collect()
    } else {
        vec!["END".to_string()]
    }
}

/// 【B3】读「自动换行」开关（测试环境回退默认 true，与内核一致）。
pub(crate) fn editor_soft_wrap_or_default(cx: &App) -> bool {
    if cx.has_global::<settings::model::Settings>() {
        SettingsService::soft_wrap(cx)
    } else {
        true
    }
}

/// 【B4】读「自动闭合括号」开关（测试环境回退默认 true，与内核一致）。
pub(crate) fn editor_auto_close_or_default(cx: &App) -> bool {
    if cx.has_global::<settings::model::Settings>() {
        SettingsService::auto_close(cx)
    } else {
        true
    }
}

/// 【B4】读「智能缩进」开关（测试环境回退默认 true，与内核一致）。
pub(crate) fn editor_smart_indent_or_default(cx: &App) -> bool {
    if cx.has_global::<settings::model::Settings>() {
        SettingsService::smart_indent(cx)
    } else {
        true
    }
}

/// 【B4】高亮类型 → 编辑器语言键（决定自动闭合 / 智能缩进的规则来源）
///
/// - Sql / 未识别扩展名 → `"sql"`（走 `language_config::install` 注册的 SQL 规则）；
/// - 脚本类型 → gpui-component 默认键（`ComponentLanguages` 对 python 有缩进规则，
///   json 有专属配对，其余落 fallback 默认 `()[]{}`+引号）；
/// - None（不上色：CSV / 分析模式）→ 不设语言（保持空键，内核默认）。
pub(crate) fn editor_language_key(kind: crate::view::highlight::HighlightKind) -> Option<&'static str> {
    use crate::mode::ScriptFileType;
    use crate::view::highlight::HighlightKind;
    match kind {
        HighlightKind::Sql => Some("sql"),
        HighlightKind::Script(ScriptFileType::Python) => Some("python"),
        HighlightKind::Script(ScriptFileType::R) => Some("r"),
        HighlightKind::Script(ScriptFileType::Rust) => Some("rust"),
        HighlightKind::Script(ScriptFileType::Markdown) => Some("markdown"),
        HighlightKind::Script(ScriptFileType::Json) => Some("json"),
        // CSV 走表格展示、Log 无专属编辑规则：留空键 = 内核默认
        HighlightKind::Script(ScriptFileType::Csv | ScriptFileType::Log) => None,
        HighlightKind::None => None,
    }
}

/// 【B3】读事实档载体偏好快照（分析 / 带类型导出在事件路径上取一次，随请求带入工作线程：
/// `QueryRunner` 是 `Send + Sync + 'static` 且方法签名无 `cx`，工作线程内不能自己读 global）。
/// 测试环境未注册 Settings global 时回退 `Auto`（生产在 app 启动经 `SettingsService::init` 注册）。
pub(crate) fn fact_storage_pref_or_default(cx: &App) -> ::shared::StoragePref {
    if cx.has_global::<settings::model::Settings>() {
        SettingsService::get(cx)
            .result_panel
            .fact_storage
            .to_shared()
    } else {
        ::shared::StoragePref::Auto
    }
}

/// 【B3】取当前选中结果集的事实档 `Arc` 快照（无事实档 = `None`，执行器走显示矩阵降级档）。
/// 克隆 `Arc` 即出 `Ref` 借用边界，不在 `results()` 引用存活期间回写 store。
pub(crate) fn active_fact_snapshot(
    shared: &EditorShared,
    document: &crate::model::DocumentId,
) -> Option<std::sync::Arc<::shared::FactFrame>> {
    shared
        .results()
        .active_fact(document)
        .and_then(|state| state.fact().map(std::sync::Arc::clone))
}

/// 【B4】当前选中结果集的槽位下标（无结果集回退 0）：分析 / 导出物化回执据此回挂原槽位。
pub(crate) fn active_set_index(shared: &EditorShared, document: &crate::model::DocumentId) -> usize {
    shared.results().active_index(document).unwrap_or(0)
}
