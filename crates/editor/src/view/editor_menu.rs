//! 编辑器正文右键菜单（B2）：中文定制菜单项
//!
//! ## 一句话
//!
//! gpui-component 的 `Editor`（经 `Input::render`）**默认就挂右键菜单**（英文：Go to
//! Definition / Show Code Actions / Cut / Copy / Paste / Select All，`input.rs` 的
//! `state.on_context_menu`）。本模块用 `Editor::context_menu` **覆盖**成中文菜单，
//! 并把「哪些项出现、哪些禁用」抽成纯函数——菜单项的顺序 / 禁用态可以在 headless
//! 测试里断言，不用读弹层内部（`NativeMenu` 不暴露条目）。
//!
//! ## 能力位口径（与内核 capabilities 对齐）
//!
//! - 可编辑 = 非只读且未禁用（`is_editable`，与内核 `InputContextMenuCapabilities::is_editable` 同式）；
//! - 有选区 = `selected_range` 非空（复制 / 剪切可用）；
//! - 定义项只在挂了 definition provider 时出现（`lsp().definition_provider` 非空）；
//! - 代码操作项只在挂了 code action provider 时出现（`lsp().code_action_providers` 非空）
//!   ——A1 接线后 SQL / LSP 模式都有，文本模式没有。

use gpui_kit::component::input::{
    Copy, Cut, EditorState, GoToDefinition, Paste, SelectAll, ToggleCodeActions,
};
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::*;

/// 菜单条目（渲染时逐条翻译成 `NativeMenuItem`；纯数据便于测试）
///
/// 不派生任何比较 / 克隆 trait：`Box<dyn Action>` 既不是 `PartialEq` 也不是
/// `Clone`。测试断言走 [`menu_entries`] 的形状（文案 + 禁用态），不比较动作本身。
pub enum EditorMenuEntry {
    /// 可点击项（`disabled = true` 置灰）
    Action {
        label: &'static str,
        disabled: bool,
        /// 派发动作（与菜单栏 / 快捷键同机制：`Window::dispatch_action`）
        action: Box<dyn gpui::Action>,
    },
    Separator,
}

/// 按当前内核状态生成中文右键菜单条目（纯函数，可测）
///
/// 顺序对齐 VS Code 中文习惯：剪切 / 复制 / 粘贴 → 全选 → 转到定义 / 代码操作。
/// 「全选」常亮（与 Input 默认菜单一致——全选总无害）；写操作（剪切 / 粘贴）
/// 只在可编辑时可用；复制只看有没有选区（编辑器模式无掩码，不重复判定掩码位）。
pub fn editor_context_menu_entries(state: &EditorState) -> Vec<EditorMenuEntry> {
    let editable = state.is_editable();
    let has_selection = state.selected_range().start != state.selected_range().end;
    let lsp = state.lsp();
    let has_definition = lsp.definition_provider.is_some();
    let has_code_actions = !lsp.code_action_providers.is_empty();
    menu_entries(editable, has_selection, has_definition, has_code_actions)
}

/// 能力位 → 菜单条目（**纯函数**；测试与渲染共用，不依赖窗口）
pub fn menu_entries(
    editable: bool,
    has_selection: bool,
    has_definition: bool,
    has_code_actions: bool,
) -> Vec<EditorMenuEntry> {
    let mut entries = vec![
        EditorMenuEntry::Action {
            label: "剪切",
            disabled: !(editable && has_selection),
            action: Box::new(Cut),
        },
        EditorMenuEntry::Action {
            label: "复制",
            disabled: !has_selection,
            action: Box::new(Copy),
        },
        EditorMenuEntry::Action {
            label: "粘贴",
            disabled: !editable,
            action: Box::new(Paste),
        },
        EditorMenuEntry::Separator,
        EditorMenuEntry::Action {
            label: "全选",
            disabled: false,
            action: Box::new(SelectAll),
        },
    ];
    if has_definition || has_code_actions {
        entries.push(EditorMenuEntry::Separator);
    }
    if has_definition {
        entries.push(EditorMenuEntry::Action {
            label: "转到定义",
            disabled: false, // 只读也能跳转（定义项只读不置灰，与 Input 默认一致）
            action: Box::new(GoToDefinition),
        });
    }
    if has_code_actions {
        entries.push(EditorMenuEntry::Action {
            label: "代码操作…",
            disabled: !editable,
            action: Box::new(ToggleCodeActions),
        });
    }
    entries
}

/// 把纯条目翻译成 `NativeMenu`（渲染用；`editor_context_menu_entries` 的消费端）
pub fn menu_from_entries(entries: Vec<EditorMenuEntry>) -> NativeMenu {
    let mut menu = NativeMenu::new();
    for entry in entries {
        match entry {
            EditorMenuEntry::Action { label, disabled, action } => {
                menu = menu.menu_with_disabled(label, disabled, action)
            }
            EditorMenuEntry::Separator => menu = menu.separator(),
        }
    }
    menu
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{EditorMenuEntry, menu_entries};

    /// 断言菜单条目的**形状**（顺序 + 文案 + 禁用态），不读弹层内部
    fn shape(entries: &[EditorMenuEntry]) -> Vec<(&'static str, bool)> {
        entries
            .iter()
            .map(|entry| match entry {
                EditorMenuEntry::Action { label, disabled, .. } => (*label, *disabled),
                EditorMenuEntry::Separator => ("—", false),
            })
            .collect()
    }

    #[test]
    fn empty_editor_offers_edit_items_and_select_all() {
        // 可编辑、无选区、无 provider：只有剪切 / 复制 / 粘贴 / 全选
        let entries = menu_entries(true, false, false, false);
        assert_eq!(
            shape(&entries),
            vec![
                ("剪切", true),  // 无选区 → 剪切置灰
                ("复制", true),  // 无选区 → 复制置灰
                ("粘贴", false), // 可编辑 → 粘贴可用
                ("—", false),
                ("全选", false),
            ]
        );
    }

    #[test]
    fn selection_enables_cut_and_copy() {
        let entries = menu_entries(true, true, false, false);
        let shape = shape(&entries);
        assert_eq!(shape[0], ("剪切", false)); // 有选区 + 可编辑 → 剪切可用
        assert_eq!(shape[1], ("复制", false)); // 有选区 → 复制可用
        assert_eq!(shape[2], ("粘贴", false));
    }

    #[test]
    fn readonly_disables_writes_but_keeps_copy() {
        // 只读 + 有选区：剪切 / 粘贴置灰，复制可用
        let entries = menu_entries(false, true, false, false);
        let shape = shape(&entries);
        assert_eq!(shape[0], ("剪切", true));
        assert_eq!(shape[1], ("复制", false));
        assert_eq!(shape[2], ("粘贴", true));
    }

    #[test]
    fn providers_add_definition_and_code_action_sections() {
        let entries = menu_entries(true, false, true, true);
        let shape = shape(&entries);
        // 剪切 / 复制（无选区置灰）/ 粘贴 / 分隔 / 全选 / 分隔 / 转到定义 / 代码操作
        assert_eq!(shape.len(), 8, "{shape:?}");
        assert_eq!(shape[5], ("—", false));
        assert_eq!(shape[6], ("转到定义", false));
        assert_eq!(shape[7], ("代码操作…", false));
    }

    #[test]
    fn code_actions_greyed_out_when_not_editable() {
        let entries = menu_entries(false, false, false, true);
        let shape = shape(&entries);
        let last = shape.last().expect("有代码操作项");
        assert_eq!(*last, ("代码操作…", true));
    }
}
