//! rds-settings — 设置项登记表（**准入的代码侧权威**）。
//!
//! 为什么要有这张表：设置项最容易长成"有字段、有界面行，但没有消费方"的装饰
//! （2026-09-16 审计发现 7 个这样的字段，见架构文档 §7.2）。把登记信息放进代码、
//! 用契约测试锁住三条不变式，装饰项就进不来：
//!
//! 1. **登记项必须在模型里**（每个 key 都能在 `Settings::default()` 序列化结果里找到）；
//! 2. **模型里的叶子必须登记**（新增字段不登记 → 测试失败，这就是"准入"）；
//! 3. **默认值一致**（登记表写的默认值 == 模型默认值，保证"恢复默认"有唯一目标）。
//!
//! 文档侧：`docs/architecture/settings/settings-architecture.md` §6 是同一张表的可读版，
//! §7.1 的准入五条解释每条字段为什么必须存在。**改动顺序**：先改本表，再改 `model.rs`
//! 与消费方，最后同步文档表。
//!
//! 范围：本表只收录**已落地**的项；待接线项（如 M6 的 `resources.keep_versions`）在
//! 落地前不进本表——否则第 1 条不变式不成立。

// 仅契约测试需要（生产代码不按路径取值，直接读登记信息与强类型访问器）。
#[cfg(test)]
use serde_json::Value;

/// 设置项的取值形状（决定页面用什么控件，也是"值语义"的唯一声明）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingKind {
    /// 布尔开关（页面用 `Switch`）。
    Bool,
    /// 布尔但两端有明确名字（页面用分段控件；`on` 对应 `true`）。
    BoolPair {
        on: &'static str,
        off: &'static str,
    },
    /// 枚举（页面用分段控件 ≤ 4 项，多于此用下拉）；元素为 `(落盘值, 显示名)`。
    Enum(&'static [(&'static str, &'static str)]),
    /// 数值（页面用输入框 + 校验）。
    Number,
    /// 复合值：多个子键打成一个登记项（子键不单独登记）。
    Composite,
}

/// 生效方式（页面的说明行必须讲清楚，禁止"改了不知道生效没"）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingEffect {
    /// 改完立刻反映到界面（需要主动通知重绘）。
    Immediate,
    /// 下一次操作才用到（建连 / 排序 / 归档），无需通知。
    NextUse,
    /// 需要重载进程（页面必须标注 + 提供重启入口）。
    Restart,
}

/// 入口位置：登记是义务，**上不上设置页是判断**（登记表 ≠ 页面清单）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingEntry {
    /// 只上设置页。
    Page,
    /// 只在操作现场（拖拽、局部菜单），上页反而添乱。
    Module,
    /// 两处都有：必须是同一 `SettingsService` 写入路径，禁止各存一份。
    Both,
}

/// 一条设置项的登记信息。
#[derive(Debug, Clone, Copy)]
pub struct SettingSpec {
    /// 稳定点分命名，**必须与 `Settings` 的 JSON 字段路径一致**。
    pub key: &'static str,
    /// 所属节（页面左侧分节导航的 id）。
    pub section: &'static str,
    /// 节的显示名。
    pub section_label: &'static str,
    /// 行标签（页面上那一行叫什么）。
    pub label: &'static str,
    /// 行说明（弱化小字；也承载"生效方式"这类边界说明）。
    pub hint: &'static str,
    pub kind: SettingKind,
    /// 默认值（JSON 字面量）；与 `model.rs` 的 `Default` 逐项比对。
    pub default_json: &'static str,
    pub effect: SettingEffect,
    pub entry: SettingEntry,
    /// 消费方（`文件::符号`）——准入第 1 条要求能点到具体符号。
    pub consumer: &'static str,
    /// 复合值：契约测试不展开它的子键。
    pub composite: bool,
}

/// 设置项登记表（已落地项；顺序 = 页面上的顺序）。
///
/// 节顺序：外观 → 数据源导航 → 连接默认值 → 项目。
pub const REGISTRY: &[SettingSpec] = &[
    SettingSpec {
        key: "appearance.theme_mode",
        section: "appearance",
        section_label: "外观",
        label: "主题模式",
        hint: "立即切换明暗配色并持久化",
        kind: SettingKind::Enum(&[("light", "浅色"), ("dark", "深色")]),
        default_json: "\"light\"",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Both,
        consumer: "app/src/main.rs（启动应用）+ SettingsService::set_theme_mode",
        composite: false,
    },
    SettingSpec {
        key: "navigator.source_short_code",
        section: "navigator",
        section_label: "数据源导航",
        label: "来源标识",
        hint: "连接行右端显示短码或文字（同一列，宽度随模式切换）",
        kind: SettingKind::BoolPair {
            on: "短码 P/G/GP",
            off: "文字（项目 / 全局 / 共享）",
        },
        default_json: "true",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "workbench/src/panels.rs::render_connection_row",
        composite: false,
    },
    SettingSpec {
        key: "navigator.show_tags",
        section: "navigator",
        section_label: "数据源导航",
        label: "显示标签",
        hint: "连接行名称下方最多 2 个标签 chip，其余折叠为 +N",
        kind: SettingKind::Bool,
        default_json: "false",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Both,
        consumer: "workbench/src/panels.rs::render_connection_row",
        composite: false,
    },
    SettingSpec {
        key: "navigator.show_scope",
        section: "navigator",
        section_label: "数据源导航",
        label: "显示归属域",
        hint: "行尾右对齐的归属域列（便于扫视的一端对齐列）",
        kind: SettingKind::Bool,
        default_json: "true",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Both,
        consumer: "workbench/src/panels.rs::render_connection_row",
        composite: false,
    },
    SettingSpec {
        key: "navigator.property_panel_width",
        section: "navigator",
        section_label: "数据源导航",
        label: "属性面板宽度",
        hint: "拖拽右侧属性面板分隔条时记忆（关闭面板时落盘）",
        kind: SettingKind::Number,
        default_json: "24.5",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Module,
        consumer: "workbench/src/panels.rs::EditorPanel::render_property_panel",
        composite: false,
    },
    SettingSpec {
        key: "navigator.filters",
        section: "navigator",
        section_label: "数据源导航",
        label: "facet 筛选",
        hint: "归属域 / 类型 / 驱动 / 标签筛选（导航面板内 chips 与「筛选 ▾」）",
        kind: SettingKind::Composite,
        default_json: "{\"source\":null,\"db_type\":null,\"driver\":null,\"tag\":null}",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Module,
        consumer: "workbench/src/panels.rs::{SidebarPanel::new, write_nav_filters}",
        composite: true,
    },
    SettingSpec {
        key: "connection_defaults.connect_timeout_ms",
        section: "connection",
        section_label: "连接默认值",
        label: "建连超时",
        hint: "超时判定本次尝试失败并自动重试一次；对之后新建的连接生效",
        kind: SettingKind::Number,
        default_json: "15000",
        effect: SettingEffect::NextUse,
        entry: SettingEntry::Page,
        consumer: "workbench/src/services/connection_service.rs::connect_with_type",
        composite: false,
    },
    SettingSpec {
        key: "connection_defaults.lan_disable_tls",
        section: "connection",
        section_label: "连接默认值",
        label: "LAN 直连 TLS",
        hint: "LAN / 本机直连时显式关闭 TLS（规避 sqlx 默认 prefer 的握手卡顿）；公网不受影响",
        kind: SettingKind::BoolPair {
            on: "关 TLS",
            off: "保留 TLS",
        },
        default_json: "true",
        effect: SettingEffect::NextUse,
        entry: SettingEntry::Page,
        consumer: "workbench/src/services/connection_service.rs::apply_lan_tls_default",
        composite: false,
    },
    SettingSpec {
        key: "projects.sort_mode",
        section: "projects",
        section_label: "项目",
        label: "列表排序",
        hint: "项目选择器的排序方式（选择器内也有循环按钮）",
        kind: SettingKind::Enum(&[
            ("last_opened", "最近打开"),
            ("name", "名称"),
            ("created", "创建时间"),
        ]),
        default_json: "\"last_opened\"",
        effect: SettingEffect::NextUse,
        entry: SettingEntry::Both,
        consumer: "workbench/src/view.rs::WorkbenchView::new（读）+ components/project_host.rs（写）",
        composite: false,
    },
];

/// 按 key 查登记项。
pub fn spec(key: &str) -> Option<&'static SettingSpec> {
    REGISTRY.iter().find(|s| s.key == key)
}

/// 节清单（按登记顺序去重）——页面的分节导航直接用它，顺序不由视图另定。
pub fn sections() -> Vec<(&'static str, &'static str)> {
    let mut out: Vec<(&'static str, &'static str)> = Vec::new();
    for s in REGISTRY {
        if !out.iter().any(|(id, _)| *id == s.section) {
            out.push((s.section, s.section_label));
        }
    }
    out
}

/// 上设置页的行（`entry` 非 `Module`）；页面只渲染这些。
pub fn page_rows() -> impl Iterator<Item = &'static SettingSpec> {
    REGISTRY.iter().filter(|s| s.entry != SettingEntry::Module)
}

// ===== 契约测试：登记表 ↔ 模型 的三条不变式 =====
//
// 下面三个辅助函数只服务于契约测试（生产代码不需要按路径取值 ——
// 页面将来直接读 `registry` 的登记信息 + `SettingsService` 的强类型访问器）。

/// 取默认值的 JSON 表示（测试与将来文档生成的共同基准）。
#[cfg(test)]
fn default_value() -> Value {
    serde_json::to_value(crate::model::Settings::default()).expect("Settings 可序列化")
}

/// 按点分路径取值（复合键取到对象本身）。
#[cfg(test)]
fn lookup<'a>(root: &'a Value, key: &str) -> Option<&'a Value> {
    let mut cur = root;
    for seg in key.split('.') {
        cur = cur.get(seg)?;
    }
    Some(cur)
}

/// 收集默认值的全部叶子路径；遇到登记的复合键就整体收起，不展开子键。
#[cfg(test)]
fn leaf_paths(root: &Value) -> Vec<String> {
    fn walk(prefix: &str, value: &Value, out: &mut Vec<String>) {
        // 复合键（如 `navigator.filters`）整体算一个叶子：它的子键不单独登记。
        if !prefix.is_empty() && spec(prefix).is_some_and(|s| s.composite) {
            out.push(prefix.to_string());
            return;
        }
        match value {
            Value::Object(map) => {
                for (k, v) in map {
                    let child = if prefix.is_empty() {
                        k.clone()
                    } else {
                        format!("{prefix}.{k}")
                    };
                    walk(&child, v, out);
                }
            }
            _ => {
                if !prefix.is_empty() {
                    out.push(prefix.to_string());
                }
            }
        }
    }
    let mut out = Vec::new();
    walk("", root, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    // 安全模式：逐项列举需要的东西，不通配导入（见 gpui-kit-dev skill「窗口测试」）
    use serde_json::from_str;

    /// 不变式 1：登记项必须能落到模型里（防止"登记了不存在的字段"）。
    #[test]
    fn every_registered_key_exists_in_the_model() {
        let default = default_value();
        for s in REGISTRY {
            assert!(
                lookup(&default, s.key).is_some(),
                "登记项 `{}` 在 Settings 里找不到对应字段",
                s.key
            );
        }
    }

    /// 不变式 2：模型里的每个叶子都必须登记（**这条是准入的护栏**：新字段不登记就红）。
    #[test]
    fn every_model_leaf_is_registered() {
        let default = default_value();
        let mut leaves = leaf_paths(&default);
        leaves.sort();
        let mut keys: Vec<String> = REGISTRY.iter().map(|s| s.key.to_string()).collect();
        keys.sort();
        assert_eq!(
            leaves,
            keys,
            "模型叶子与登记表不一致。多出的叶子（未登记的新字段）= {:?}；多出的登记项 = {:?}",
            leaves.iter().filter(|p| !keys.contains(p)).collect::<Vec<_>>(),
            keys.iter().filter(|p| !leaves.contains(p)).collect::<Vec<_>>()
        );
    }

    /// 不变式 3：登记表写的默认值 == 模型默认值（"恢复默认"才有唯一目标）。
    #[test]
    fn registered_defaults_match_the_model() {
        let default = default_value();
        for s in REGISTRY {
            let actual = lookup(&default, s.key).expect("不变式 1 已保证存在");
            let expected: Value = from_str(s.default_json).expect("默认值是合法 JSON");
            assert_eq!(
                actual, &expected,
                "`{}` 的登记默认值 {} 与模型默认值 {} 不一致",
                s.key, s.default_json, actual
            );
        }
    }

    /// 枚举项：默认值必须在候选值里（否则页面渲染不出选中的那一项）。
    #[test]
    fn enum_defaults_are_among_the_options() {
        let default = default_value();
        for s in REGISTRY {
            if let SettingKind::Enum(options) = s.kind {
                let actual = lookup(&default, s.key).expect("存在");
                let value = actual.as_str().unwrap_or_else(|| {
                    panic!("`{}` 登记为枚举，模型默认值却不是字符串", s.key)
                });
                assert!(
                    options.iter().any(|(v, _)| *v == value),
                    "`{}` 的默认值 `{}` 不在候选 {:?} 里",
                    s.key,
                    value,
                    options.iter().map(|(v, _)| *v).collect::<Vec<_>>()
                );
                // 显示名不得为空（页面直接用这两列渲染）
                for (v, label) in options.iter() {
                    assert!(!label.is_empty(), "`{}` 的选项 `{}` 缺显示名", s.key, v);
                }
            }
        }
    }

    /// 两态项：默认值必须是布尔，且两端文案非空。
    #[test]
    fn bool_pair_specs_are_complete() {
        let default = default_value();
        for s in REGISTRY {
            if let SettingKind::BoolPair { on, off } = s.kind {
                let actual = lookup(&default, s.key).expect("存在");
                assert!(
                    actual.is_boolean(),
                    "`{}` 登记为两态布尔，模型默认值不是布尔",
                    s.key
                );
                assert!(!on.is_empty() && !off.is_empty(), "`{}` 缺两态文案", s.key);
                assert_ne!(on, off, "`{}` 的两态文案相同，用户看不出区别", s.key);
            }
        }
    }

    /// 登记表自身的卫生：key 唯一、命名规范、必填字段非空、composite 与形态自洽。
    #[test]
    fn registry_is_wellformed() {
        let mut seen = Vec::new();
        for s in REGISTRY {
            assert!(!seen.contains(&s.key), "key `{}` 重复登记", s.key);
            seen.push(s.key);

            for seg in s.key.split('.') {
                assert!(!seg.is_empty(), "key `{}` 有空段", s.key);
                assert!(
                    seg.chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
                    "key `{}` 的段 `{}` 应为小写 snake_case",
                    s.key,
                    seg
                );
            }
            assert!(!s.section.is_empty(), "`{}` 缺节 id", s.key);
            assert!(!s.section_label.is_empty(), "`{}` 缺节显示名", s.key);
            assert!(!s.label.is_empty(), "`{}` 缺行标签", s.key);
            assert!(!s.hint.is_empty(), "`{}` 缺行说明（生效方式要能读出来）", s.key);
            assert!(!s.consumer.is_empty(), "`{}` 缺消费方（准入第 1 条）", s.key);

            let expected: Value = from_str(s.default_json).expect("默认值是合法 JSON");
            assert_eq!(
                s.composite,
                expected.is_object(),
                "`{}` 的 composite 标记与默认值形态不符",
                s.key
            );
            assert_eq!(
                s.kind == SettingKind::Composite,
                s.composite,
                "`{}` 的 kind 与 composite 标记不一致",
                s.key
            );
        }
    }

    /// 同一节的行必须在登记表里相邻（去重后节序 = 页面导航顺序，避免视图另排一遍）。
    #[test]
    fn sections_are_contiguous() {
        let mut seen: Vec<&str> = Vec::new();
        let mut current = None;
        for s in REGISTRY {
            if current != Some(s.section) {
                assert!(
                    !seen.contains(&s.section),
                    "节 `{}` 在登记表里被打断（页面导航顺序会前后跳）",
                    s.section
                );
                seen.push(s.section);
                current = Some(s.section);
            }
        }
        assert_eq!(sections().len(), seen.len(), "节清单与登记表不一致");
        let module_only = REGISTRY
            .iter()
            .filter(|s| s.entry == SettingEntry::Module)
            .count();
        assert_eq!(
            page_rows().count(),
            REGISTRY.len() - module_only,
            "page_rows 与 entry 分类不一致（页面会多渲染或少渲染行）"
        );
        assert!(page_rows().next().is_some(), "页面至少要有一行");
    }
}
