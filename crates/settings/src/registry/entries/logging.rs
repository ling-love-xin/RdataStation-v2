//! 日志设置项登记。
use crate::registry::{SettingEffect, SettingEntry, SettingKind, SettingSpec};

/// 本域全部登记项（`registry::registry()` 惰性合并，顺序 = 登记顺序）。
pub fn entries() -> Vec<SettingSpec> {
    vec![
        SettingSpec {
        key: "logging.min_level",
        section: "logging",
        section_label: "日志",
        group: "logging",
        group_label: None,
        label: "日志级别",
        hint: "低于该级别的记录不写文件也不落库（改完立即生效；排障时可临时调成 DEBUG）",
        hint_inline: true,
        kind: SettingKind::Enum(&[
        ("TRACE", "TRACE"),
        ("DEBUG", "DEBUG"),
        ("INFO", "INFO"),
        ("WARN", "WARN"),
        ("ERROR", "ERROR"),
        ]),
        presets: &[],
        default_json: "\"INFO\"",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "crates/app/src/main.rs（启动取值）+ engine::logging::reload_log_level（运行时，经装配层注册的 sink）",
        composite: false,
        disabled: false,
        },
    ]
}
