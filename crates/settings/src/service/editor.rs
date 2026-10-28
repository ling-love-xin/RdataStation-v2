//! 编辑器设置项的写入处理（`apply_slot`，由 `SettingsService::apply_by_key` 分发）与读写访问器。
use gpui_kit::App;

use crate::model::Settings;
use crate::persistence::persist;
use crate::registry::{SettingValue, Slot};
use crate::SettingsService;


/// 本域 slot 的标量写入处理；返回 `false` 表示拒绝（值形态不符 / 复合值 / 无法解析）。
pub(crate) fn apply_slot(
    slot: Slot,
    value: SettingValue,
    _window: Option<&mut gpui_kit::Window>,
    cx: &mut App,
) -> bool {
    match slot {
            Slot::EditorShowLineNumbers => {
                let Some(on) = value.as_bool() else {
                    return false;
                };
                SettingsService::set_show_line_numbers(on, cx);
            }
            Slot::EditorShowFolds => {
                let Some(on) = value.as_bool() else {
                    return false;
                };
                SettingsService::set_show_folds(on, cx);
            }
            Slot::EditorIndentFolds => {
                let Some(on) = value.as_bool() else {
                    return false;
                };
                SettingsService::set_indent_folding(on, cx);
            }
            Slot::EditorFoldOpenKeywords => {
                let Some(keywords) = value.as_text() else {
                    return false;
                };
                SettingsService::set_fold_open_keywords(keywords.to_string(), cx);
            }
            Slot::EditorFoldCloseKeywords => {
                let Some(keywords) = value.as_text() else {
                    return false;
                };
                SettingsService::set_fold_close_keywords(keywords.to_string(), cx);
            }
            Slot::EditorSoftWrap => {
                let Some(on) = value.as_bool() else {
                    return false;
                };
                SettingsService::set_soft_wrap(on, cx);
            }
            Slot::EditorAutoClose => {
                let Some(on) = value.as_bool() else {
                    return false;
                };
                SettingsService::set_auto_close(on, cx);
            }
            Slot::EditorSmartIndent => {
                let Some(on) = value.as_bool() else {
                    return false;
                };
                SettingsService::set_smart_indent(on, cx);
            }
            Slot::EditorInlineCompletion => {
                let Some(on) = value.as_bool() else {
                    return false;
                };
                SettingsService::set_inline_completion(on, cx);
            }
            Slot::EditorDefaultTabSize => {
                let Some(n) = value.as_number() else {
                    return false;
                };
                if !(1.0..=16.0).contains(&n) {
                    return false;
                }
                SettingsService::set_default_tab_size(n as i64, cx);
            }
            Slot::EditorDefaultHardTabs => {
                let Some(on) = value.as_bool() else {
                    return false;
                };
                SettingsService::set_default_hard_tabs(on, cx);
            }
            Slot::EditorDefaultLineEnding => {
                let Some(v) = value.as_text() else {
                    return false;
                };
                // 只认 LF / CRLF（与「重设 ▾」同一套换行档；手改坏时拒绝，不猜）。
                if !(v == "LF" || v == "CRLF") {
                    return false;
                }
                SettingsService::set_default_line_ending(&v, cx);
            }
            Slot::EditorDefaultEncoding => {
                let Some(v) = value.as_text() else {
                    return false;
                };
                // 只认编码五档（与「重设 ▾」同一套档；手改坏时拒绝，不猜）。
                if !(v == "UTF-8"
                    || v == "UTF-8-BOM"
                    || v == "GBK"
                    || v == "UTF-16LE"
                    || v == "UTF-16BE")
                {
                    return false;
                }
                SettingsService::set_default_encoding(&v, cx);
            }
            Slot::EditorFormatKeywordCase => {
                let Some(v) = value.as_text() else {
                    return false;
                };
                // 只认三档（preserve / upper / lower；手改坏时拒绝，不猜）。
                if !(v == "preserve" || v == "upper" || v == "lower") {
                    return false;
                }
                SettingsService::set_format_keyword_case(&v, cx);
            }
            Slot::EditorFormatIndentWidth => {
                let Some(n) = value.as_number() else {
                    return false;
                };
                if !(2.0..=8.0).contains(&n) {
                    return false;
                }
                SettingsService::set_format_indent_width(n as i64, cx);
            }
            Slot::EditorFormatOnSave => {
                let Some(on) = value.as_bool() else {
                    return false;
                };
                SettingsService::set_format_on_save(on, cx);
            }
        _ => return false,
    }
    true
}

impl SettingsService {
    /// 读取当前主题模式。
    /// 当前「显示行号」开关（`EditorHostPanel::render` 渲染同步点读取）。
    pub fn show_line_numbers(cx: &App) -> bool {
        cx.global::<Settings>().editor.show_line_numbers
    }
    /// 编辑器默认值快照（新建文档 / 无会话文档应用；已开文档走「重设 ▾」）。
    pub fn editor_defaults(cx: &App) -> crate::model::Editor {
        cx.global::<Settings>().editor.clone()
    }
    /// 行号开关：更新 global → 持久化 → 立即重绘所有窗口。
    ///
    /// 即时生效靠 `refresh_windows`：下一帧 `EditorHostPanel::render` 读到新值，
    /// 与面板内缓存比较后同步到内核 `set_line_number`（值相同不通知，无重绘循环）。
    pub fn set_show_line_numbers(on: bool, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.editor.show_line_numbers = on;
        }
        persist(&cx.global::<Settings>().clone());
        cx.refresh_windows();
    }
    /// 读取「代码折叠」开关（2026-10-27 起设在设置页；`EditorHostPanel::render` 渲染
    /// 权威同步点读取 → 同步 `shared.show_folds` → `sync_fold` 重跑）。
    pub fn show_folds(cx: &App) -> bool {
        cx.global::<Settings>().editor.show_folds
    }
    /// 折叠开关：更新 global → 持久化 → 立即重绘所有窗口。
    ///
    /// 即时生效靠 `refresh_windows`：下一帧 `EditorHostPanel::render` 读到新值，与
    /// `shared.show_folds` 比较后重跑 `sync_fold`（install / set_folding(false)）。
    pub fn set_show_folds(on: bool, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.editor.show_folds = on;
        }
        persist(&cx.global::<Settings>().clone());
        cx.refresh_windows();
    }
    /// 读取「基于缩进的折叠」开关（设在设置页；`EditorHostPanel::render` 渲染权威同步点
    /// 读取 → 重新计算折叠候选）。
    pub fn indent_folding(cx: &App) -> bool {
        cx.global::<Settings>().editor.indent_folding
    }
    /// 基于缩进的折叠开关：更新 global → 持久化 → 立即重绘所有窗口。
    ///
    /// 即时生效靠 `refresh_windows`：下一帧 `EditorHostPanel::render` 读到新值，
    /// 重新计算折叠候选。
    pub fn set_indent_folding(on: bool, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.editor.indent_folding = on;
        }
        persist(&cx.global::<Settings>().clone());
        cx.refresh_windows();
    }
    /// 读取自定义开块关键字列表（折叠用；逗号分隔）。
    pub fn fold_open_keywords(cx: &App) -> String {
        cx.global::<Settings>().editor.fold_open_keywords.clone()
    }
    /// 自定义开块关键字列表：更新 global → 持久化 → 立即重绘所有窗口。
    pub fn set_fold_open_keywords(keywords: String, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.editor.fold_open_keywords = keywords;
        }
        persist(&cx.global::<Settings>().clone());
        cx.refresh_windows();
    }
    /// 读取自定义闭块关键字列表（折叠用；逗号分隔）。
    pub fn fold_close_keywords(cx: &App) -> String {
        cx.global::<Settings>().editor.fold_close_keywords.clone()
    }
    /// 自定义闭块关键字列表：更新 global → 持久化 → 立即重绘所有窗口。
    pub fn set_fold_close_keywords(keywords: String, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.editor.fold_close_keywords = keywords;
        }
        persist(&cx.global::<Settings>().clone());
        cx.refresh_windows();
    }
    /// 读取「自动换行」开关（设在设置页；`EditorHostPanel::render` 渲染权威同步点
    /// 读取 → 与面板缓存比较后同步内核 `set_soft_wrap`）。
    pub fn soft_wrap(cx: &App) -> bool {
        cx.global::<Settings>().editor.soft_wrap
    }
    /// 自动换行开关：更新 global → 持久化 → 立即重绘所有窗口。
    ///
    /// 即时生效靠 `refresh_windows`：下一帧 `EditorHostPanel::render` 读到新值，
    /// 与面板缓存比较后同步内核 `set_soft_wrap`（值相同不通知，无重绘循环）。
    pub fn set_soft_wrap(on: bool, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.editor.soft_wrap = on;
        }
        persist(&cx.global::<Settings>().clone());
        cx.refresh_windows();
    }
    /// 读取「自动闭合括号」开关（设在设置页；`EditorHostPanel::render` 渲染权威同步点
    /// 读取 → 与面板缓存比较后同步内核 `set_auto_close`）。
    pub fn auto_close(cx: &App) -> bool {
        cx.global::<Settings>().editor.auto_close
    }
    /// 自动闭合括号开关：更新 global → 持久化 → 立即重绘所有窗口。
    ///
    /// 即时生效靠 `refresh_windows`：下一帧 `EditorHostPanel::render` 读到新值，
    /// 与面板缓存比较后同步内核 `set_auto_close`（值相同不通知，无重绘循环）。
    pub fn set_auto_close(on: bool, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.editor.auto_close = on;
        }
        persist(&cx.global::<Settings>().clone());
        cx.refresh_windows();
    }
    /// 读取「智能缩进」开关（设在设置页；`EditorHostPanel::render` 渲染权威同步点
    /// 读取 → 与面板缓存比较后同步内核 `set_smart_indent`）。
    pub fn smart_indent(cx: &App) -> bool {
        cx.global::<Settings>().editor.smart_indent
    }
    /// 智能缩进开关：更新 global → 持久化 → 立即重绘所有窗口。
    ///
    /// 即时生效靠 `refresh_windows`：下一帧 `EditorHostPanel::render` 读到新值，
    /// 与面板缓存比较后同步内核 `set_smart_indent`（值相同不通知，无重绘循环）。
    pub fn set_smart_indent(on: bool, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.editor.smart_indent = on;
        }
        persist(&cx.global::<Settings>().clone());
        cx.refresh_windows();
    }
    /// 读取「AI 补全」开关（**当前版本置灰**：设置页禁用，值恒为默认关；
    /// 未来放开 registry disabled 后由编辑器 render 同步到 AI 源）。
    pub fn inline_completion(cx: &App) -> bool {
        cx.global::<Settings>().editor.inline_completion
    }
    /// AI 补全开关：仅持久化（置灰期不接内核——内核 inline_completion 槽位
    /// 未挂 AI 源；放开 disabled 后此写入路径复用，无需改设置侧）。
    pub fn set_inline_completion(on: bool, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.editor.inline_completion = on;
        }
        persist(&cx.global::<Settings>().clone());
    }
    /// 新文档默认缩进宽度（1–16；仅影响新建文档，已开文档不动）。
    pub fn set_default_tab_size(n: i64, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.editor.default_tab_size = n.clamp(1, 16);
        }
        persist(&cx.global::<Settings>().clone());
    }
    /// 新文档默认是否用制表符缩进。
    pub fn set_default_hard_tabs(on: bool, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.editor.default_hard_tabs = on;
        }
        persist(&cx.global::<Settings>().clone());
    }
    /// 新文档默认换行（`"LF"` / `"CRLF"`）。
    pub fn set_default_line_ending(v: &str, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.editor.default_line_ending = v.to_string();
        }
        persist(&cx.global::<Settings>().clone());
    }
    /// 新文档默认编码（`"UTF-8"` / `"UTF-8-BOM"` / `"GBK"` / `"UTF-16LE"` / `"UTF-16BE"`）。
    pub fn set_default_encoding(v: &str, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.editor.default_encoding = v.to_string();
        }
        persist(&cx.global::<Settings>().clone());
    }
    /// SQL 格式化关键字大小写（`"preserve"` / `"upper"` / `"lower"`）。
    pub fn format_keyword_case(cx: &App) -> String {
        cx.global::<Settings>().editor.format_keyword_case.clone()
    }
    /// 设置 SQL 格式化关键字大小写：更新 global → 持久化。
    pub fn set_format_keyword_case(v: &str, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.editor.format_keyword_case = v.to_string();
        }
        persist(&cx.global::<Settings>().clone());
    }
    /// SQL 格式化缩进宽度（空格数；2 = sqlglot 原生）。
    pub fn format_indent_width(cx: &App) -> i64 {
        cx.global::<Settings>().editor.format_indent_width
    }
    /// 设置 SQL 格式化缩进宽度：更新 global → 持久化。
    pub fn set_format_indent_width(v: i64, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.editor.format_indent_width = v;
        }
        persist(&cx.global::<Settings>().clone());
    }
    /// 保存时自动格式化 SQL（默认关）。
    pub fn format_on_save(cx: &App) -> bool {
        cx.global::<Settings>().editor.format_on_save
    }
    /// 保存时自动格式化开关：更新 global → 持久化。
    pub fn set_format_on_save(on: bool, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.editor.format_on_save = on;
        }
        persist(&cx.global::<Settings>().clone());
    }
}
