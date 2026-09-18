//! Quick Open 的命令目录（**当前唯一权威**：原先硬编码在 `view.rs`，后搬进 `model.rs`，现独立成文件）。
//!
//! ## 为什么先不做跨 crate 登记
//!
//! 命令要能落地得先回答「谁执行」——本仓的执行入口**全在宿主**：新建文档要 `&mut Window`、
//! 打开资产库要顺带刷新列表、选中连接要清导航缓存。特性 crate 手上没有宿主实体，
//! 注册不了行为；只登记一条点不动的命令，等于「没实现却先宣传」（编辑器模块的既有原则）。
//!
//! ## 已经留好的口子（Phase 1 → 插件 M9 beta3）
//!
//! - 每条命令有**稳定 id**（`spec.id`）与 `keywords`（参与匹配，中英皆可）；
//! - 宿主把「id → 行为」的映射放在 `view.rs::execute_quick_open_action`（唯一执行点）；
//! - 将来做真登记（全局登记表 + 宿主端口）时，`id` / `keywords` / `label` 原样搬迁，
//!   只需把本表换成从登记表读。

use editor::model::EditorMode;

use crate::quick_open::model::{Action, Row, RowKind};
use crate::view::{LeftPanel, RightPanel};

/// 一条命令的目录项。
pub(crate) struct CommandSpec {
    /// 稳定 id（选中跟随 / 将来登记表的键）。
    pub id: &'static str,
    /// 展示名（也是高亮与匹配的主文本）。
    pub label: &'static str,
    /// 快捷键提示（没有就空串）。
    pub shortcut: &'static str,
    /// 参与匹配的补充词：英文名 / 常见别名（不展示，仅让 `settings` 这类输入也能命中）。
    pub keywords: &'static str,
    pub action: Action,
}

/// 命令目录。顺序 = 展示顺序（“新建”放最前：Quick Open 是工作台的命令面）。
pub(crate) fn command_specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "file.new_query",
            label: "新建查询",
            shortcut: "Ctrl+N",
            keywords: "new query sql 查询",
            action: Action::NewDocument(EditorMode::Sql),
        },
        CommandSpec {
            id: "file.new_note",
            label: "新建笔记",
            shortcut: "",
            keywords: "new note markdown 笔记",
            action: Action::NewDocument(EditorMode::Analysis),
        },
        CommandSpec {
            id: "file.new_file",
            label: "新建文件",
            shortcut: "",
            keywords: "new file text 文件",
            action: Action::NewDocument(EditorMode::Text),
        },
        CommandSpec {
            id: "panel.draft",
            label: "打开草稿箱",
            shortcut: "",
            keywords: "draft scratchpad 草稿",
            action: Action::OpenLeftPanel(LeftPanel::Draft),
        },
        CommandSpec {
            id: "panel.database",
            label: "打开数据库导航",
            shortcut: "",
            keywords: "database navigator explorer 导航 数据源",
            action: Action::OpenLeftPanel(LeftPanel::Database),
        },
        CommandSpec {
            id: "panel.resources",
            label: "打开资产库",
            shortcut: "",
            keywords: "resources archive 资产 存档",
            action: Action::OpenLeftPanel(LeftPanel::Resources),
        },
        CommandSpec {
            id: "panel.plugin",
            label: "打开插件",
            shortcut: "",
            keywords: "plugin 插件",
            action: Action::OpenLeftPanel(LeftPanel::Plugin),
        },
        CommandSpec {
            id: "panel.insight",
            label: "打开洞察",
            shortcut: "",
            keywords: "insight 洞察",
            action: Action::OpenRightPanel(RightPanel::Insight),
        },
        CommandSpec {
            id: "panel.mock",
            label: "打开 Mock 生成",
            shortcut: "",
            keywords: "mock data generator 生成 测试数据",
            action: Action::OpenRightPanel(RightPanel::Mock),
        },
        CommandSpec {
            id: "panel.history",
            label: "打开历史",
            shortcut: "",
            keywords: "history 历史 查询记录",
            action: Action::OpenRightPanel(RightPanel::History),
        },
        CommandSpec {
            id: "app.settings",
            label: "打开设置",
            shortcut: "Ctrl+,",
            keywords: "settings preferences 设置",
            action: Action::OpenSettings,
        },
        CommandSpec {
            id: "view.hide_sidebars",
            label: "完全隐藏侧边栏",
            shortcut: "",
            keywords: "hide sidebars zen 隐藏 专注",
            action: Action::HideSidebars,
        },
        CommandSpec {
            id: "view.restore_sidebars",
            label: "恢复侧边栏",
            shortcut: "",
            keywords: "restore sidebars 恢复 侧边栏",
            action: Action::RestoreSidebars,
        },
    ]
}

/// 目录 → 结果行（`match_text` = 展示名 + 关键词，让英文名 / 别名也能命中）。
pub(crate) fn command_rows() -> Vec<Row> {
    command_specs()
        .into_iter()
        .map(|spec| Row {
            key: format!("cmd:{}", spec.id),
            kind: RowKind::Command,
            title: spec.label.to_string(),
            secondary: spec.shortcut.to_string(),
            match_text: format!("{} {}", spec.label, spec.keywords),
            snippet: None,
            why: None,
            action: spec.action,
        })
        .collect()
}
