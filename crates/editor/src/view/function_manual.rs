//! 【#54-C5】函数手册对话框（Quick Open 式）
//!
//! 场景：用户想不起函数名或写法时，中文语义搜索——输入「截取」→ `SUBSTR`。
//! 数据来自 `engine::sql` 的静态手册（157 条 / 9 类，中英混搜排序：函数名前缀 > 包含 >
//! 中文关键词 > 中文描述），不接 LLM 推荐、不全量中文化（方案已与用户确认）。
//!
//! 交互复用组件库 `List`（自带搜索框 + 键盘导航 + Enter 确认，Quick Open 同款）：
//! - 搜索框输入 → `perform_search` 重算结果（纯内存，157 条同步完成）
//! - ↑↓ 移动选中、Enter / 点击 → `confirm` → 宿主插入函数名到光标处并关弹层
//! - Esc 关闭（组件自带）
//!
//! 返回 `Entity<ListState<ManualDelegate>>`：测试可直接驱动 delegate 的
//! `perform_search` / `confirm`（headless 点击到不了对话框层，沿用项目既有口径）。

use std::rc::Rc;

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::list::{List, ListDelegate, ListItem, ListState};
use gpui_kit::component::{ActiveTheme, IndexPath, WindowExt as _};
use gpui_kit::*;

use engine::sql::{search_manual, ManualEntry};

use crate::ui;

/// 手册结果代理：一行 = 函数名（等宽）+ 分类标签 + 中文描述（两行式）
pub struct ManualDelegate {
    /// 当前搜索结果（空查询 = 全部手册）
    entries: Vec<&'static ManualEntry>,
    /// 当前选中行（组件回传，`confirm` 时取它）
    selected: Option<IndexPath>,
    /// 宿主插入回调：确认后由它把函数名写进编辑内核（`'static` 闭包捕获宿主实体）
    on_insert: Rc<dyn Fn(String, &mut Window, &mut App)>,
}

impl ListDelegate for ManualDelegate {
    type Item = ListItem;

    fn items_count(&self, _section: usize, _cx: &App) -> usize {
        self.entries.len()
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<Self::Item> {
        let entry = self.entries.get(ix.row)?;
        let theme = cx.theme();
        let muted = theme.colors.muted_foreground;

        // 第一行：函数名（等宽，一眼区分「函数」与描述）+ 分类小标签
        let first = div()
            .h_flex()
            .items_center()
            .w_full()
            .min_w_0()
            .gap_2()
            .child(
                div()
                    .flex_none()
                    .font_family("monospace")
                    .text_sm()
                    .text_color(theme.colors.foreground)
                    .child(entry.name),
            )
            .child(
                div()
                    .flex_none()
                    .rounded_sm()
                    .border_1()
                    .border_color(theme.colors.border)
                    .px_1()
                    .text_xs()
                    .text_color(muted)
                    .child(entry.category.label()),
            );
        // 第二行：中文一句话描述（超长省略，悬停可看全）
        let second = div()
            .w_full()
            .min_w_0()
            .text_xs()
            .text_color(muted)
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis()
            .child(entry.zh);

        Some(
            ListItem::new(SharedString::from(format!("fn-manual-{}", entry.name)))
                .h(rems(ui::FUNCTION_MANUAL_ROW_HEIGHT))
                .py_0()
                .px_2()
                .gap_2()
                .child(
                    div()
                        .v_flex()
                        .w_full()
                        .min_w_0()
                        .gap_1()
                        .child(first)
                        .child(second),
                ),
        )
    }

    fn render_empty(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .v_flex()
            .items_center()
            .gap_1()
            .py_6()
            .text_xs()
            .text_color(theme.colors.muted_foreground)
            .child(div().text_color(theme.colors.foreground).child("无匹配函数"))
            .child("试试输入函数名（substr）或中文语义（截取 / 日期）")
            .into_any_element()
    }

    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        self.selected = ix;
        cx.notify();
    }

    /// 搜索框内容变化 → 重算结果（引擎纯函数，同步完成；空输入 = 全量手册）
    fn perform_search(
        &mut self,
        query: &str,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Task<()> {
        self.entries = search_manual(query);
        cx.notify();
        Task::ready(())
    }

    /// Enter / 点击确认：插入选中函数名 → 交回宿主（宿主关弹层已由组件处理前完成）
    fn confirm(
        &mut self,
        _secondary: bool,
        window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        let Some(ix) = self.selected else {
            return;
        };
        let Some(entry) = self.entries.get(ix.row) else {
            return;
        };
        let name = entry.name.to_string();
        let on_insert = self.on_insert.clone();
        window.close_dialog(cx);
        on_insert(name, window, cx);
    }
}

/// 打开函数手册对话框
///
/// `on_insert`：确认后携带函数名回调宿主（宿主负责插入编辑器 + 同步服务层）。
/// 返回 `ListState` 实体：测试经它直接驱动搜索 / 确认（headless 口径，见模块头）。
pub fn open_function_manual(
    window: &mut Window,
    cx: &mut App,
    on_insert: impl Fn(String, &mut Window, &mut App) + 'static,
) -> Entity<ListState<ManualDelegate>> {
    let delegate = ManualDelegate {
        entries: search_manual(""),
        selected: None,
        on_insert: Rc::new(on_insert),
    };
    let state = cx.new(|cx| {
        ListState::new(delegate, window, cx)
            .searchable(true)
            .selectable(true)
    });
    let list = state.clone();

    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let body = div()
            .v_flex()
            .w_full()
            .gap_2()
            .child(
                div()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child("输入函数名或中文语义，回车插入函数名"),
            )
            .child(List::new(&list));
        dialog
            .title("函数手册")
            .w(theme.font_size * ui::FUNCTION_MANUAL_DIALOG_WIDTH)
            .child(body)
    });

    state
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**（`use super::*` 会带进 gpui_kit::*，`#[gpui_kit::test]`
    // 展开出的裸 `#[test]` 会解析到它自己 → recursion limit，见 gpui-kit-dev skill）
    use super::{ManualDelegate, open_function_manual};
    use gpui_kit::component::IndexPath;
    use gpui_kit::component::list::{ListDelegate, ListState};
    use gpui_kit::component::Root;
    use gpui_kit::component::WindowExt as _;
    use gpui_kit::{
        AppContext as _, Context, Entity, IntoElement, Render, Styled as _, TestAppContext,
        Window, div,
    };

    /// 空壳根视图：只包 `Root`（对话框要真渲染出来需要组件库的 Root 挂 dialog 层）
    struct RootShell;

    impl Render for RootShell {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div().size_full()
        }
    }

    #[gpui_kit::test]
    fn opens_searches_and_confirms(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);

        let inserted = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let ins = inserted.clone();
        let slot = std::rc::Rc::new(std::cell::RefCell::new(None::<Entity<ListState<ManualDelegate>>>));
        let slot_in = slot.clone();
        // 窗口创建闭包里只挂 Root（open_dialog 依赖 Root entity，必须等窗口建好再开）
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let shell = cx.new(|_cx| RootShell);
            Root::new(shell, window, cx)
        });
        // 生产入口开窗：on_insert 记录被插入的函数名（Root 已挂，open_dialog 可走）
        cx.update(|window, cx| {
            let state = open_function_manual(window, cx, move |name, _, _| {
                ins.borrow_mut().push(name);
            });
            *slot_in.borrow_mut() = Some(state);
        });
        let state = slot.borrow().clone().expect("ListState 已创建");

        // 层真渲染（headless 口径：断言弹层存在，不赌点击）
        cx.update(|window, cx| assert!(window.has_active_dialog(cx)));

        // 中文语义搜索：截取 → SUBSTR 命中
        cx.update(|window, cx| {
            state.update(cx, |s, cx| {
                _ = s.delegate_mut().perform_search("截取", window, cx);
            });
        });
        let entries = cx.update(|_window, cx| state.read(cx).delegate().entries.clone());
        assert!(
            entries.iter().any(|e| e.name == "SUBSTR"),
            "中文语义「截取」应命中 SUBSTR"
        );
        let row = entries
            .iter()
            .position(|e| e.name == "SUBSTR")
            .expect("SUBSTR 应在结果里");

        // 选中 + 确认（Enter 语义）：回调收到函数名，弹层关闭
        cx.update(|window, cx| {
            state.update(cx, |s, cx| {
                s.delegate_mut().selected = Some(IndexPath {
                    section: 0,
                    row,
                    column: 0,
                });
                s.delegate_mut().confirm(false, window, cx);
            });
        });
        assert_eq!(
            inserted.borrow().as_slice(),
            &["SUBSTR"],
            "确认应把选中函数名交给宿主"
        );
        cx.update(|window, cx| assert!(!window.has_active_dialog(cx)));
    }
}
