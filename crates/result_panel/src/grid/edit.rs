//! 编辑域（`grid/` 第三块）：编辑态生命周期（`begin_edit` / `commit_edit` /
//! `cancel_edit` / 编辑框渲染）+ 提交值类型校验。

use gpui_kit::*;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::table::TableState;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder as _;

use crate::rowset::RowRef;
use crate::store::ColumnKind;
use super::delegate::{CellEdit, CommitEditHook, CommitLocalEditHook, ResultGridDelegate, RowActionHook};
use workbench_shell::product_tokens;
impl ResultGridDelegate {
    // ---------- 【T2a-4】单元格编辑 ----------

    /// 这份结果能否编辑（面板同步数据时算；`false` 时双击不进入编辑）
    pub fn set_editable(&mut self, editable: bool) {
        self.editable = editable;
    }

    /// 编辑框实体（宿主构造时创建、同步数据时注入；编辑框渲染用同一个实体）
    pub fn set_edit_input(&mut self, input: Entity<InputState>) {
        self.edit_input = Some(input);
    }

    /// 提交回调（宿主注入：落变更集）
    pub fn set_commit_edit_hook(&mut self, hook: CommitEditHook) {
        self.on_commit_edit = Some(hook);
    }

    /// 【#28-B4】本地行格编辑提交回调（宿主注入：写回本地行快照，不进变更集 / 撤销栈）
    pub fn set_commit_local_edit_hook(&mut self, hook: CommitLocalEditHook) {
        self.on_commit_local_edit = Some(hook);
    }

    /// 【#28-B3】注入行级动作钩子（新增 / 复制 / 删除一行交回宿主分派）
    pub fn set_row_action_hook(&mut self, hook: RowActionHook) {
        self.on_row_action = Some(hook);
    }

    /// 【T2a-3】脏格集合（面板从权威变更集投影；**数据行序**，含数据列序）
    pub fn set_dirty(&mut self, dirty: Vec<(usize, usize)>) {
        self.dirty = dirty;
    }

    /// 【T2a-6】冲突格集合（提交回执标过 `Conflict`；面板从权威变更集投影）
    pub fn set_conflict(&mut self, conflict: Vec<(usize, usize)>) {
        self.conflict = conflict;
    }

    /// 这一格是不是脏的（视图行序 + 组件列序 → 数据行 / 数据列比较）
    pub(crate) fn is_dirty(&self, row_ix: usize, col_ix: usize) -> bool {
        // 【P2-4】转置是只读视图，没有待提交的变更
        if self.transposed {
            return false;
        }
        if Self::is_row_number(col_ix) {
            return false;
        }
        let Some(data_ix) = self.data_row(row_ix) else {
            return false;
        };
        self.dirty.contains(&(data_ix, self.data_column(col_ix - 1)))
    }

    /// 这一格是不是冲突的（提交没写进去；与脏格分开标色）
    pub(crate) fn is_conflict(&self, row_ix: usize, col_ix: usize) -> bool {
        // 【P2-4】转置是只读视图，没有提交冲突
        if self.transposed {
            return false;
        }
        if Self::is_row_number(col_ix) {
            return false;
        }
        let Some(data_ix) = self.data_row(row_ix) else {
            return false;
        };
        self.conflict
            .contains(&(data_ix, self.data_column(col_ix - 1)))
    }

    /// 这一格是否正在编辑（视图行序 + 组件列序匹配）
    pub fn is_editing(&self, row_ix: usize, col_ix: usize) -> bool {
        // 【P2-4】转置是只读视图，不进编辑
        if self.transposed {
            return false;
        }
        if Self::is_row_number(col_ix) {
            return false;
        }
        self.editing.as_ref().is_some_and(|edit| {
            edit.view_row == row_ix && self.view_column(edit.column) == Some(col_ix - 1)
        })
    }

    /// 进入编辑：只设状态（初值同步 / 聚焦由 render_td 首次渲染时一次性做）。
    /// 返回是否真的进入了（不可编辑 / 点行号槽 / 行越界 / 被标记删除 = `false`）。
    pub fn begin_edit(&mut self, view_row: usize, col_ix: usize) -> bool {
        // 【P2-4】转置是只读视图，双击不进入编辑
        if self.transposed || !self.editable || self.editing.is_some() || Self::is_row_number(col_ix)
        {
            return false;
        }
        let Some(reference) = self.row_ref(view_row) else {
            return false;
        };
        // 【#28-B4】被标记删除的底层行不再进编辑（撤销删除后才恢复可编辑）
        if let RowRef::Data(data_ix) = reference {
            if self
                .local
                .as_ref()
                .is_some_and(|set| set.is_deleted(data_ix))
            {
                return false;
            }
        }
        let column = self.data_column(col_ix - 1);
        let original = self
            .row_cell_text(view_row, col_ix)
            .unwrap_or_default()
            .to_string();
        self.editing = Some(CellEdit {
            view_row,
            reference,
            column,
            value: original.clone(),
            original,
            settled: false,
            error: None,
        });
        true
    }

    /// 取消编辑（Esc）：丢弃输入值回到非编辑态。**不**动变更集。
    pub fn cancel_edit(&mut self) {
        self.editing = None;
    }

    /// 提交编辑（Enter / 失焦）：取当前输入值走钩子交回面板。
    /// 这里先清编辑态——面板落变更集后，编辑框不再渲染，InputState 失焦
    /// 触发的 Blur 看到编辑态已空，不会重复提交。
    ///
    /// 【类型感知】提交前按列类型校验（数字列必须数字、布尔列取值受限…）；
    /// 非法 = **拒绝提交**（编辑态保留 + `error` 记录，渲染红框提示），返回 `false`。
    /// 返回 `true` 才真的交了。调用方（宿主 Enter / Blur 订阅）拿返回值决定要不要
    /// 投影刷新：失败时只重绘（显示错误），不去跑整轮结果投影。
    pub fn commit_edit(&mut self) -> bool {
        // 取编辑态先只读：校验失败要把错误写回它，不能 take 后再补
        let Some(edit) = &self.editing else {
            return false;
        };
        let kind = self
            .column_types
            .get(edit.column)
            .and_then(|t| t.as_ref())
            .map(|t| t.kind);
        if let Some(kind) = kind {
            if let Some(message) = validate_edit_value(kind, &edit.value) {
                self.editing.as_mut().unwrap().error = Some(message);
                return false;
            }
        }
        let Some(edit) = self.editing.take() else {
            return false;
        };
        match edit.reference {
            // 底层行：走变更集钩子（原值 / 新值一并交回，面板据此 upsert 或移除）
            RowRef::Data(data_row) => {
                if let Some(hook) = &self.on_commit_edit {
                    hook(data_row, edit.column, edit.original, edit.value);
                }
            }
            // 【#28-B4】本地行：走本地快照钩子；值没变就不落（避免无谓重投影），
            // 不进 ChangeSet、不进格级 / 行级撤销栈（#28-B4 拍板口径）
            RowRef::Local(local_id) => {
                if edit.original != edit.value {
                    if let Some(hook) = &self.on_commit_local_edit {
                        hook(local_id, edit.column, edit.value);
                    }
                }
            }
        }
        true
    }

    /// 编辑框输入值同步（`InputEvent::Change` 时由宿主调；不区分“谁发的”）。
    /// 输入一变化就清掉上次的提交错误——用户已经动手改，红框该撤。
    pub fn sync_edit_value(&mut self, value: String) {
        if let Some(edit) = &mut self.editing {
            edit.value = value;
            edit.error = None;
        }
    }

    /// 当前编辑态（render_td 画编辑框用；借读不拷贝）
    pub fn editing(&self) -> Option<&CellEdit> {
        self.editing.as_ref()
    }

    /// 编辑框实体（宿主与测试聚焦 / 读值时用；未进入编辑也可能已注入——实体是复用的）
    pub fn edit_input(&self) -> Option<Entity<InputState>> {
        self.edit_input.clone()
    }

    /// 标记编辑框已完成“初值同步 + 聚焦”（render_td 首次渲染时调，防重复）
    pub(crate) fn mark_edit_settled(&mut self) {
        if let Some(edit) = &mut self.editing {
            edit.settled = true;
        }
    }

    /// 【T2a-4】编辑框（正在编辑的那一格）：`Input` 组件 + 初值同步 / 聚焦（幂等一次）+
    /// Escape 取消。渲染位仍是原生的 `render_td`（属于 `TableDelegate` 的调用点）；
    /// Enter 提交走 `InputEvent::PressEnter` 订阅（宿主接），Esc 因输入内核默认
    /// `cx.propagate()`（不消费、不清值）能传到外层这里。
    ///
    /// 为什么放在普通 impl 而不是 trait impl：这是网格**自己的**渲染细节，不是
    /// `TableDelegate` 接口的一部分（组件库不关心编辑框长什么样）。
    pub(crate) fn render_edit_cell(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let state = self.edit_input.clone().expect("宿主必注入编辑框实体");
        // 幂等一次性：首次渲染时同步初值（`set_value` **不发** `InputEvent::Change`，
        // 不会把“用户改了才做”的副作用重跑一遍）。
        // 聚焦**不在这里做**（render 是纯读阶段，`window.focus` 不可靠）：由宿主在
        // 进入编辑的事件路径给（`host.rs` 双击分支），焦点事件在下一帧 draw 时派发。
        if self.editing().is_some_and(|edit| !edit.settled) {
            let initial = self
                .editing()
                .map(|edit| edit.value.clone())
                .unwrap_or_default();
            state.update(cx, |state, cx| state.set_value(initial, window, cx));
            self.mark_edit_settled();
        }
        let theme = cx.theme();
        // 【#28-B4】本地行编辑框沿用本地行底色；被删行不会进入编辑（begin_edit 已拒）。
        let edit_bg = if self.row_is_local(row_ix) {
            product_tokens::get(cx).result_local_row_background(theme)
        } else {
            theme.colors.background
        };
        // 【类型感知】提交校验失败的错误（红框 + 悬停提示）；无错误 = 与今天完全一致
        let error = self.editing().and_then(|edit| edit.error.clone());
        let danger = theme.colors.danger;
        // Escape：输入内核默认 `cx.propagate()`（不消费、不清值），在这里拦下 → 取消编辑。
        // 元素回调没有实体参数，把表格实体抓到闭包里再 update。
        let table = cx.entity().clone();
        let input = Input::new(&state)
            .id(format!("editor-result-cell-input-{row_ix}-{col_ix}"))
            .w_full()
            .h_full()
            .bordered(false)
            .bg(edit_bg);
        div()
            .id(format!("editor-result-edit-cell-{row_ix}-{col_ix}"))
            .size_full()
            .h_full()
            .min_w_0()
            .flex()
            .items_center()
            .on_key_down(move |event: &KeyDownEvent, _window, cx| {
                // gpui 的键名是字符串（`Keystroke.key`），与 keymap 绑定一致：`"escape"`
                if event.keystroke.key == "escape" {
                    table.update(cx, |state, cx| {
                        state.delegate_mut().cancel_edit();
                        cx.notify();
                    });
                }
            })
            .when_some(error, |cell, message| {
                // 提交被拒：1px 红框 + 悬停看错误文案（单元格高度是行高，文本没地方放）
                cell.border_1()
                    .border_color(danger)
                    .tooltip(move |window, cx| Tooltip::new(message.clone()).build(window, cx))
            })
            .child(input)
    }
}
/// 【类型感知】按列类型校验编辑值（提交前调用；空 / `NULL` 一律放行——清掉
/// 这一格是“删值”语义，不该被类型挡住）。返回错误文案（`None` = 合法）。
pub fn validate_edit_value(kind: ColumnKind, value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value == "NULL" {
        return None;
    }
    match kind {
        ColumnKind::Integer => value
            .parse::<i64>()
            .map(|_| ())
            .map_err(|_| "必须是整数（如 42）".to_string())
            .err(),
        ColumnKind::Float | ColumnKind::Decimal => value
            .parse::<f64>()
            .map(|_| ())
            .map_err(|_| "必须是数字（如 3.14）".to_string())
            .err(),
        ColumnKind::Boolean => {
            if matches!(
                value.to_lowercase().as_str(),
                "true" | "false" | "1" | "0" | "t" | "f" | "yes" | "no"
            ) {
                None
            } else {
                Some("布尔列只能填 true / false".to_string())
            }
        }
        ColumnKind::Uuid => {
            if looks_like_uuid(value) {
                None
            } else {
                Some(
                    "UUID 格式应为 8-4-4-4-12（如 550e8400-e29b-41d4-a716-446655440000）"
                        .to_string(),
                )
            }
        }
        ColumnKind::Date => {
            if looks_like_date(value) {
                None
            } else {
                Some("日期格式应为 YYYY-MM-DD".to_string())
            }
        }
        // Text / Timestamp / Time / Json / Binary / Unknown：不校验（文本自由、
        // 时刻格式各驱动不一、JSON / 二进制自由文本——交给源库自己判断）
        _ => None,
    }
}
/// UUID 格式探针（不引 uuid crate：只查形状——`8-4-4-4-12` hex，大小写皆可）
pub(crate) fn looks_like_uuid(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    [8, 13, 18, 23].iter().all(|&at| bytes[at] == b'-')
        && bytes
            .iter()
            .enumerate()
            .filter(|(i, _)| ![8, 13, 18, 23].contains(i))
            .all(|(_, b)| b.is_ascii_hexdigit())
}

/// 日期格式探针（`YYYY-MM-DD`，粗查形状 + 月 / 日范围；不引 chrono）
pub(crate) fn looks_like_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    if !bytes.iter().enumerate().all(|(i, b)| {
        i == 4 || i == 7 || b.is_ascii_digit()
    }) {
        return false;
    }
    let month = (bytes[5] - b'0') * 10 + (bytes[6] - b'0');
    let day = (bytes[8] - b'0') * 10 + (bytes[9] - b'0');
    (1..=12).contains(&month) && (1..=31).contains(&day)
}
