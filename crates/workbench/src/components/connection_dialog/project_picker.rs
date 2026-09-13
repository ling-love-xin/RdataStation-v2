//! 项目下拉项：**项目名（左）+ 路径（右、省略显示）**左右结构；末项为「＋ 新增项目」。
//!
//! 交互约定（真机反馈）：
//! - 项目栏只有一个下拉框，不再有「编辑 / 列表」模式切换；
//! - 选项文本只呈现项目名（路径作为次要信息靠右、超长省略）；
//! - 最后一项固定为「＋ 新增项目」——选中后由宿主打开项目新建入口。

use super::*;
use gpui_kit::component::searchable_list::SearchableListItem;

/// 「＋ 新增项目」选项的显示值（选中即请求宿主打开项目新建入口）。
pub const PROJECT_NEW_LABEL: &str = "＋ 新增项目";

/// 「打开现有目录…」选项的显示值（选中即请求宿主打开「打开现有目录」对话框）。
pub const PROJECT_OPEN_LABEL: &str = "打开现有目录…";

/// 「不需要项目（仅全局）」选项的显示值：选中即把作用域切为「仅全局」。
pub const PROJECT_NONE_LABEL: &str = "不需要项目（仅全局）";

/// 下拉项类别：数据项与三个动作项（动作项不带路径，选中后置位宿主请求标记或改作用域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProjectItemKind {
    /// 普通项目（带项目根路径）。
    Project,
    /// 「＋ 新增项目」（末项，固定位置）。
    NewProject,
    /// 「打开现有目录…」。
    OpenFolder,
    /// 「不需要项目（仅全局）」：把作用域切为「仅全局」。
    NoProject,
}

/// 项目下拉项（`value` = 项目名；动作项 value 同显示值）。
///
/// `pub` 且经 `connection_dialog` 再导出：`ConnectionDialogState::project_sel` 是公开字段，
/// 集成测试需要能命名本类型以驱动下拉。
#[derive(Clone)]
pub struct ProjectItem {
    label: SharedString,
    path: SharedString,
    kind: ProjectItemKind,
}

impl ProjectItem {
    /// 普通项目项（名称 + 项目根路径）。
    pub fn project(name: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            label: SharedString::from(name.into()),
            path: SharedString::from(path.into()),
            kind: ProjectItemKind::Project,
        }
    }

    /// 「＋ 新增项目」入口项。
    pub fn new_project() -> Self {
        Self {
            label: SharedString::from(PROJECT_NEW_LABEL),
            path: SharedString::from(""),
            kind: ProjectItemKind::NewProject,
        }
    }

    /// 「打开现有目录…」入口项。
    pub fn open_folder() -> Self {
        Self {
            label: SharedString::from(PROJECT_OPEN_LABEL),
            path: SharedString::from(""),
            kind: ProjectItemKind::OpenFolder,
        }
    }

    /// 「不需要项目（仅全局）」入口项。
    pub fn no_project() -> Self {
        Self {
            label: SharedString::from(PROJECT_NONE_LABEL),
            path: SharedString::from(""),
            kind: ProjectItemKind::NoProject,
        }
    }

    pub fn path(&self) -> &SharedString {
        &self.path
    }

    /// 是否为「＋ 新增项目」动作项。
    pub fn is_new(&self) -> bool {
        self.kind == ProjectItemKind::NewProject
    }

    /// 是否为动作项（不带路径，选中后置位宿主请求标记）。
    pub fn is_action(&self) -> bool {
        self.kind != ProjectItemKind::Project
    }

    pub fn kind(&self) -> ProjectItemKind {
        self.kind
    }
}

impl SearchableListItem for ProjectItem {
    type Value = SharedString;

    fn title(&self) -> SharedString {
        self.label.clone()
    }

    /// 触发器显示：项目名（左）+ 路径（右、弱化、超长从头部省略）。
    ///
    /// 该签名无 `App`，因此路径的“次要”观感用 `opacity` 表达（不依赖主题取色）。
    fn display_title(&self) -> Option<AnyElement> {
        Some(project_row(self).into_any_element())
    }

    /// 下拉行内容：与触发器同构（此处可拿到 `App`，但保持同一观感）。
    fn render(&self, _: &mut Window, _: &mut App) -> impl IntoElement {
        project_row(self)
    }

    fn value(&self) -> &Self::Value {
        &self.label
    }

    /// 搜索匹配：项目名或路径命中。
    fn matches(&self, query: &str) -> bool {
        let q = query.to_lowercase();
        self.label.to_lowercase().contains(&q) || self.path.to_lowercase().contains(&q)
    }
}

/// 行元素：左项目名 + 右路径（`flex_1` + 头部省略，长路径保留尾部目录名）。
/// 动作项（新增项目 / 打开现有目录）只显示标签，用主色区分可点击行为。
fn project_row(item: &ProjectItem) -> Div {
    if item.is_action() {
        return div()
            .h_flex()
            .items_center()
            .text_xs()
            .child(item.label.clone());
    }
    div()
        .h_flex()
        .items_center()
        .gap(rems(GAP_MD))
        .w_full()
        .min_w(rems(0.))
        .child(div().flex_shrink_0().text_xs().child(item.label.clone()))
        .child(
            div()
                .flex_1()
                .min_w(rems(0.))
                .overflow_hidden()
                .text_ellipsis_start()
                .text_xs()
                .opacity(0.6)
                .child(item.path.clone()),
        )
}
