//! 插件贡献点注册表（P4 第一刀）。
//!
//! 清单里的 `[[contributes.commands]]` / `[[contributes.panels]]` /
//! `[[contributes.settings]]` 此前只做反序列化，没有任何宿主侧消费者：装两个插件
//! 都声明同一个命令 id，也不会有人发现。这里先把「静态清单 → 可查询目录」这层
//! 建起来：
//!
//! - id 与插件 id 同口径（字母 / 数字 / `.` / `_` / `-`），设置键同口径；
//! - 命令、面板、设置三张表**各自**全局唯一，跨插件撞车当场报错（附双方插件 id）；
//! - 面板 `location` 只认 `left` / `right` / `center`；
//! - 设置 `type` 只认 JSON 基础类型，且 `default` 必须与 `type` 一致。
//!
//! **本刀不接宿主 UI**：Quick Open / 左右 Dock / 设置页还没有消费方，命令也还没有
//! 执行入口。注册表只是让「清单写错」在安装 / 加载期就能被拒绝，并给下一步的
//! `CommandRegistry` / `PanelRegistry` 接线留一个唯一权威。

use std::collections::HashMap;

use shared::error::{CommonError, CoreError};

use crate::manifest::{ContributesCommand, ContributesPanel, ContributesSetting, PluginManifest};

/// 贡献点类别（三张表各自命名空间，同名字符串互不冲突）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContributionKind {
    Command,
    Panel,
    Setting,
}

impl ContributionKind {
    /// 错误文案里的中文类别名。
    pub fn label(self) -> &'static str {
        match self {
            Self::Command => "命令",
            Self::Panel => "面板",
            Self::Setting => "设置",
        }
    }

    /// 该类别在清单里的唯一键字段名（设置项是 `key`，其余是 `id`）。
    fn key_field(self) -> &'static str {
        match self {
            Self::Command | Self::Panel => "id",
            Self::Setting => "key",
        }
    }
}

/// 已登记的一条命令（带来源插件，宿主撤下贡献点时按插件过滤）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandContribution {
    pub plugin_id: String,
    pub id: String,
    pub title: String,
    pub category: Option<String>,
    pub icon: Option<String>,
    pub shortcut: Option<String>,
}

/// 已登记的一个面板。`location` 已校验为 `left` / `right` / `center`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelContribution {
    pub plugin_id: String,
    pub id: String,
    pub title: String,
    pub location: String,
    pub icon: Option<String>,
    pub order: Option<i32>,
}

/// 已登记的一个设置项。`default` 已校验与 `setting_type` 一致。
#[derive(Debug, Clone, PartialEq)]
pub struct SettingContribution {
    pub plugin_id: String,
    pub key: String,
    pub setting_type: String,
    pub default: serde_json::Value,
    pub label: Option<String>,
    pub description: Option<String>,
}

/// 三张贡献表的合并结果。顺序 = 输入清单顺序 + 清单内声明顺序（宿主按此初排）。
#[derive(Debug, Clone, Default)]
pub struct ContributionRegistry {
    commands: Vec<CommandContribution>,
    panels: Vec<PanelContribution>,
    settings: Vec<SettingContribution>,
}

impl ContributionRegistry {
    /// 从已校验的清单构建注册表。
    ///
    /// 冲突策略是**拒绝**而不是「后者覆盖」：同一个 id 被两个插件声明时，无法判断
    /// 用户点下去该执行谁；静默覆盖会让先装的插件悄悄失效。
    pub fn build<'a, I>(manifests: I) -> Result<Self, CoreError>
    where
        I: IntoIterator<Item = &'a PluginManifest>,
    {
        let mut registry = Self::default();
        let mut command_sources: HashMap<String, String> = HashMap::new();
        let mut panel_sources: HashMap<String, String> = HashMap::new();
        let mut setting_sources: HashMap<String, String> = HashMap::new();

        for manifest in manifests {
            let plugin_id = manifest.plugin.id.as_str();
            for command in &manifest.contributes.commands {
                push_unique(
                    &mut registry.commands,
                    &mut command_sources,
                    ContributionKind::Command,
                    plugin_id,
                    &command.id,
                    command_entry(plugin_id, command),
                )?;
            }
            for panel in &manifest.contributes.panels {
                push_unique(
                    &mut registry.panels,
                    &mut panel_sources,
                    ContributionKind::Panel,
                    plugin_id,
                    &panel.id,
                    panel_entry(plugin_id, panel),
                )?;
            }
            for setting in &manifest.contributes.settings {
                push_unique(
                    &mut registry.settings,
                    &mut setting_sources,
                    ContributionKind::Setting,
                    plugin_id,
                    &setting.key,
                    setting_entry(plugin_id, setting),
                )?;
            }
        }

        Ok(registry)
    }

    pub fn commands(&self) -> &[CommandContribution] {
        &self.commands
    }

    pub fn panels(&self) -> &[PanelContribution] {
        &self.panels
    }

    pub fn settings(&self) -> &[SettingContribution] {
        &self.settings
    }

    pub fn command(&self, id: &str) -> Option<&CommandContribution> {
        self.commands.iter().find(|entry| entry.id == id)
    }

    pub fn panel(&self, id: &str) -> Option<&PanelContribution> {
        self.panels.iter().find(|entry| entry.id == id)
    }

    pub fn setting(&self, key: &str) -> Option<&SettingContribution> {
        self.settings.iter().find(|entry| entry.key == key)
    }

    pub fn is_empty(&self) -> bool {
        self.commands.is_empty() && self.panels.is_empty() && self.settings.is_empty()
    }
}

fn push_unique<T>(
    entries: &mut Vec<T>,
    sources: &mut HashMap<String, String>,
    kind: ContributionKind,
    plugin_id: &str,
    id: &str,
    entry: T,
) -> Result<(), CoreError> {
    if let Some(first) = sources.get(id) {
        return Err(CoreError::common(CommonError::General(format!(
            "{}贡献点 {} 冲突：{} 已由 {} 声明，{} 又声明了一次",
            kind.label(),
            kind.key_field(),
            id,
            first,
            plugin_id
        ))));
    }
    sources.insert(id.to_string(), plugin_id.to_string());
    entries.push(entry);
    Ok(())
}

fn command_entry(plugin_id: &str, command: &ContributesCommand) -> CommandContribution {
    CommandContribution {
        plugin_id: plugin_id.to_string(),
        id: command.id.clone(),
        title: command.title.clone(),
        category: command.category.clone(),
        icon: command.icon.clone(),
        shortcut: command.shortcut.clone(),
    }
}

fn panel_entry(plugin_id: &str, panel: &ContributesPanel) -> PanelContribution {
    PanelContribution {
        plugin_id: plugin_id.to_string(),
        id: panel.id.clone(),
        title: panel.title.clone(),
        location: panel.location.clone(),
        icon: panel.icon.clone(),
        order: panel.order,
    }
}

fn setting_entry(plugin_id: &str, setting: &ContributesSetting) -> SettingContribution {
    SettingContribution {
        plugin_id: plugin_id.to_string(),
        key: setting.key.clone(),
        setting_type: setting.setting_type.clone(),
        default: setting.default.clone(),
        label: setting.label.clone(),
        description: setting.description.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::ManifestParser;

    static TEMP_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    /// 生成一份能过解析的最小清单，再拼上贡献点片段。
    fn manifest(id: &str, contributes: &str) -> PluginManifest {
        let toml = format!(
            r#"
[plugin]
id = "{id}"
name = "{id}"
version = "1.0.0"
publisher = "test"

[plugin.engines]
rdatastation = ">=0.1"

[capabilities.frontend]
entry = "main.js"

{contributes}
"#
        );
        let path = std::env::temp_dir().join(format!(
            "rds-contributes-{}-{}.toml",
            std::process::id(),
            TEMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        std::fs::write(&path, toml).unwrap();
        let parsed = ManifestParser::parse(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        parsed
    }

    fn one_command(id: &str) -> String {
        format!(
            r#"[[contributes.commands]]
id = "{id}"
title = "命令 {id}"
"#
        )
    }

    #[test]
    fn registry_indexes_all_three_tables_in_declaration_order() {
        let a = manifest(
            "com.example.a",
            &format!(
                r#"{}
[[contributes.panels]]
id = "a.panel"
title = "A 面板"
location = "left"
order = 10

[[contributes.settings]]
key = "a.enabled"
type = "boolean"
default = true
"#,
                one_command("a.cmd")
            ),
        );
        let b = manifest(
            "com.example.b",
            &format!(
                r#"{}
[[contributes.panels]]
id = "b.panel"
title = "B 面板"
location = "right"
"#,
                one_command("b.cmd")
            ),
        );

        let registry = ContributionRegistry::build([&a, &b]).unwrap();
        assert_eq!(registry.commands().len(), 2);
        assert_eq!(registry.panels().len(), 2);
        assert_eq!(registry.settings().len(), 1);
        assert_eq!(registry.commands()[0].id, "a.cmd");
        assert_eq!(registry.commands()[1].id, "b.cmd");
        assert_eq!(registry.panels()[0].plugin_id, "com.example.a");
        assert_eq!(registry.panels()[1].location, "right");
        assert_eq!(
            registry.setting("a.enabled").unwrap().plugin_id,
            "com.example.a"
        );
        assert!(registry.command("a.cmd").is_some());
        assert!(registry.command("missing").is_none());
        assert!(!registry.is_empty());
    }

    #[test]
    fn a_command_id_declared_by_two_plugins_is_a_conflict() {
        let a = manifest("com.example.a", &one_command("shared.cmd"));
        let b = manifest("com.example.b", &one_command("shared.cmd"));

        let error = ContributionRegistry::build([&a, &b])
            .unwrap_err()
            .to_string();
        assert!(error.contains("命令贡献点 id 冲突"), "{error}");
        assert!(error.contains("shared.cmd"), "{error}");
        assert!(error.contains("com.example.a"), "{error}");
        assert!(error.contains("com.example.b"), "{error}");
    }

    #[test]
    fn same_id_in_different_kinds_is_allowed() {
        let a = manifest("com.example.a", &one_command("shared.id"));
        let b = manifest(
            "com.example.b",
            r#"[[contributes.panels]]
id = "shared.id"
title = "同名面板"
location = "center"
"#,
        );

        let registry = ContributionRegistry::build([&a, &b]).unwrap();
        assert_eq!(registry.commands()[0].id, "shared.id");
        assert_eq!(registry.panels()[0].id, "shared.id");
    }

    #[test]
    fn duplicate_setting_keys_are_a_conflict() {
        let setting = r#"[[contributes.settings]]
key = "shared.key"
type = "string"
default = "x"
"#;
        let a = manifest("com.example.a", setting);
        let b = manifest("com.example.b", setting);

        let error = ContributionRegistry::build([&a, &b])
            .unwrap_err()
            .to_string();
        assert!(error.contains("设置贡献点 key 冲突"), "{error}");
        assert!(error.contains("shared.key"), "{error}");
    }
}
