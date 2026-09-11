//! 项目下拉项：**项目名（左）+ 路径（右、省略显示）**左右结构；末项为「＋ 新增项目」。
//!
//! 交互约定（真机反馈）：
//! - 项目栏只有一个下拉框，不再有「编辑 / 列表」模式切换；
//! - 选项文本只呈现项目名（路径作为次要信息靠右、超长省略）；
//! - 最后一项固定为「＋ 新增项目」——选中后由宿主打开项目新建入口。

use super::*;
use gpui_kit::component::searchable_list::SearchableListItem;

/// 「＋ 新增项目」选项的显示值（选中即请求打开项目新建入口）。
pub const PROJECT_NEW_LABEL: &str = "＋ 新增项目";

/// 项目下拉项（`value` = 项目名；「新增项目」项 value 同显示值）。
///
/// `pub` 且经 `connection_dialog` 再导出：`ConnectionDialogState::project_sel` 是公开字段，
/// 集成测试需要能命名本类型以驱动下拉。
#[derive(Clone)]
pub struct ProjectItem {
    label: SharedString,
    path: SharedString,
    /// 是否为「＋ 新增项目」入口项（无路径）。
    is_new: bool,
}

impl ProjectItem {
    /// 普通项目项（名称 + 项目根路径）。
    pub fn project(name: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            label: SharedString::from(name.into()),
            path: SharedString::from(path.into()),
            is_new: false,
        }
    }

    /// 「＋ 新增项目」入口项。
    pub fn new_project() -> Self {
        Self {
            label: SharedString::from(PROJECT_NEW_LABEL),
            path: SharedString::from(""),
            is_new: true,
        }
    }

    pub fn path(&self) -> &SharedString {
        &self.path
    }

    pub fn is_new(&self) -> bool {
        self.is_new
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
fn project_row(item: &ProjectItem) -> Div {
    if item.is_new {
        return div().h_flex().items_center().text_xs().child(item.label.clone());
    }
    div()
        .h_flex()
        .items_center()
        .gap(rems(GAP_MD))
        .w_full()
        .min_w(px(0.))
        .child(div().flex_shrink_0().text_xs().child(item.label.clone()))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .overflow_hidden()
                .text_ellipsis_start()
                .text_xs()
                .opacity(0.6)
                .child(item.path.clone()),
        )
}
