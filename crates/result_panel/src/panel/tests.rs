//! panel/ 全量单元测试

    use crate::column_browser::filter_browser_columns;
    use super::next_right_edge_state;
    use super::reorder_by;
    use super::right_tab_from_index;
    use super::sidebar_width_anim;
    use crate::ports::RightTab;
    use gpui_kit::px;
    use gpui_kit::Pixels;

    /// 三份等长样本：列名 / 类型名 / 注释（注释可空）
    fn sample() -> (Vec<String>, Vec<String>, Vec<Option<String>>) {
        let names = vec![
            "id".to_string(),
            "user_name".to_string(),
            "created_at".to_string(),
        ];
        let types = vec![
            "INTEGER".to_string(),
            "VARCHAR".to_string(),
            "TIMESTAMP".to_string(),
        ];
        let comments = vec![
            Some("主键".to_string()),
            Some("用户登录名".to_string()),
            None,
        ];
        (names, types, comments)
    }

    /// 侧栏宽度动画参数：首帧免动画（from==to）/ 展开 0→全宽 / 收起 全宽→0
    #[test]
    fn sidebar_width_anim_first_frame_and_directions() {
        let full = px(36.);
        // 首帧（epoch=0）：无论开合都直接落位，不开场动画
        let (from, to) = sidebar_width_anim(0, true, full);
        assert_eq!(from, to);
        assert_eq!(to, full);
        let (from, to) = sidebar_width_anim(0, false, full);
        assert_eq!(from, to);
        assert_eq!(to, px(0.));
        // 展开（epoch>0）：0 → 全宽
        let (from, to) = sidebar_width_anim(1, true, full);
        assert_eq!(from, px(0.));
        assert_eq!(to, full);
        // 收起（epoch>0）：全宽 → 0
        let (from, to) = sidebar_width_anim(1, false, full);
        assert_eq!(from, full);
        assert_eq!(to, px(0.));
        // 类型约束（Pixels 参与计算，编译期校验）
        let _: (Pixels, Pixels) = sidebar_width_anim(2, true, full);
    }

    /// 右侧开关三态循环：收起 → 窄展开 → 宽展开 → 收起；每次点击恰好在三态间轮转
    #[test]
    fn right_edge_state_cycles_through_three_states() {
        // 从宽展开（初始默认）出发，点击三次回原态
        assert_eq!(next_right_edge_state(true, true), (false, false));
        assert_eq!(next_right_edge_state(false, false), (true, false));
        assert_eq!(next_right_edge_state(true, false), (true, true));
        // 宽展开恒有竖条可见（不变量）
        let (edge, show) = next_right_edge_state(true, false);
        assert!(show);
        assert!(edge);
        // 收起态点击直接进窄展开，不跨到宽展开（比左侧多一个展开态：窄 = 只显图标条）
        let (edge, show) = next_right_edge_state(false, false);
        assert!(edge);
        assert!(!show);
    }

    /// 空词 / 纯空白 = 全量原序（隐藏列由渲染层置灰，过滤本身不剔列）
    #[test]
    fn browser_filter_empty_returns_all_in_order() {
        let (n, t, c) = sample();
        assert_eq!(filter_browser_columns(&n, &t, &c, ""), vec![0, 1, 2]);
        assert_eq!(filter_browser_columns(&n, &t, &c, "   "), vec![0, 1, 2]);
    }

    /// 列名匹配忽略大小写；子串包含即命中
    #[test]
    fn browser_filter_matches_name_case_insensitive() {
        let (n, t, c) = sample();
        assert_eq!(filter_browser_columns(&n, &t, &c, "USER"), vec![1]);
        assert_eq!(filter_browser_columns(&n, &t, &c, "name"), vec![1]);
    }

    /// 类型名同样参与匹配（DBeaver 元数据面板只匹配列名；本轮拍板加宽到类型名 / 注释）
    #[test]
    fn browser_filter_matches_type_name() {
        let (n, t, c) = sample();
        assert_eq!(filter_browser_columns(&n, &t, &c, "varchar"), vec![1]);
        assert_eq!(filter_browser_columns(&n, &t, &c, "timestamp"), vec![2]);
    }

    /// 注释参与匹配；注释为空的列不参与注释分支
    #[test]
    fn browser_filter_matches_comment() {
        let (n, t, c) = sample();
        assert_eq!(filter_browser_columns(&n, &t, &c, "登录名"), vec![1]);
        assert_eq!(filter_browser_columns(&n, &t, &c, "主键"), vec![0]);
    }

    /// 列名 / 类型 / 注释都不命中 = 空清单（渲染层摆「无匹配列」空态）
    #[test]
    fn browser_filter_no_match_returns_empty() {
        let (n, t, c) = sample();
        assert!(filter_browser_columns(&n, &t, &c, "zzz").is_empty());
    }

    /// 页签下标 → 枚举：0=列浏览器（默认）/ 1=值详情 / 2=计算 / 3=分组，越界回落默认页
    #[test]
    fn right_tab_index_maps_to_four_tabs() {
        assert_eq!(right_tab_from_index(0), RightTab::Browser);
        assert_eq!(right_tab_from_index(1), RightTab::Value);
        assert_eq!(right_tab_from_index(2), RightTab::Stats);
        assert_eq!(right_tab_from_index(3), RightTab::Group);
        assert_eq!(right_tab_from_index(99), RightTab::Browser);
    }

    /// 【结果集 #296】空列序 = 默认序原样直通（列 / 行一字不差）
    #[test]
    fn empty_order_passes_columns_through_unchanged() {
        let columns = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let rows = vec![
            vec!["1".to_string(), "2".to_string(), "3".to_string()],
            vec!["4".to_string(), "5".to_string(), "6".to_string()],
        ];
        let (out_columns, out_rows) = reorder_by(&[], columns.clone(), rows.clone());
        assert_eq!(out_columns, columns);
        assert_eq!(out_rows, rows);
    }

    /// 【结果集 #296】按持久化列名序重排：列序与行内容同步换位
    #[test]
    fn reorder_moves_columns_and_rows_together() {
        let order = vec!["b".to_string(), "a".to_string()];
        let columns = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let rows = vec![
            vec!["1".to_string(), "2".to_string(), "3".to_string()],
            vec!["4".to_string(), "5".to_string(), "6".to_string()],
        ];
        let (out_columns, out_rows) = reorder_by(&order, columns, rows);
        assert_eq!(
            out_columns,
            vec!["b".to_string(), "a".to_string(), "c".to_string()],
            "持久化序在前，未匹配列（c）按数据序附加末尾"
        );
        assert_eq!(
            out_rows,
            vec![
                vec!["2".to_string(), "1".to_string(), "3".to_string()],
                vec!["5".to_string(), "4".to_string(), "6".to_string()],
            ],
            "行按列同序换位"
        );
    }

    /// 【结果集 #296】持久化序里的列已不在新结果里 → 只匹配存在的；全不匹配 = 数据序
    #[test]
    fn reorder_keeps_only_matching_columns() {
        let order = vec!["gone".to_string(), "x".to_string()];
        let columns = vec!["a".to_string(), "b".to_string()];
        let rows = vec![vec!["1".to_string(), "2".to_string()]];
        let (out_columns, out_rows) = reorder_by(&order, columns, rows);
        assert_eq!(out_columns, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(out_rows, vec![vec!["1".to_string(), "2".to_string()]]);
    }

    /// 【2026-10-29】结果集 UI 渲染断言（gpui-kit 测试框架 `TestAppContext` /
    /// `TestWindowExt` / `VisualTestContext::debug_bounds`）：渲染**真实面板**（非纯函数），
    /// 断言本轮微调的关键 UI 元素存在且可见——**防回退**：
    /// - 空面板根 `editor-result-root` 恒渲染；
    /// - 展开筛选条后：`editor-result-filter-expand-bar` + **两内嵌按钮**
    ///   （左=展开 `-toggle` / 右=运行 `-run`，无边框输入框）；
    /// - SQL 预览弹窗：`open_sql_dialog` 后遮罩 `editor-result-sql-dialog` 出现、
    ///   `close_sql_dialog` 后消失。
    ///
    /// 查询机制（gpui-kit 0.8.1 实测）：
    /// - `TestWindowExt.find/try_find` 只匹配**显式 ElementId** 元素
    ///   （`Button::new(id)` / `.id()` / EffectTransition 包装）——纯 `div().debug_selector()`
    ///   的 div **不注册**（`Observed::new` 要求 `Element::id().is_some()`）；
    /// - `div().debug_selector()` 走 gpui 原生 `debug_bounds` 注册路径
    ///   （`VisualTestContext::debug_bounds(&'static str)`，paint 阶段注册）；
    /// - 故 `visible()` 兜底查询两路：显式 id 走 `try_find().visible()`，
    ///   debug_selector div 走 `debug_bounds().is_some()`。
    ///
    /// 谁删了弹窗 / 改回旧展开条 / 拆掉两内嵌按钮，此测试即红。
    #[gpui_kit::test]
    fn rendered_ui_elements_survive(cx: &mut gpui_kit::TestAppContext) {
        use gpui_kit::component::Root;
        use gpui_kit::test::TestWindowExt as _;
        use gpui_kit::{AppContext as _, ElementId, Entity, IntoElement, Render, SharedString};

        cx.update(gpui_kit::init);
        let slot: std::rc::Rc<std::cell::RefCell<Option<Entity<super::ResultPanel>>>> =
            std::rc::Rc::new(std::cell::RefCell::new(None));
        let slot_in = slot.clone();
        // 宿主壳：ResultPanel 无标准 `Render`（自定义 `render(&self, cx)`，host 手动调），
        // 测试仿 host.rs 9075（`result_panel.read(cx).render(cx)`）包一层 Render 实体
        struct PanelHost {
            panel: Entity<super::ResultPanel>,
        }
        impl Render for PanelHost {
            fn render(&mut self, _window: &mut gpui_kit::Window, cx: &mut gpui_kit::Context<Self>) -> impl IntoElement {
                self.panel.read(cx).render(cx)
            }
        }
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let queue = crate::ports::ResultActionQueue::new();
            let panel = cx.new(|cx| super::ResultPanel::new(window, cx, queue));
            *slot_in.borrow_mut() = Some(panel.clone());
            let host = cx.new(|_| PanelHost { panel });
            Root::new(host, window, cx)
        });
        let panel = slot.borrow().clone().expect("ResultPanel 已创建");
        cx.update(|window, cx| window.draw(cx).clear(cx));

        let visible = |id: &'static str, cx: &mut gpui_kit::VisualTestContext| -> bool {
            let by_id = cx.update(|window, _| {
                window
                    .try_find(ElementId::Name(SharedString::from(id.to_string())))
                    .is_some_and(|snap| snap.visible())
            });
            // 无显式 ElementId 的 div（纯 debug_selector）走 gpui debug_bounds 注册路径
            by_id || cx.debug_bounds(id).is_some()
        };

        // 1) 空面板根恒渲染
        assert!(visible("editor-result-root", cx), "空面板根应渲染可见");

        // 2) 展开筛选条：两内嵌按钮（左=展开 / 右=运行）随条出现
        cx.update(|_, app| panel.update(app, |p, cx| p.toggle_filter_expand(cx)));
        cx.update(|window, cx| window.draw(cx).clear(cx));
        for id in [
            "editor-result-filter-expand-bar",
            "editor-result-filter-expand-toggle",
            "editor-result-filter-expand-run",
        ] {
            assert!(visible(id, cx), "展开筛选条元素 {id} 应渲染可见（回退检查）");
        }

        // 3) SQL 预览弹窗：open 出现 / close 消失
        cx.update(|window, app| {
            panel.update(app, |p, cx| p.open_sql_dialog(window, cx));
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visible("editor-result-sql-dialog", cx), "SQL 弹窗应渲染（回退检查）");
        cx.update(|_, app| panel.update(app, |p, cx| p.close_sql_dialog(cx)));
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert!(
            !visible("editor-result-sql-dialog", cx),
            "关闭后 SQL 弹窗应消失"
        );
    }