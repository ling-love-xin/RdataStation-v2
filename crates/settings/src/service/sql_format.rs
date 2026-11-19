//! SQL 格式化设置项的写入处理（`apply_slot`，由 `SettingsService::apply_by_key` 分发）
//! 与读写访问器、设置页预览出口。
//!
//! 消费模式是**拉**：编辑器在格式化时读当前值注入 `FormatOptions`
//! （`crates/editor/src/view/host/format.rs::format_options`），无需 sink 推送。
//! 设置页的实时预览走 [`install_sql_format_preview`]：本 crate 不依赖 engine
//! （依赖方向），格式化计算由装配层（`crates/app/src/main.rs`）注入——设置页改值
//! → global 更新 → 页面重渲 → 预览回调拿新值重算，天然实时。
use std::sync::RwLock;

use gpui_kit::App;

use crate::SettingsService;
use crate::model::{Settings, SqlFormat};
use crate::persistence::persist;
use crate::registry::{SettingValue, Slot};

/// 预览出口（设置页「SQL 格式化」节的实时预览；由装配层注册一次）。
/// 入参是当前格式化设置，返回按该设置格式化后的示例 SQL。
type PreviewFn = fn(&SqlFormat) -> String;

static SQL_FORMAT_PREVIEW: RwLock<Option<PreviewFn>> = RwLock::new(None);

/// 注册预览出口（装配层调用一次；重复调用覆盖；未注册时设置页不渲染预览卡片）。
pub fn install_sql_format_preview(preview: PreviewFn) {
    if let Ok(mut guard) = SQL_FORMAT_PREVIEW.write() {
        *guard = Some(preview);
    }
}

/// 取预览结果（设置页渲染时调用；未注册返回 `None`）。
pub(crate) fn sql_format_preview(prefs: &SqlFormat) -> Option<String> {
    SQL_FORMAT_PREVIEW
        .read()
        .ok()
        .and_then(|guard| *guard)
        .map(|preview| preview(prefs))
}

/// 本域 slot 的标量写入处理；返回 `false` 表示拒绝（值形态不符 / 非本域 slot）。
pub(crate) fn apply_slot(
    slot: Slot,
    value: SettingValue,
    _window: Option<&mut gpui_kit::Window>,
    cx: &mut App,
) -> bool {
    match slot {
        Slot::SqlFormatKeywordCase => {
            let Some(v) = value.as_text() else {
                return false;
            };
            // 只认三档（preserve / upper / lower；手改坏时拒绝，不猜）。
            if !(v == "preserve" || v == "upper" || v == "lower") {
                return false;
            }
            SettingsService::set_sql_format_keyword_case(v, cx);
        }
        Slot::SqlFormatIndentChar => {
            let Some(v) = value.as_text() else {
                return false;
            };
            if !(v == "space" || v == "tab") {
                return false;
            }
            SettingsService::set_sql_format_indent_char(v, cx);
        }
        Slot::SqlFormatIndentWidth => {
            let Some(n) = value.as_number() else {
                return false;
            };
            if !(2.0..=8.0).contains(&n) {
                return false;
            }
            SettingsService::set_sql_format_indent_width(n as i64, cx);
        }
        Slot::SqlFormatCommaStyle => {
            let Some(v) = value.as_text() else {
                return false;
            };
            if !(v == "trailing" || v == "leading") {
                return false;
            }
            SettingsService::set_sql_format_comma_style(v, cx);
        }
        Slot::SqlFormatBlankLines => {
            let Some(n) = value.as_number() else {
                return false;
            };
            if !(0.0..=2.0).contains(&n) {
                return false;
            }
            SettingsService::set_sql_format_blank_lines(n as i64, cx);
        }
        Slot::SqlFormatOnSave => {
            let Some(on) = value.as_bool() else {
                return false;
            };
            SettingsService::set_sql_format_on_save(on, cx);
        }
        _ => return false,
    }
    true
}

impl SettingsService {
    /// 当前 SQL 格式化六旋钮（快照；编辑器格式化与设置页预览共用）。
    pub fn sql_format(cx: &App) -> SqlFormat {
        cx.global::<Settings>().sql_format.clone()
    }

    /// 设置并持久化关键字大小写。
    pub fn set_sql_format_keyword_case(v: &str, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.sql_format.keyword_case = v.to_string();
        }
        persist(&cx.global::<Settings>().clone());
    }

    /// 设置并持久化缩进字符。
    pub fn set_sql_format_indent_char(v: &str, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.sql_format.indent_char = v.to_string();
        }
        persist(&cx.global::<Settings>().clone());
    }

    /// 设置并持久化缩进宽度。
    pub fn set_sql_format_indent_width(v: i64, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.sql_format.indent_width = v;
        }
        persist(&cx.global::<Settings>().clone());
    }

    /// 设置并持久化逗号风格。
    pub fn set_sql_format_comma_style(v: &str, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.sql_format.comma_style = v.to_string();
        }
        persist(&cx.global::<Settings>().clone());
    }

    /// 设置并持久化语句间空行数。
    pub fn set_sql_format_blank_lines(v: i64, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.sql_format.blank_lines_between = v;
        }
        persist(&cx.global::<Settings>().clone());
    }

    /// 设置并持久化「保存时自动格式化」开关。
    pub fn set_sql_format_on_save(on: bool, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.sql_format.format_on_save = on;
        }
        persist(&cx.global::<Settings>().clone());
    }
}
