//! connection设置项登记。
use crate::registry::{SettingEffect, SettingEntry, SettingKind, SettingSpec};

/// 本域全部登记项（`registry::registry()` 惰性合并，顺序 = 登记顺序）。
pub fn entries() -> Vec<SettingSpec> {
    vec![
        SettingSpec {
            key: "connection_defaults.connect_timeout_ms",
            section: "connection",
            section_label: "连接默认值",
            group: "connection",
            group_label: None,
            label: "建连超时",
            hint: "超时判定本次尝试失败并自动重试一次；对之后新建的连接生效",
            hint_inline: true,
            kind: SettingKind::Number,
            presets: &[
                (5_000, "5s"),
                (15_000, "15s"),
                (30_000, "30s"),
                (60_000, "60s"),
            ],
            default_json: "15000",
            effect: SettingEffect::NextUse,
            entry: SettingEntry::Page,
            consumer: "workbench/src/services/connection_service.rs::connect_with_type",
            composite: false,
            disabled: false,
        },
        SettingSpec {
            key: "connection_defaults.lan_disable_tls",
            section: "connection",
            section_label: "连接默认值",
            group: "connection",
            group_label: None,
            label: "LAN 直连 TLS",
            hint: "LAN / 本机直连时显式关闭 TLS（sqlx 与 tokio-postgres 默认都先试 TLS，握手会卡）；公网不受影响，mysql_async（MySQL Official）本就不加密、不受此开关影响",
            hint_inline: true,
            kind: SettingKind::BoolPair {
                on: "关 TLS",
                off: "保留 TLS",
            },
            presets: &[],
            default_json: "true",
            effect: SettingEffect::NextUse,
            entry: SettingEntry::Page,
            consumer:
                "connection_dialog/src/services/connection_service/runtime.rs::apply_lan_tls_default",
            composite: false,
            disabled: false,
        },
    ]
}
