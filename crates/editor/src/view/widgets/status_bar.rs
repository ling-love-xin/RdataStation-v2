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

use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::Icon;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::status_bar::StatusBar;
use gpui_kit::prelude::FluentBuilder as _;
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
    /// 【T2c-3】光标全文偏移（字符；状态栏「Ch」微标）
    pub cursor_offset: usize,
    /// 【T2c-3 收尾】光标数（列编辑 = 多光标；`> 1` 时右段显「N 光标」徽标，真实值）
    pub cursor_count: usize,
    /// 【T2c-3】缩进宽度（内核 TabSize，状态栏「Sp」微标；真实值）
    pub tab_size: usize,
    /// 【T2c-4】缩进用制表符（`true` 时状态栏微标显「Tab」而非「Sp n」）
    pub hard_tabs: bool,
    /// 【2c】保存格式（编码 + 换行；状态栏右段微标真实值）
    pub format: crate::encoding::SaveFormat,
    /// 【T2c-3】文档字节数（状态栏「大小」微标；O(1)）
    pub bytes: usize,
    /// 选中的字符数（0 = 无选区）
    pub selected_chars: usize,
    /// 【原型 §2.5】选中的行数（`Row N` 微标；0 = 无选区，不显示）——选中跨行数，
    /// 与 `selected_chars` 同源（`selected_range` 首尾偏移 → 行号差 + 1）
    pub selected_rows: usize,
    /// 动作失败的原因（未命名需另存为 / 有未保存改动 / 只读拒绝）；`None` = 无提示
    pub message: Option<&'a str>,
    /// 是否有执行在跑（真实状态，不猜）
    pub executing: bool,
    /// 本次执行已跑的时长（B3；`None` = 没有在跑 → 不显示耗时）
    pub elapsed: Option<std::time::Duration>,
    /// 连接段（已格式化的文案，如 `●P·orders`）；`None` = 不显示（文本模式没有连接概念）
    pub connection: Option<&'a str>,
    /// 【T2c-2】方言段（`方言 PostgreSQL`；从绑定连接的 db_type 解析）；`None` = 不显示
    pub dialect: Option<&'a str>,
    /// 【B13】执行通道段（`通道 源库` / `通道 本地加速（快照）`）；`None` = 不显示
    pub channel: Option<&'a str>,
    /// 【2b】通用模式的执行环境段（`环境 Python 3.12`）；`None` = 不显示（非通用模式 / 未选）
    pub env: Option<&'a str>,
    /// 【2b/遗留 #14】通用模式的脚本文件类型段（`类型 Python`）；`None` = 不显示
    /// （非通用模式 / 非脚本扩展名）。判定走 `mode::script_file_type`，展示类型名；
    /// 上色本体由 `view::highlight` 按类型分派词法器（CSV 走表格展示不上色）。
    pub file_type: Option<&'a str>,
    /// 【LSP】通用模式的语言服务状态段（`LSP 就绪` / `LSP 未就绪`）；`None` = 不显示
    /// （非通用模式 / 该文档类型不需要语言服务）
    pub lsp: Option<&'a str>,
    /// 【B4】事务区文案（`TX 未开启` / `TX 已开启 3.4s`）；`None` = 不显示
    pub tx: Option<&'a str>,
}

/// 状态栏里的可交互控件（由面板构造：它知道点击该干什么）
///
/// 文案走 [`labels`]（纯函数、可穷举），控件走这里——两者分开才能把“该显示什么”测干净。
#[derive(Default)]
pub struct StatusControls {
    /// 【T2c-1】连接 ▸ 库 ▸ Schema 面包屑（可点切换；有它时替代文本连接段——它自己带连接信息）
    pub breadcrumb: Option<AnyElement>,
    /// 【T2c-3】行｜列编辑切换入口（PopupMenu；列编辑内核多光标 API 未公开 → 列项置灰标注）
    pub edit_mode: Option<AnyElement>,
    /// 【T2c-4】重设菜单入口（编码 / 换行 / 缩进；缩进真改内核 TabSize，编码换行置灰标注）
    pub reset_menu: Option<AnyElement>,
    /// 【B4】事务区控件（自动提交开关 + 事务开启时的提交 / 回滚），挂在**左段**
    pub tx: Option<AnyElement>,
    /// 【B3】中断按钮，挂在**右段**
    pub interrupt: Option<AnyElement>,
}

/// 状态栏文案（左右两段）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusLabels {
    /// 连接段（`●P·orders` / `○ 未绑定连接`）；`None` = 无连接概念（文本模式）
    ///
    /// 渲染时若有面包屑控件，这一段被控件替代（控件自己显示 连接 ▸ 库 ▸ Schema）。
    pub connection: Option<String>,
    /// 方言段（`方言 PostgreSQL`）；`None` = 无绑定连接（不占位）
    pub dialect: Option<String>,
    /// 左段正文（通道 · 事务 · 环境族 · 未保存 · 执行中 · 提示）
    pub body: String,
    /// 右段微标（VSCode 式独立分段：定位 · 选区 · 编码 · 换行 · 缩进 · 大小 · 语句数 ·
    /// 模式 · 脏……每一项一个元素，而不是拼成一个长字符串）
    ///
    /// 渲染层把每一项画成独立微标（独立 `div` + 分隔），互不挤占：
    /// 宽窗口全显，窄窗口从左裁（最不重要的先走）而不是整段消失。
    pub right: Vec<String>,
}

/// 右段单个微标：图标 + 文案（VSCode 式：语义徽标带图标、数据徽标纯文字）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusBadge {
    pub icon: Option<IconName>,
    pub text: String,
}

/// 计算右段微标（纯函数，**唯一真相源**）
///
/// [`labels`] 的 `right` 从这里派生（只取文案，657 条测试断字符串不受影响）；
/// `render` 用它画图标 + 文案。语义徽标（只读 / 连接只读 / 脏 / 模式 / 语句数）图标化，
/// 数据微标（定位 / 选区 / 编码 / 换行 / 缩进 / 大小）保留文字——VSCode 同款。
pub fn right_badges(inputs: &StatusInputs) -> Vec<StatusBadge> {
    let mut badges: Vec<StatusBadge> = Vec::new();
    // 两个只读维度分别表达（图标语义：编辑器只读 = 锁，连接只读 = 锁 + 数据库）
    if inputs.read_only.editor {
        badges.push(StatusBadge {
            icon: Some(IconName::Lock),
            text: "只读".to_string(),
        });
    }
    if inputs.read_only.connection {
        badges.push(StatusBadge {
            icon: Some(IconName::Lock),
            text: "连接只读".to_string(),
        });
    }
    // 定位（VSCode `Ln 1, Col 1`；数据微标，纯文字）
    badges.push(StatusBadge {
        icon: None,
        text: format!("Ln {}, Col {}", inputs.line, inputs.column),
    });
    // 选区：选中字符（Ch）+ 选中行（Row）——原型 §2.5 的 `Ch 0` · `Row 1`；无选区不摆
    if inputs.selected_chars > 0 {
        badges.push(StatusBadge {
            icon: None,
            text: format!("Ch {}", inputs.selected_chars),
        });
    }
    if inputs.selected_rows > 0 {
        badges.push(StatusBadge {
            icon: None,
            text: format!("Row {}", inputs.selected_rows),
        });
    }
    // 编码 / 换行 / 缩进 / 大小：文档级保存格式（2c：打开按磁盘检测、菜单可切），独立微标
    badges.push(StatusBadge {
        icon: None,
        text: inputs.format.encoding.short_label().to_string(),
    });
    badges.push(StatusBadge {
        icon: None,
        text: inputs.format.eol.label().to_string(),
    });
    badges.push(StatusBadge {
        icon: None,
        text: if inputs.hard_tabs {
            "Tab".to_string()
        } else {
            format!("Spaces: {}", inputs.tab_size)
        },
    });
    badges.push(StatusBadge {
        icon: None,
        text: human_size(inputs.bytes),
    });
    // SQL 模式：语句数（原型 ⑧ 状态栏的 `2 条语句`；语义徽标带图标）
    if inputs.mode == EditorMode::Sql {
        badges.push(StatusBadge {
            icon: Some(IconName::Hash),
            text: format!("{} 条语句", inputs.statements),
        });
    }
    // 模式（原型 ⑧ 状态栏末尾的 `SQL`；语义徽标带图标，与工具栏模式选择器同源）
    badges.push(StatusBadge {
        icon: Some(crate::toolbar::mode_icon(inputs.mode)),
        text: inputs.mode.short_label().to_string(),
    });
    if inputs.dirty {
        badges.push(StatusBadge {
            icon: Some(IconName::Circle),
            text: "脏".to_string(),
        });
    }
    // 多光标（列编辑）徽标：真实光标数，行编辑模式下恒为 1 不显示
    if inputs.cursor_count > 1 {
        badges.push(StatusBadge {
            icon: None,
            text: format!("{} 光标", inputs.cursor_count),
        });
    }

    badges
}

/// 计算状态栏文案（纯函数）
pub fn labels(inputs: &StatusInputs) -> StatusLabels {
    // 连接段独立成一段：面包屑控件渲染时用它替代（控件显示 连接▸库▸Schema，比纯文本信息量更大）
    let connection = inputs.connection.map(str::to_string);
    // 方言段独立成一段：它跟着「绑定连接」走，面包屑控件里不含它（连接▸库▸schema 是路径，方言是引擎）
    let dialect = inputs.dialect.map(|d| format!("方言 {d}"));

    // 左段正文：通道 + 事务 + 环境族（通用模式）+ 未保存 + 执行中 + 提示
    let mut body = String::new();
    if let Some(channel) = inputs.channel {
        // 【B13】通道紧跟连接：同一个连接上，源库直连与本地加速看的是两份数据
        body.push_str(channel);
        body.push_str(" · ");
    }
    if let Some(tx) = inputs.tx {
        // 【B4】事务是**连接级**的事实，紧跟在连接段后面
        body.push_str(tx);
        body.push_str(" · ");
    }
    // 【2b】通用模式：环境段紧跟模式（`通用 · 环境 Python 3.12`）；未选环境如实不摆
    if let Some(env) = inputs.env {
        body.push_str(env);
        body.push_str(" · ");
    }
    // 【2b/开放决定 #2】通用模式：脚本文件类型段紧跟环境（`通用 · 类型 Python`）；非脚本扩展名不摆
    if let Some(file_type) = inputs.file_type {
        body.push_str(file_type);
        body.push_str(" · ");
    }
    // 【LSP】通用模式：语言服务状态段（原型 §2.5「环境 · 文件类型 · LSP」）；
    // `None` = 该文档不需要语言服务（不是“服务关了”）
    if let Some(lsp) = inputs.lsp {
        body.push_str(lsp);
        body.push_str(" · ");
    }
    if inputs.dirty {
        body.push_str("未保存 · ");
    }
    if inputs.executing {
        // 耗时累加（原型 §4.6：执行中要看得见跑了多久——它也是“该不该中断”的判断依据）
        match inputs.elapsed {
            Some(elapsed) => body.push_str(&format!("执行中 {}… · ", elapsed_text(elapsed))),
            None => body.push_str("执行中… · "),
        }
    }
    if let Some(message) = inputs.message {
        // 提示紧跟在左侧状态段之后：它是“刚才那个动作”的后果，不是文档属性
        body.push_str(&format!("{message} · "));
    }
    // 去掉尾部的「 · 」（有内容时；全空则保持空串——渲染层据此判断不摆空段）
    if body.ends_with(" · ") {
        body.truncate(body.len() - " · ".len());
    }

    // 右段微标（VSCode 式独立分段，原型 §2.5）：每项一个元素，窄窗口从左裁。
    // 文案与图标都来自 `right_badges`（唯一真相源）；`labels` 只取文案，
    // `render` 画图标 + 文案——两者不各写一遍语义判断。
    let right = right_badges(inputs)
        .into_iter()
        .map(|b| b.text)
        .collect::<Vec<_>>();

    StatusLabels {
        connection,
        dialect,
        body,
        right,
    }
}

/// 【T2c-3】字节数的人读微标（`<1K` / `1.2K` / `3.4M`；纯函数，可穷举）
pub fn human_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{}B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1}K", bytes as f32 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1}M", bytes as f32 / (1024.0 * 1024.0))
    } else {
        format!("{:.1}G", bytes as f32 / (1024.0 * 1024.0 * 1024.0))
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
    let badges = right_badges(inputs);
    let theme = cx.theme();
    let muted = theme.colors.muted_foreground;
    let border = theme.colors.border;

    // 面包屑优先于文本连接段（它自己显示 连接 ▸ 库 ▸ Schema，比纯文本信息量更大）
    let has_breadcrumb = controls.breadcrumb.is_some();
    let left = div()
        .h_flex()
        .items_center()
        .gap_1()
        .text_xs()
        .text_color(muted)
        .flex_1()
        .min_w_0()
        .children(controls.breadcrumb)
        .when(!has_breadcrumb, |row| {
            row.child(text.connection.unwrap_or_default())
        })
        // 方言段紧跟连接（面包屑/文本连接段之后）：它是「这个连接是什么引擎」，不是路径
        .when(text.dialect.is_some(), |row| {
            row.child(text.dialect.clone().unwrap())
        })
        // 正文超宽时收缩截断而不是把右段推出窗口（VSCode 左段同款行为）
        .child(
            div()
                .min_w_0()
                .flex_1()
                .text_ellipsis()
                .child(text.body),
        )
        .children(controls.tx);

    // 右段微标（VSCode 式）：每个微标独立成段，段间用竖线分隔；宽窗口全显、
    // 窄窗口从左裁（`flex_none` + 容器 `overflow_hidden`）——不会整段消失。
    // 语义徽标（只读 / 连接只读 / 模式 / 语句数 / 脏）带图标，数据微标纯文字。
    let mut right = div()
        .h_flex()
        .items_center()
        .gap_1()
        .text_xs()
        .text_color(muted)
        .flex_none()
        .children(controls.edit_mode)
        .children(controls.reset_menu);
    for (index, badge) in badges.iter().enumerate() {
        if index > 0 {
            right = right.child(
                div()
                    .h(rems(ui::EDITOR_STATUS_BAR_HEIGHT * 0.5))
                    .w_px()
                    .bg(border),
            );
        }
        let mut content = div().h_flex().items_center().gap_0p5();
        if let Some(icon) = badge.icon {
            content = content.child(Icon::new(icon).size_3().text_color(muted));
        }
        content = content.child(badge.text.clone());
        let badge = div()
            .flex_none()
            .debug_selector(move || format!("status-badge-{}", index))
            .child(content);
        right = right.child(badge);
    }
    right = right.children(controls.interrupt);

    StatusBar::new()
        .left(left)
        .right(right)
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
            cursor_offset: 42,
            cursor_count: 1,
            tab_size: 2,
            hard_tabs: false,
            format: crate::encoding::SaveFormat::default(),
            bytes: 1536,
            selected_chars: 0,
            selected_rows: 0,
            message: None,
            executing: false,
            elapsed: None,
            // 连接段默认不显示；连接相关的断言在下面的专用用例里给值
            connection: None,
            // 方言段默认不显示（没绑定连接）；方言相关的断言在专用用例里给值
            dialect: None,
            channel: None,
            env: None,
            file_type: None,
            lsp: None,
            tx: None,
        }
    }

    /// 连接段（B1）独立成一段：面包屑控件渲染时用它替代（控件显示 连接▸库▸Schema）
    #[test]
    fn connection_segment_is_separable_and_leads() {
        let mut with_connection = inputs(ReadOnly::none());
        with_connection.connection = Some("●P·orders");
        let text = labels(&with_connection);
        assert_eq!(text.connection.as_deref(), Some("●P·orders"));
        assert!(
            !text.body.starts_with("SQL"),
            "模式已移右段微标，正文不再从模式开始（连接已拆出去）：{}",
            text.body
        );

        // 文本模式没有连接概念：连接段不出现
        let plain = labels(&inputs(ReadOnly::none()));
        assert!(plain.connection.is_none(), "无连接时不占位");
    }

    #[test]
    fn left_shows_channel_and_env_family() {
        let text = labels(&inputs(ReadOnly::none()));
        assert!(!text.body.contains("条语句"), "语句数已移右段：{}", text.body);
        assert!(!text.body.contains("未保存"));
        // 模式与语句数在右段微标（原型 ⑧：… │ 2 条语句 │ SQL │ 脏）
        assert!(text.right.iter().any(|b| b.contains("SQL")), "{:?}", text.right);
        assert!(
            text.right.iter().any(|b| b.contains("3 条语句")),
            "{:?}",
            text.right
        );
    }

    #[test]
    fn statement_count_follows_content() {
        let mut one = inputs(ReadOnly::none());
        one.statements = 1;
        assert!(labels(&one).right.iter().any(|b| b.contains("1 条语句")));

        let mut nine = inputs(ReadOnly::none());
        nine.statements = 9;
        assert!(labels(&nine).right.iter().any(|b| b.contains("9 条语句")));
    }

    #[test]
    fn dirty_state_is_visible() {
        let mut dirty = inputs(ReadOnly::none());
        dirty.dirty = true;
        assert!(labels(&dirty).body.contains("未保存"));
    }

    /// 【T2c-3 收尾】多光标徽标：列编辑（多光标）时才出现，且数字是真实值；
    /// 行编辑（单光标）恒为 1，不显示占位。
    #[test]
    fn multi_cursor_badge_shows_only_when_count_above_one() {
        let single = labels(&inputs(ReadOnly::none()));
        assert!(
            !single.right.iter().any(|b| b.contains("光标")),
            "单光标不显示徽标：{:?}",
            single.right
        );

        let mut multi = inputs(ReadOnly::none());
        multi.cursor_count = 3;
        let text = labels(&multi);
        assert!(
            text.right.iter().any(|b| b.contains("3 光标")),
            "{:?} 应含 3 光标",
            text.right
        );
    }

    #[test]
    fn general_mode_has_no_statement_count() {
        // 通用模式是纯记事本（无连接档）：没有语句概念，不该出现"N 条语句"
        let mut text = inputs(ReadOnly::none());
        text.mode = EditorMode::General;
        assert!(!labels(&text).right.iter().any(|b| b.contains("条语句")));
        // 模式名在右段微标：通用模式标「通用」
        assert!(labels(&text).right.iter().any(|b| b.contains("通用")));
    }

    /// 【LSP】通用模式左侧环境族第三项：语言服务状态段（原型 §2.5「环境 · 文件类型 · LSP」）
    #[test]
    fn general_mode_shows_the_lsp_segment_only_when_supplied() {
        // 没接端口 / 文档不需要语言服务：不摆（不是“服务关了”）
        let plain = labels(&inputs(ReadOnly::none()));
        assert!(!plain.body.contains("LSP"), "{}", plain.body);

        // 通用模式 + 配了语言服务：会话健康显示「就绪」
        let mut ready = inputs(ReadOnly::none());
        ready.mode = EditorMode::General;
        ready.lsp = Some("LSP 就绪");
        let text = labels(&ready);
        assert!(text.body.contains("LSP 就绪"), "{}", text.body);
        assert!(
            text.right.iter().any(|b| b.contains("通用")),
            "模式名在右段：{:?}",
            text.right
        );

        // 服务没起 / 挂了：如实报「未就绪」，不假装有服务
        let mut down = inputs(ReadOnly::none());
        down.mode = EditorMode::General;
        down.lsp = Some("LSP 未就绪");
        assert!(labels(&down).body.contains("LSP 未就绪"));

        // 给了值就展示（labels 是纯展示；模式过滤在 host——只有通用模式会传 lsp 值）
        let mut sql = inputs(ReadOnly::none());
        sql.lsp = Some("LSP 就绪");
        assert!(labels(&sql).body.contains("LSP 就绪"));
    }

    #[test]
    fn read_only_dimensions_are_reported_separately() {
        // 三种组合：都不只读 / 仅编辑器 / 仅连接 / 两者
        let none = labels(&inputs(ReadOnly::none()));
        assert!(!none.right.iter().any(|b| b.contains("只读")));

        let editor = labels(&inputs(ReadOnly::editor_only()));
        assert!(
            editor.right.iter().any(|b| b.contains("只读")),
            "{:?}",
            editor.right
        );
        assert!(
            !editor.right.iter().any(|b| b.contains("连接只读")),
            "编辑器只读不等于连接只读：{:?}",
            editor.right
        );

        let connection = labels(&inputs(ReadOnly {
            editor: false,
            connection: true,
        }));
        assert!(connection.right.iter().any(|b| b.contains("连接只读")));
        assert!(
            !connection.right.iter().any(|b| b == "只读"),
            "连接只读不等于编辑器只读：{:?}",
            connection.right
        );

        let both = labels(&inputs(ReadOnly {
            editor: true,
            connection: true,
        }));
        assert!(
            both.right.iter().any(|b| b.contains("只读"))
                && both.right.iter().any(|b| b.contains("连接只读"))
        );
    }

    #[test]
    fn cursor_and_selection_are_shown_with_real_values() {
        let text = labels(&inputs(ReadOnly::none()));
        assert!(
            text.right.iter().any(|b| b.contains("Ln 12, Col 4")),
            "{:?}",
            text.right
        );
        assert!(
            !text.right.iter().any(|b| b.contains("Ch")),
            "没有选区就不显示选区段：{:?}",
            text.right
        );

        let mut selecting = inputs(ReadOnly::none());
        selecting.selected_chars = 7;
        selecting.selected_rows = 2;
        assert!(
            labels(&selecting).right.iter().any(|b| b.contains("Ch 7")),
            "{:?}",
            labels(&selecting).right
        );
        assert!(
            labels(&selecting).right.iter().any(|b| b.contains("Row 2")),
            "选中行数微标：{:?}",
            labels(&selecting).right
        );
    }

    #[test]
    fn failed_actions_leave_a_visible_reason() {
        // 动作失败必须在状态栏留痕迹（“按了没反应”不可接受）；提示属于**左段正文**
        let mut failed = inputs(ReadOnly::none());
        failed.dirty = true;
        failed.message = Some("未命名文档，请先另存为");
        let text = labels(&failed);
        assert!(text.body.contains("未命名文档，请先另存为"), "{}", text.body);
        assert!(text.body.contains("未保存"), "提示不挤掉文档状态：{}", text.body);
        assert!(
            !text.right.iter().any(|b| b.contains("另存为")),
            "提示不进右段：{:?}",
            text.right
        );

        // 无提示时不显示占位
        assert!(!labels(&inputs(ReadOnly::none())).body.contains("另存为"));
    }

    /// 执行中要看得见“跑了多久”（B3）：它是判断该不该中断的依据
    #[test]
    fn executing_segment_counts_the_time_up() {
        let mut running = inputs(ReadOnly::none());
        running.executing = true;
        assert!(labels(&running).body.contains("执行中…"), "没耗时也得说在执行");

        running.elapsed = Some(std::time::Duration::from_millis(3400));
        let text = labels(&running).body;
        assert!(text.contains("执行中 3.4s…"), "{text}");

        // 不在执行时不留耗时（它是执行态的一部分，不是文档属性）
        let idle = inputs(ReadOnly::none());
        assert!(!labels(&idle).body.contains("执行中"));
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
        let text = labels(&with_tx);
        assert_eq!(text.connection.as_deref(), Some("●P·orders"), "连接段独立");
        assert!(
            text.body.starts_with("TX 未开启"),
            "事务紧随连接段（正文从事务开始）：{}",
            text.body
        );

        let mut running = inputs(ReadOnly::none());
        running.tx = Some("TX 已开启 3.4s");
        assert!(labels(&running).body.contains("TX 已开启 3.4s"));

        // 没有事务信息（未接执行）时不出现占位文案
        assert!(!labels(&inputs(ReadOnly::none())).body.contains("TX"));
    }

    /// 【B13】通道段在连接与事务之间（原型 §2.5 的那一行顺序），没给就不占位
    #[test]
    fn the_channel_segment_sits_between_connection_and_transaction() {
        let mut with_channel = inputs(ReadOnly::none());
        with_channel.connection = Some("●P·orders");
        with_channel.channel = Some("通道 源库");
        with_channel.tx = Some("TX 未开启");
        let text = labels(&with_channel);
        assert_eq!(text.connection.as_deref(), Some("●P·orders"));
        assert!(
            text.body.starts_with("通道 源库 · TX 未开启"),
            "通道在事务之前：{}",
            text.body
        );

        // 没给通道（文本模式）时不留占位
        assert!(!labels(&inputs(ReadOnly::none())).body.contains("通道"));
    }

    /// 【T2c-2】方言段独立成段、紧跟连接段（渲染层放面包屑/连接文本之后）；
    /// 没绑定连接就不出现（真实值原则）
    #[test]
    fn dialect_segment_is_separable_and_only_with_a_connection() {
        let mut with_dialect = inputs(ReadOnly::none());
        with_dialect.connection = Some("●P·orders");
        // 输入是裸引擎名（host 从 db_type 解析出的产品名），前缀「方言 」是 labels 加的
        with_dialect.dialect = Some("PostgreSQL");
        let text = labels(&with_dialect);
        assert_eq!(text.connection.as_deref(), Some("●P·orders"));
        assert_eq!(text.dialect.as_deref(), Some("方言 PostgreSQL"));
        assert!(!text.body.contains("PostgreSQL"), "方言不进正文：{}", text.body);

        // 没绑定连接（文本模式 / 未绑定）→ 方言段不占位
        assert!(labels(&inputs(ReadOnly::none())).dialect.is_none());
    }

    /// 【T2c-3】字节数人读微标：B / K / M / G 四档，纯函数可穷举
    #[test]
    fn human_size_formats_b_k_m_g() {
        use super::human_size;
        assert_eq!(human_size(0), "0B");
        assert_eq!(human_size(1023), "1023B");
        assert_eq!(human_size(1536), "1.5K");
        assert_eq!(human_size(1024 * 1024), "1.0M");
        assert_eq!(human_size(5 * 1024 * 1024 * 1024), "5.0G");
    }

    /// 【T2c-3】右段英文微标：Ln/Col + Ch/Row + 编码/换行/缩进 + 大小 + 语句数/模式 + 脏，
    /// 全部真实值、各自独立成段（VSCode 式，不拼长串）
    #[test]
    fn right_segment_uses_english_badges_with_real_values() {
        let text = labels(&inputs(ReadOnly::none()));
        assert!(
            text.right.iter().any(|b| b.contains("Ln 12, Col 4")),
            "行/列：{:?}",
            text.right
        );
        assert!(
            text.right.iter().any(|b| b.contains("UTF-8")),
            "编码微标独立：{:?}",
            text.right
        );
        assert!(
            text.right.iter().any(|b| b.contains("LF")),
            "换行微标独立：{:?}",
            text.right
        );
        assert!(
            text.right.iter().any(|b| b.contains("Spaces: 2")),
            "缩进微标独立（Spaces: N 文案）：{:?}",
            text.right
        );
        assert!(
            text.right.iter().any(|b| b.contains("1.5K")),
            "大小微标独立：{:?}",
            text.right
        );
        // 不脏不占位（真实值原则）
        assert!(
            !text.right.iter().any(|b| b.contains("脏")),
            "干净文档不显示脏：{:?}",
            text.right
        );

        let mut dirty = inputs(ReadOnly::none());
        dirty.dirty = true;
        assert!(labels(&dirty).right.iter().any(|b| b.contains("脏")));

        // 制表符缩进模式：微标显「Tab」而非「Spaces: n」（T2c-4 真实值）
        let mut tabs = inputs(ReadOnly::none());
        tabs.hard_tabs = true;
        assert!(
            labels(&tabs).right.iter().any(|b| b == "Tab"),
            "制表符缩进要显 Tab：{:?}",
            labels(&tabs).right
        );
    }

    /// 【2c】编码/换行微标跟随文档保存格式（不硬编码 UTF-8 · LF）
    #[test]
    fn encoding_and_eol_badges_follow_save_format() {
        // 默认 UTF-8 · LF
        let text = labels(&inputs(ReadOnly::none()));
        assert!(
            text.right.iter().any(|b| b == "UTF-8"),
            "默认编码微标：{:?}",
            text.right
        );
        assert!(
            text.right.iter().any(|b| b == "LF"),
            "默认换行微标：{:?}",
            text.right
        );

        // GBK + CRLF：两个微标都跟着变
        let mut gbk = inputs(ReadOnly::none());
        gbk.format = crate::encoding::SaveFormat {
            encoding: crate::encoding::TextEncoding::Gbk,
            eol: crate::encoding::LineEnding::Crlf,
        };
        let text = labels(&gbk);
        assert!(
            text.right.iter().any(|b| b == "GBK") && text.right.iter().any(|b| b == "CRLF"),
            "GBK/CRLF 微标：{:?}",
            text.right
        );

        // UTF-8 BOM 用短标（状态栏一行放不下长名）
        let mut bom = inputs(ReadOnly::none());
        bom.format = crate::encoding::SaveFormat {
            encoding: crate::encoding::TextEncoding::Utf8Bom,
            eol: crate::encoding::LineEnding::Lf,
        };
        let text = labels(&bom);
        assert!(
            text.right.iter().any(|b| b == "UTF-8 BOM") && text.right.iter().any(|b| b == "LF"),
            "UTF-8 BOM 短标：{:?}",
            text.right
        );
    }
}
