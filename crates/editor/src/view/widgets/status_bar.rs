//! 编辑器状态栏（A7 真实值 + A6 只读两维度）
//!
//! ## 只显示真实值
//!
//! 方言 / 编码 / 换行 / 缩进的数据源属连接绑定与持久化（1b / A12）；**没有数据源就不显示**，
//! 不用占位文案撑场面（对齐 connection 模块"零 UI 造数据"的口径）。
//!
//! ## 两个只读维度分别表达（A6）
//!
//! - **编辑器只读**：不可输入 → 状态栏显示「只读」（同时编辑内核也置 `readonly`）
//! - **连接只读**：可输入、写语句被拦 → 状态栏显示「连接只读」（加锁图标语义）
//!
//! 二者互不蕴含：可能出现"能编辑但不能写库"，也可能"不能编辑但连接可写"。合并成一个标志
//! 正是 v2 现状（`Shared::project_ui.read_only`）的缺陷。
//!
//! ## 为什么文案计算是纯函数
//!
//! `labels()` 不碰 GPUI：哪一段该出现、数字是多少，全部可以在单测里穷举（三条只读组合、
//! 语句数随内容变化）。`render()` 只负责把它画成一行。

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::status_bar::StatusBar;
use gpui_kit::*;

use crate::model::{EditorMode, ReadOnly};
use crate::ui;

/// 状态栏的输入（全部来自真实状态；无数据源的字段不放进结构体）
///
/// `message` 借用面板字段而不克隆：状态栏每帧都画，渲染路径不做字符串分配。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusInputs<'a> {
    pub mode: EditorMode,
    pub dirty: bool,
    pub read_only: ReadOnly,
    /// 语句数（仅 SQL 模式有意义；由内容变化时增量计算，不在渲染期扫描）
    pub statements: usize,
    /// 光标位置（1 基，与主流编辑器一致）
    pub line: usize,
    pub column: usize,
    /// 选中的字符数（0 = 无选区）
    pub selected_chars: usize,
    /// 动作失败的原因（未命名需另存为 / 有未保存改动 / 只读拒绝）；`None` = 无提示
    pub message: Option<&'a str>,
    /// 是否有执行在跑（真实状态，不猜）
    pub executing: bool,
    /// 本次执行已跑的时长（B3；`None` = 没有在跑 → 不显示耗时）
    pub elapsed: Option<std::time::Duration>,
    /// 连接段（已格式化的文案，如 `●P·orders`）；`None` = 不显示（文本模式没有连接概念）
    pub connection: Option<&'a str>,
    /// 【B4】事务区文案（`TX 未开启` / `TX 已开启 3.4s`）；`None` = 不显示
    pub tx: Option<&'a str>,
}

/// 状态栏里的可交互控件（由面板构造：它知道点击该干什么）
///
/// 文案走 [`labels`]（纯函数、可穷举），控件走这里——两者分开才能把“该显示什么”测干净。
#[derive(Default)]
pub struct StatusControls {
    /// 【B4】事务区控件（自动提交开关 + 事务开启时的提交 / 回滚），挂在**左段**
    pub tx: Option<AnyElement>,
    /// 【B3】中断按钮，挂在**右段**
    pub interrupt: Option<AnyElement>,
}

/// 状态栏文案（左右两段）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusLabels {
    pub left: String,
    pub right: String,
}

/// 计算状态栏文案（纯函数）
pub fn labels(inputs: &StatusInputs) -> StatusLabels {
    // 左：连接（会通信的模式才有）+ 事务 + 模式 + 语句数（SQL 模式才有语句概念）+ 未保存 + 动作提示
    let mut left = String::new();
    if let Some(connection) = inputs.connection {
        left.push_str(connection);
        left.push_str(" · ");
    }
    if let Some(tx) = inputs.tx {
        // 【B4】事务是**连接级**的事实，紧跟在连接段后面
        left.push_str(tx);
        left.push_str(" · ");
    }
    left.push_str(inputs.mode.short_label());
    if inputs.mode == EditorMode::Sql {
        left.push_str(&format!(" · {} 条语句", inputs.statements));
    }
    if inputs.dirty {
        left.push_str(" · 未保存");
    }
    if inputs.executing {
        // 耗时累加（原型 §4.6：执行中要看得见跑了多久——它也是“该不该中断”的判断依据）
        match inputs.elapsed {
            Some(elapsed) => left.push_str(&format!(" · 执行中 {}…", elapsed_text(elapsed))),
            None => left.push_str(" · 执行中…"),
        }
    }
    if let Some(message) = inputs.message {
        // 提示紧跟在左侧状态段之后：它是“刚才那个动作”的后果，不是文档属性
        left.push_str(&format!(" · {message}"));
    }

    // 右：两个只读维度分别表达 + 光标 + 选区
    let mut parts: Vec<String> = Vec::new();
    if inputs.read_only.editor {
        parts.push("只读".to_string());
    }
    if inputs.read_only.connection {
        parts.push("连接只读".to_string());
    }
    parts.push(format!("Ln {}, Col {}", inputs.line, inputs.column));
    if inputs.selected_chars > 0 {
        parts.push(format!("已选 {} 字", inputs.selected_chars));
    }

    StatusLabels {
        left,
        right: parts.join(" · "),
    }
}

/// 跑多久了（人读的短文案：′10s 一位小数，之后整秒，过分钟折成 `1m42s`）
pub fn elapsed_text(elapsed: std::time::Duration) -> String {
    let secs = elapsed.as_secs_f32();
    if secs < 10.0 {
        format!("{secs:.1}s")
    } else if secs < 60.0 {
        format!("{}s", secs as u64)
    } else {
        let total = secs as u64;
        format!("{}m{}s", total / 60, total % 60)
    }
}

/// 画成状态栏（固定高、顶部分隔线；颜色全部来自主题）
///
/// `controls` 里的控件由面板给（它们要拿面板实体去处理点击）。
pub fn render(
    inputs: &StatusInputs,
    controls: StatusControls,
    cx: &App,
) -> impl IntoElement {
    let text = labels(inputs);
    let theme = cx.theme();
    let muted = theme.colors.muted_foreground;
    let border = theme.colors.border;

    let left = div()
        .h_flex()
        .items_center()
        .gap_1()
        .text_xs()
        .text_color(muted)
        .child(text.left)
        .children(controls.tx);

    StatusBar::new()
        .left(left)
        .right(
            div()
                .h_flex()
                .items_center()
                .gap_1()
                .text_xs()
                .text_color(muted)
                .child(text.right)
                .children(controls.interrupt),
        )
        .h(rems(ui::EDITOR_STATUS_BAR_HEIGHT))
        .border_t(ui::HAIRLINE)
        .border_color(border)
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**（父模块引入了 gpui，`use super::*` 会把它的 `test` 宏带进来）
    use super::{StatusInputs, elapsed_text, labels};
    use crate::model::{EditorMode, ReadOnly};

    fn inputs(read_only: ReadOnly) -> StatusInputs<'static> {
        StatusInputs {
            mode: EditorMode::Sql,
            dirty: false,
            read_only,
            statements: 3,
            line: 12,
            column: 4,
            selected_chars: 0,
            message: None,
            executing: false,
            elapsed: None,
            // 连接段默认不显示；连接相关的断言在下面的专用用例里给值
            connection: None,
            tx: None,
        }
    }

    /// 连接段（B1）放在最左：读状态栏第一眼要知道“这条 SQL 会发到哪”
    #[test]
    fn connection_leads_the_left_segment() {
        let mut with_connection = inputs(ReadOnly::none());
        with_connection.connection = Some("●P·orders");
        let text = labels(&with_connection);
        assert!(
            text.left.starts_with("●P·orders · SQL"),
            "连接在前、模式在后：{}",
            text.left
        );

        // 文本模式没有连接概念：整段不出现
        let plain = labels(&inputs(ReadOnly::none()));
        assert!(!plain.left.contains('●') && !plain.left.contains("未绑定"));
    }

    #[test]
    fn left_shows_mode_and_statement_count() {
        let text = labels(&inputs(ReadOnly::none()));
        assert!(text.left.starts_with("SQL"), "{}", text.left);
        assert!(text.left.contains("3 条语句"), "语句数必须真实：{}", text.left);
        assert!(!text.left.contains("未保存"));
    }

    #[test]
    fn statement_count_follows_content() {
        let mut one = inputs(ReadOnly::none());
        one.statements = 1;
        assert!(labels(&one).left.contains("1 条语句"));

        let mut nine = inputs(ReadOnly::none());
        nine.statements = 9;
        assert!(labels(&nine).left.contains("9 条语句"));
    }

    #[test]
    fn dirty_state_is_visible() {
        let mut dirty = inputs(ReadOnly::none());
        dirty.dirty = true;
        assert!(labels(&dirty).left.contains("未保存"));
    }

    #[test]
    fn text_mode_has_no_statement_count() {
        // 文本模式是纯记事本：没有语句概念，不该出现"N 条语句"
        let mut text = inputs(ReadOnly::none());
        text.mode = EditorMode::Text;
        assert!(!labels(&text).left.contains("条语句"));
        assert!(labels(&text).left.starts_with("TXT"));
    }

    #[test]
    fn read_only_dimensions_are_reported_separately() {
        // 三种组合：都不只读 / 仅编辑器 / 仅连接 / 两者
        let none = labels(&inputs(ReadOnly::none()));
        assert!(!none.right.contains("只读"));

        let editor = labels(&inputs(ReadOnly::editor_only()));
        assert!(editor.right.contains("只读"), "{}", editor.right);
        assert!(
            !editor.right.contains("连接只读"),
            "编辑器只读不等于连接只读：{}",
            editor.right
        );

        let connection = labels(&inputs(ReadOnly {
            editor: false,
            connection: true,
        }));
        assert!(connection.right.contains("连接只读"));
        assert!(
            !connection.right.contains("· 只读 ·"),
            "连接只读不等于编辑器只读：{}",
            connection.right
        );

        let both = labels(&inputs(ReadOnly {
            editor: true,
            connection: true,
        }));
        assert!(both.right.contains("只读") && both.right.contains("连接只读"));
    }

    #[test]
    fn cursor_and_selection_are_shown_with_real_values() {
        let text = labels(&inputs(ReadOnly::none()));
        assert!(text.right.contains("Ln 12, Col 4"), "{}", text.right);
        assert!(!text.right.contains("已选"), "没有选区就不显示选区段");

        let mut selecting = inputs(ReadOnly::none());
        selecting.selected_chars = 7;
        assert!(labels(&selecting).right.contains("已选 7 字"));
    }

    #[test]
    fn failed_actions_leave_a_visible_reason() {
        // 动作失败必须在状态栏留痕迹（“按了没反应”不可接受）；提示属于**左段**
        let mut failed = inputs(ReadOnly::none());
        failed.dirty = true;
        failed.message = Some("未命名文档，请先另存为");
        let text = labels(&failed);
        assert!(text.left.contains("未命名文档，请先另存为"), "{}", text.left);
        assert!(text.left.contains("未保存"), "提示不挤掉文档状态：{}", text.left);
        assert!(!text.right.contains("另存为"), "提示不进右段：{}", text.right);

        // 无提示时不显示占位
        assert!(!labels(&inputs(ReadOnly::none())).left.contains("另存为"));
    }

    /// 执行中要看得见“跑了多久”（B3）：它是判断该不该中断的依据
    #[test]
    fn executing_segment_counts_the_time_up() {
        let mut running = inputs(ReadOnly::none());
        running.executing = true;
        assert!(labels(&running).left.contains("执行中…"), "没耗时也得说在执行");

        running.elapsed = Some(std::time::Duration::from_millis(3400));
        let text = labels(&running).left;
        assert!(text.contains("执行中 3.4s…"), "{text}");

        // 不在执行时不留耗时（它是执行态的一部分，不是文档属性）
        let idle = inputs(ReadOnly::none());
        assert!(!labels(&idle).left.contains("执行中"));
    }

    #[test]
    fn elapsed_text_is_short_and_readable() {
        use std::time::Duration;
        assert_eq!(elapsed_text(Duration::from_millis(400)), "0.4s");
        assert_eq!(elapsed_text(Duration::from_millis(9_900)), "9.9s");
        assert_eq!(elapsed_text(Duration::from_secs(10)), "10s");
        assert_eq!(elapsed_text(Duration::from_secs(59)), "59s");
        assert_eq!(elapsed_text(Duration::from_secs(102)), "1m42s");
    }

    /// 【B4】事务区跟着连接段（它是连接级事实），没给就不占位
    #[test]
    fn the_transaction_segment_follows_the_connection() {
        let mut with_tx = inputs(ReadOnly::none());
        with_tx.connection = Some("●P·orders");
        with_tx.tx = Some("TX 未开启");
        let text = labels(&with_tx).left;
        assert!(
            text.starts_with("●P·orders · TX 未开启 · SQL"),
            "事务紧随连接、在模式之前：{text}"
        );

        let mut running = inputs(ReadOnly::none());
        running.tx = Some("TX 已开启 3.4s");
        assert!(labels(&running).left.contains("TX 已开启 3.4s"));

        // 没有事务信息（未接执行）时不出现占位文案
        assert!(!labels(&inputs(ReadOnly::none())).left.contains("TX"));
    }
}
