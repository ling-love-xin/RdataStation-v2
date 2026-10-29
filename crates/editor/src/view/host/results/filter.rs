//! 筛选 / 值预览 / 绑定（run_filter_as_query / pushdown_filter / open_value_preview / open_bind_dialog）
//!
//! 自 host/results.rs 拆分（2026-10-29 Step 7）：按能力域切分 `impl EditorHostPanel` 方法块。
//! 跨域互调经 `pub(crate)` 方法；字段在 mod.rs 统一 `pub(crate)`。

use super::super::*;
use crate::execution::ExecTarget;
use crate::store::{ResultEntry, ResultPlacement};
use crate::view::dialogs;

impl EditorHostPanel {
    /// 【1.2】筛选框运行：用 `select * from (原SQL) where 词` **替换**当前结果集重查
    /// （用户口径：刷新结果集）。词取面板**实时**筛选词（运行按钮点击时面板已定稿）。
    /// 生成的 wrapped SQL 写入面板 `exec_sql`（右栏「执行SQL」页签展示）。
    pub(crate) fn run_filter_as_query(&mut self, cx: &mut Context<Self>) {
        let value = self.result_panel.read(cx).filter_text().trim().to_string();
        if value.is_empty() {
            self.set_message(Some("先填一个筛选词再运行".to_string()), cx);
            return;
        }
        let Some(entry) = self
            .shared
            .results_active(&self.document)
            .filter(ResultEntry::has_grid)
        else {
            self.set_message(Some("没有可执行的结果集".to_string()), cx);
            return;
        };
        let wrapped = format!("select * from ({}) as _q where {}", entry.sql, value);
        self.result_panel
            .update(cx, |p, cx| p.set_exec_sql(Some(wrapped.clone()), cx));
        self.execute(ExecTarget::Statement(wrapped), ResultPlacement::Replace, cx);
    }

    /// 【B15】下发源库：把当前筛选词交给执行器拼 `WHERE` 重查（结果落**新结果集**）
    pub(crate) fn pushdown_filter(&mut self, cx: &mut Context<Self>) {
        let Some(entry) = self
            .shared
            .results_active(&self.document)
            .filter(ResultEntry::has_grid)
        else {
            self.set_message(Some("没有可下发的结果集".to_string()), cx);
            return;
        };
        let filter = self.result_panel.read(cx).filter_text().trim().to_string();
        if filter.is_empty() {
            self.set_message(Some("先填一个筛选词再下发".to_string()), cx);
            return;
        }
        self.execute(
            ExecTarget::Filtered {
                sql: entry.sql.clone(),
                filter,
                columns: entry.columns.clone(),
            },
            // 原型 §5.5：下发**产生新结果集**，原结果保留
            ResultPlacement::NewSet,
            cx,
        );
    }

    /// 【B14】按值筛选（右键菜单来的）：把值写进筛选框并立刻应用（**本地**那档）
    ///
    /// 输入框要跟着变（用户得看到自己被填了什么）；窗口句柄是构造时存的——
    /// 菜单点击发生在独立事件里，但没有 `window` 参数（headless 里甚至真的没有窗口），
    /// 拿不到就只改真值，筛选照样生效。
    pub(crate) fn apply_filter_value(&mut self, value: &str, cx: &mut Context<Self>) {
        let value = value.to_string();
        // 真值定稿（掐在途防抖 + 立即应用）不依赖窗口
        self.result_panel.update(cx, |p, cx| p.set_filter_value(&value, cx));
        // 输入框回填在窗口可用时做；拿不到（headless / 已关）就跳过，真值已生效
        let panel = self.result_panel.clone();
        let _ = self.window.update(cx, |_panel, window, app| {
            panel.update(app, |p, cx| p.sync_filter_input(&value, window, cx));
        });
    }

    /// 【筛选】列头菜单「按值筛选此列 / 清除」落点：代理到结果面板（空串 = 清除）
    pub(crate) fn apply_column_filter(&mut self, column: usize, value: &str, cx: &mut Context<Self>) {
        let value = value.to_string();
        self.result_panel
            .update(cx, |p, cx| p.apply_column_filter(column, &value, cx));
    }

    /// 【遗留 #7】右键「查看完整值」：记录目标并弹值查看器（对话框由 dialogs 层开）
    ///
    /// 钩子给的是 `&mut App`（拿不到窗口），对话框要 `&mut Window`——所以经
    /// `self.window.update` 转手调 [`Self::open_value_preview_with_window`]。
    /// headless 测试里窗口不在 `App::windows`（测试上下文持有），转手会报
    /// `window not found` 而弹不出——这是测试环境限制，真实窗口下正常。
    pub(crate) fn open_value_preview(
        &mut self,
        value: &str,
        column: &str,
        row: usize,
        cx: &mut Context<Self>,
    ) {
        self.value_preview = Some(ValuePreview {
            value: value.to_string(),
            column: column.to_string(),
            row,
        });
        let target = self.value_preview.clone().expect("刚写入");
        let result = self.window.update(cx, move |_panel, window, app| {
            // 转手闭包里 window 是真实窗口：直接弹
            Self::open_value_preview_with_window_impl(
                window,
                app,
                target.value.clone(),
                target.column.clone(),
                target.row,
            );
        });
        // 窗口句柄不可用（headless / 窗口已关）→ 留一句可读原因，不静默
        if let Err(error) = result {
            self.set_message(Some(format!("值查看器弹不出：{error}")), cx);
        }
    }

    /// 【遗留 #7】带窗口的值查看器入口（测试走它做窗口级验收；生产钩子经
    /// [`Self::open_value_preview`] 转手调同一实现）
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn open_value_preview_with_window(        &mut self,
        value: &str,
        column: &str,
        row: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.value_preview = Some(ValuePreview {
            value: value.to_string(),
            column: column.to_string(),
            row,
        });
        Self::open_value_preview_with_window_impl(
            window,
            cx,
            value.to_string(),
            column.to_string(),
            row,
        );
    }

    /// 弹层实现（与窗口来源无关）：`dialogs::open_value_preview` 是模态弹层，
    /// 复制不关弹层（用户复制完可能还要接着看）
    pub(crate) fn open_value_preview_with_window_impl(
        window: &mut Window,
        cx: &mut App,
        value: String,
        column: String,
        row: usize,
    ) {
        dialogs::open_value_preview(
            window,
            cx,
            value.clone(),
            column.clone(),
            row,
            move |app| app.write_to_clipboard(ClipboardItem::new_string(value.clone())),
        );
    }

    /// 【遗留 #7】只读访问器：值查看器目标是否已记录（测试断言用；`ValuePreview` 是私有类型，只暴露 bool）
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn value_preview_for_test(&self) -> bool {
        self.value_preview.is_some()
    }

    /// 【B16】弹参数表单（生产入口；headless 下 `self.window.update` 转手会报
    /// `window not found`——与 [`Self::open_value_preview`] 同款测试环境限制，留可读原因）
    pub(crate) fn open_bind_dialog(&mut self, names: Vec<String>, cx: &mut Context<Self>) {
        let entity = cx.entity();
        let result = self.window.update(cx, move |_panel, window, app| {
            let run_entity = entity.clone();
            let cancel_entity = entity.clone();
            dialogs::open_bind_dialog(
                window,
                app,
                names,
                move |values, _window, cx| {
                    run_entity.update(cx, |panel, cx| panel.finish_bind(values, cx));
                },
                move |cx| {
                    cancel_entity.update(cx, |panel, cx| panel.cancel_bind(cx));
                },
            );
        });
        if let Err(error) = result {
            self.set_message(Some(format!("参数表单弹不出：{error}")), cx);
        }
    }

    /// 【B16】参数表单提交：替换 `:name` 后继续原提交链（替换后无参数，直接走执行）
    pub(crate) fn finish_bind(
        &mut self,
        values: std::collections::HashMap<String, String>,
        cx: &mut Context<Self>,
    ) {
        let Some(pending) = self.pending_bind.take() else {
            return;
        };
        let target = pending.substitute(&values);
        self.execute_labeled(target, pending.placement, pending.title, cx);
    }

    /// 【B16】取消参数表单：清掉挂起（「这次不跑了」不是只关个窗）
    pub(crate) fn cancel_bind(&mut self, _cx: &mut Context<Self>) {
        self.pending_bind = None;
    }

    /// 【B16】只读访问器：参数表单是否在挂起（测试断言用）
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn pending_bind_for_test(&self) -> bool {
        self.pending_bind.is_some()
    }

    /// 【B14】排序下发（右键菜单来的）：按这一列重查源库，结果落**新结果集**
    pub(crate) fn sort_down(&mut self, column: &str, descending: bool, cx: &mut Context<Self>) {
        let Some(entry) = self
            .shared
            .results_active(&self.document)
            .filter(ResultEntry::has_grid)
        else {
            self.set_message(Some("没有可下发的结果集".to_string()), cx);
            return;
        };
        self.execute(
            ExecTarget::SortedDown {
                sql: entry.sql.clone(),
                column: column.to_string(),
                descending,
            },
            // 与筛选下发同口径：**产生新结果集**，原结果保留
            ResultPlacement::NewSet,
            cx,
        );
    }

}
