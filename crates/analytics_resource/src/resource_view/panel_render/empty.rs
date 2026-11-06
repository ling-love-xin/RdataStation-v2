//! 列表区与空态：列表、加载骨架、空库、无匹配。

use super::*;

impl ResourcesPanel {
    /// 列表主体：`List`（虚拟化 + 组件化的 hover / 选中 / 键盘漫游）。
    ///
    /// 行数据在委托里（面板重算可见行时推过去）：`render_item` 在列表渲染期被调用，
    /// 那一刻面板正被借用，不能回头读面板的行集合。滚动归 `List` 自己（虚拟化列表自带）。
    pub(super) fn render_rows(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(list) = self.list.clone() else {
            // 理论上不可达（`render` 已先 `ensure_list`）：退化成空区而不是 panic。
            let _ = cx;
            return div().flex_1().into_any_element();
        };
        div()
            .flex_1()
            .min_h_0()
            .child(List::new(&list).flex_1())
            .into_any_element()
    }
    /// 加载中骨架（原型 §5）：3 行灰条，**不用转圈**——形状直接提示"这里将要出现行"，
    /// 宽度递减以免看着像真行。
    ///
    /// 灰条走组件的 `Skeleton`（自带 2s 呼吸，只改透明度；`reduce_motion` 下停在全亮）：
    /// 这是**暂态**（首帧取数期间），符合「动效只挂暂态」；列表已有行时不摆它，所以
    /// 不会每帧重绘。
    pub(super) fn render_loading(&self) -> Div {
        let mut skeleton = div()
            .flex_1()
            .v_flex()
            .w_full()
            .min_h_0()
            .gap_2()
            .p_2()
            // 调试选择器：窗口测试靠它断言"取数期间不摆空态"。
            .debug_selector(|| "archive-loading".to_string());
        for ratio in [0.75_f32, 0.55, 0.65] {
            skeleton = skeleton.child(
                Skeleton::new()
                    .h(rems(ui::ROW_HEIGHT))
                    .w(relative(ratio))
                    .rounded_sm(),
            );
        }
        skeleton
    }
    pub(super) fn render_empty(&self, _cx: &mut Context<Self>) -> Empty {
        let read_only = self.snapshot.read_only;
        // 空态按实例分流（§13.2）：全局实例不直接归档，只给一句"升级上来"的引导，
        // 不摆草稿箱 / 本地文件两个项目侧入口；项目与「全部」视图保持原归档引导。
        // 组件选型：gpui-kit `empty::Empty`（规格 §7.2 UI 统一；自带居中布局 + 虚线框），
        // 标题 / 说明 / 按钮分别进 `EmptyHeader` / `EmptyContent` 槽位。
        let is_global = self.instance == InstanceFilter::Global;
        let (title, hint) = if is_global {
            (
                "全局实例还没有存档",
                "把项目里值得共享的存档升级到全局后会出现在这里；全局实例不直接归档，跨项目分发都从这里选。",
            )
        } else {
            (
                "还没有任何存档",
                "把草稿箱里值得留存的文件归档进来，它就变成只读、带版本、可追溯的存档",
            )
        };

        let empty = Empty::new().header(
            EmptyHeader::new()
                .title(EmptyTitle::new().child(title))
                .description(EmptyDescription::new().child(hint)),
        );

        if is_global {
            empty
        } else {
            // 两个按钮各拿一份（`Rc` 不是 `Copy`：两个闭包都要 `move` 走它）。
            let host_for_drafts = self.host.clone();
            let host_for_file = self.host.clone();
            empty.content(
                EmptyContent::new()
                    .child(
                        Button::new("archive-for-empty")
                            .primary()
                            .debug_selector(|| "archive-empty-action".to_string())
                            .label("从草稿箱归档…")
                            .disabled(read_only)
                            .on_click(move |_, window, cx| {
                                host_for_drafts.request_archive_from_drafts(window, cx)
                            }),
                    )
                    .child(
                        Button::new("archive-for-empty-file")
                            .secondary()
                            .label("从本地文件归档…")
                            .disabled(read_only)
                            .on_click(move |_, window, cx| {
                                host_for_file.request_archive_from_file(window, cx)
                            }),
                    ),
            )
        }
    }
    /// 搜索 / 筛选无结果：**不是空库态**（原型 §5 状态与空态矩阵）。
    ///
    /// 两态的出口才是关键区别：空库引导"归档"（没有东西可用），无匹配引导"清筛选"
    /// （东西在，只是被挡住了）。文案混用会把用户引到错误的下一步。
    pub(super) fn render_no_match(&self, cx: &mut Context<Self>) -> Empty {
        let panel = cx.entity();

        // 副文案由数据层给（`filter::no_match_hint`）：搜的是“匹配面”，
        // 能搜哪些字段就该由管匹配面的那处说了算，而不是渲染层自己编一句。
        Empty::new()
            .header(
                EmptyHeader::new()
                    .title(EmptyTitle::new().child("没有匹配的存档"))
                    .description(
                        EmptyDescription::new().child(filter::no_match_hint(&self.filter.query)),
                    ),
            )
            .content(
                EmptyContent::new().child(
                    Button::new("archive-clear-filter")
                        .ghost()
                        .label("清空筛选")
                        .on_click(move |_, window, app| {
                            panel.update(app, |panel, cx| panel.clear_filter(window, cx));
                        }),
                ),
            )
    }
}
