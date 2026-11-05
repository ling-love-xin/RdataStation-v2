//! Mock 视图的样式原语（卡片 / 段落 / 分隔 / chip / 按钮语义）。
//!
//! 为什么单独抽一层：此前「一张卡片」在 `detail.rs`（列卡片）、`panel::render.rs`
//! （清单项 / 结果卡片）、`generator_library.rs`（检视面板）各画一份——
//! `border_1().rounded(radius).p_2()` 手抄三遍，hover 底色各自挑（`list_hover` /
//! `table_hover` / 不设），段标题「属性 / 参数」也是三份 `border_b_1().pb_1()`。
//! 结果是：改卡片内距要动三四处，漏改一处就静默错位（卡片高 1px、线宽不齐）。
//!
//! 这一层只回答**语义**（这是一张卡片 / 这一段叫什么 / 这个 chip 是什么性质的东西），
//! 尺寸与配色在函数内部各自单一来源：
//!
//! | 原语 | 用途 |
//! | --- | --- |
//! | [`card`] / [`card_interactive`] | 卡片容器（常态 / 可悬浮） |
//! | [`card_divider`] | 卡片内的分隔线（比卡片描边更轻） |
//! | [`section`] | 段标题 + 下方细线 |
//! | [`kv`] | 标签-值行（左标签右值） |
//! | [`chip`] / [`data_type_tag`] / [`confidence_tag`] | chip（色相即语义） |
//! | [`surface`] / [`text_hint`] | 表面色 / 弱化文字色 |
//! | [`button`] | 按钮语义（主行动 / 次行动） |
//! | [`icon_button`] | 图标按钮（统一尺寸 + tooltip 延迟） |
//!
//! ## 视觉层级
//!
//! 模块里一共有**三层表面**，靠底色与描边的重量区分：
//!
//! | 层 | 底色 | 描边 |
//! | --- | --- | --- |
//! | 面板（最外） | `background` | 无 |
//! | **卡片** | `muted`（比面板抬一档） | `border` → hover 转 `ring` |
//! | 卡内分隔 | — | `border` 打 0.6 折（不抢卡片边界的视觉重量） |
//!
//! 卡片此前是**透明**的（只有描边），与面板完全同色——白底 + 一根灰线框，正是
//! 线框图（wireframe）的定义，这也是整个模块看起来像表配置的根源。

// 继承父模块（mock_view）的全部 use 绑定
use super::*;

use gpui_kit::base::motion::{Transition, transition};
use gpui_kit::component::Theme;
use gpui_kit::{Hsla, px};
use std::time::Duration;

/// 模块内所有 tooltip 的同一档延迟（500ms）。
///
/// 此前有的 300ms 有的 500ms：鼠标扫过一排按钮，有的立刻弹、有的慢半拍——
/// 同一个面板里两种手感，看着像两个应用拼起来的。
pub const TOOLTIP_DELAY: Duration = Duration::from_millis(500);

/// 卡片悬停 / 聚焦的描边过渡时长（120ms）。
///
/// 组件的 `.hover()` 是**即时切换**（无过渡）：卡片在面板里排成一列时，鼠标扫过去
/// 是一连串硬切，加上右侧操作同时冒出来，整排卡片跟着抖。给 120ms 之后是
/// 「跟着鼠标走」的连续感；再长就会在快速扫过一排卡片时拖出一串尾巴。
pub const CARD_BORDER_TRANSITION: Duration = Duration::from_millis(120);

// ===== 表面（层级）=====

/// 悬停明细的触发记号：弱化小号 info 图标。
///
/// 「常驻短句 + 低频解释进悬停」（右面板状态条的手法）在多处复用，图标尺寸与
/// 弱化色收在这一处，避免各处挑一档灰、一档大——记号不统一，用户就认不出
/// 「这里能悬停看详情」。
pub fn info_hint_icon(theme: &Theme) -> Icon {
    Icon::new(IconName::Info)
        .size(rems(ui::STATUS_ICON_SIZE))
        .text_color(theme.colors.muted_foreground)
}

/// 卡片表面色（比面板抬一档）。
///
/// 取主题 `muted`（亮 neutral-100 / 暗 neutral-800）：它正好是「比 `background` 抬一档」
/// 那一档，是主题 token（换主题时卡片跟着换），不是手挑的灰。
///
/// 为什么卡片必须有底色：此前卡片是透明的，只靠一根 `border` 描边与面板划界——
/// 面板是 `background`（亮 = white），卡片也是 `background`，等于没划。
pub fn surface(theme: &Theme) -> Hsla {
    theme.colors.muted
}

/// 卡片容器（列卡片 / 结果列卡片 / 空态卡片共用）。
///
/// 内距 12px、内部间距 8px。**内距必须大于卡片间距（8px）**：两者等宽时，
/// 眼睛分不出是一张卡还是两张——落差本身就是「组」的边界，有了底色之后
/// 间距只需防粘连，内距负责呼吸。
pub fn card(theme: &Theme) -> Div {
    div()
        .v_flex()
        .gap_2()
        .p_3()
        .w_full()
        .bg(surface(theme))
        .border_1()
        .border_color(theme.colors.border)
        .rounded(theme.radius)
}

/// 卡片 + 悬浮反馈：描边转 `ring`（更重一档）。
///
/// **hover 只动描边、不动底色**。默认主题的中性色只有 100 / 200 两档，
/// 卡片底在 100 那档，hover 该去的 150 档不存在——`list_hover` 在亮主题下
/// 与 `muted` 同为 #f5f5f5，换来等于没换；`table`（亮 = white）更是往反方向走。
/// 而 `ring` 在明暗两主题下都比 `border` 明显一档（亮 neutral-200→400 / 暗 800→500），
/// 描边变深本身就是清晰的反馈，再叠加右侧操作同时冒出来（见视图层），足够。
pub fn card_interactive(theme: &Theme) -> Div {
    let hover_border = theme.colors.ring;
    card(theme).hover(move |s| s.border_color(hover_border))
}

/// 卡片描边色（悬停 / 聚焦时从 `border` 过渡到 `ring`）。
///
/// 用 [`transition`] 而不是 `card_interactive` 的 CSS `.hover()`：后者是即时切换，
/// 而这里要的是**连续**变化。「悬停」是视图状态（`hovered_column` / 焦点句柄），
/// 不是 CSS 伪类——所以能拿到目标值、交给 `transition` 插值。
///
/// `id` 必须每张卡唯一（`transition` 用 keyed state 记过渡起点，撞 id 会让两张卡
/// 共用一条时间线，鼠标从一张移到另一张时颜色从错的值开始变）。
///
/// `transition` 内部已按 `cx.reduce_motion()` 走零时长分支——系统开了「减少动效」
/// 时自动退回即时切换，这里不用再判一次。
pub fn card_border(
    theme: &Theme,
    active: bool,
    id: impl Into<gpui_kit::base::motion::TransitionId>,
    window: &mut Window,
    cx: &mut App,
) -> Hsla {
    let target = if active {
        theme.colors.ring
    } else {
        theme.colors.border
    };
    transition(
        id,
        target,
        Transition::new(CARD_BORDER_TRANSITION),
        window,
        cx,
    )
}

/// 清单行卡片（结果表 / 场景表 / 表配置那一行）。
///
/// 与列卡片同一族（表面 `muted` + 描边 `border`），内距收到「行」的档（左右 8px、上下 4px）：
/// 清单在 280px 的右 Dock 里，按内容档（`p_3`）铺开一屏只放得下五张卡，而清单是
/// 「扫一眼挑一张」的容器——紧凑卡片才是它的正确密度。
///
/// **卡片必须有表面**：此前清单行是贴在面板上的几行字，只有选中 / 悬停时才临时上色，
/// 平时行与行之间没有边界，看上去是表配置里的列表而不是产品里的清单。
/// 且悬停用 `list_hover`（亮主题与卡片底同为 #f5f5f5）等于没换——上了卡片底色之后
/// 悬停只能改描边，正好与列卡片同口径。
///
/// 卡片自己是定位上下文：下缘的 2px 进度线是绝对定位的，贴的是**卡片**的底边。
pub fn row_card(theme: &Theme) -> Div {
    div()
        .v_flex()
        .w_full()
        .px_2()
        .py_1()
        .bg(surface(theme))
        .border_1()
        .border_color(theme.colors.border)
        .rounded(theme.radius)
        .relative()
}

/// 清单行卡片 + 悬停反馈：描边转 `ring`（与列卡片同口径，见 [`card_interactive`]）。
pub fn row_card_interactive(theme: &Theme) -> Div {
    let hover_border = theme.colors.ring;
    row_card(theme).hover(move |s| s.border_color(hover_border))
}

/// 清单行卡片 + 选中态：`list_active` 底 + 同色描边 + 左缘 2px 标识条。
///
/// 标识条走共用原语 `workbench_shell::tree::active_bar`（与 M4 导航 / M5 草稿箱同一根条），
/// 不在本模块手搓绝对定位的 div；它是绝对定位的，放在卡片里不占布局。
pub fn row_card_selected(theme: &Theme) -> Div {
    row_card(theme)
        .bg(theme.colors.list_active)
        .border_color(theme.colors.list_active_border)
        .child(tree::active_bar(theme.colors.list_active_border))
}

/// 「当前正在编辑这一列」的左缘强调条（浮在卡片左缘，不挤动卡内内容）。
///
/// 与导航树的 `tree::active_bar` 同形态（绝对定位 + 上下内缩），但**不复用**那个原语：
/// 语义不同——树条是「选中」（`list_active_border`），这里是「正在编辑」（`primary`）。
/// 一个列表里可以选中多行，但能编辑的列同时只有一列，所以它是强调色、也稍宽一档。
pub fn current_bar(theme: &Theme) -> Div {
    div()
        .absolute()
        .left(px(0.))
        .top(ui::CURRENT_BAR_INSET)
        .bottom(ui::CURRENT_BAR_INSET)
        .w(ui::CURRENT_BAR_WIDTH)
        .flex_none()
        .rounded_sm()
        .bg(theme.colors.primary)
}

/// 卡片内的分隔线（1px，上下各留 4px）。
///
/// 比卡片描边更轻（`border` 打 0.6 折）：卡内分隔只是说「这两块不是一回事」，
/// 不该抢卡片边界的视觉重量，否则卡片看上去被切成两半。
pub fn card_divider(theme: &Theme) -> Div {
    div()
        .w_full()
        .my_1()
        .h(px(1.))
        .bg(theme.colors.border.opacity(0.6))
}

/// 段标题（「属性」/「参数」/「列（3）」）：小字 + **主色** + 半粗 + 下方细线。
///
/// 字色从 `muted_foreground` 提到 `foreground`：标题与它下面的内容同色时，
/// 标题就不是标题了，只是另一行字——这也是表配置感的来源之一。
/// 线跟着标题走（此前标题与线是两个兄弟，线与字的间距三处分别是 0 / 4 / 8px）。
pub fn section(theme: &Theme, label: impl Into<SharedString>) -> Div {
    div()
        .w_full()
        .pb_1()
        .border_b_1()
        .border_color(theme.colors.border)
        .child(
            div()
                .w_full()
                // 标题也会长（场景清单的「本次 Mock 表（N）· 场景名」）：280px 的
                // 右 Dock 里不截断就会顶破面板宽度
                .min_w_0()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.colors.foreground)
                .text_ellipsis()
                .child(label.into()),
        )
}

/// 「标签 + 值」行（左标签弱化色、右值主色，标签区可收缩）。
///
/// 本模块有 6 处手写同一结构（生成器名 / 产出类型 / 业务组 / 样例预览 / 空值率 / 唯一值），
/// 其中两处漏了 `min_w_0`（长值顶破容器），一处漏了 `items_center`（多行时值会飘）。
pub fn kv(theme: &Theme, label: &str, value: impl IntoElement) -> Div {
    div()
        .h_flex()
        .items_center()
        .gap_2()
        .w_full()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_xs()
                .text_color(text_hint(theme))
                .child(label.to_string()),
        )
        .child(value)
}

// ===== 文字三档 =====

/// 弱化提示文字（标签 / 说明 / 计数）：在 `muted_foreground` 上再淡一档。
///
/// 此前段标题、标签、摘要、徽标、说明**全用** `muted_foreground`，只有列名是主色——
/// 中间缺一档，结果是卡片里除了列名全是同一种灰，读的时候分不出主次。
pub fn text_hint(theme: &Theme) -> Hsla {
    theme.colors.muted_foreground.opacity(0.78)
}

// ===== chip =====

/// chip 的色相分族：按「数据长什么样」分，不按字母序——
/// 一张卡片里 13 种类型要能一眼分族，靠色不靠读字（读了字才知道是啥，就失去 chip 的意义）。
///
/// 灰色族取 `muted_foreground` 而不是 `secondary`：`secondary` 在亮主题下是 neutral-200，
/// 拿它当文字色等于把字画没。
fn data_type_tone(theme: &Theme, t: &ColumnDataType) -> Hsla {
    match t {
        ColumnDataType::Integer
        | ColumnDataType::BigInt
        | ColumnDataType::Float
        | ColumnDataType::Double
        | ColumnDataType::Decimal { .. } => theme.colors.info,
        ColumnDataType::Varchar { .. } | ColumnDataType::Text => theme.colors.success,
        ColumnDataType::Boolean => theme.colors.primary,
        ColumnDataType::Date | ColumnDataType::DateTime | ColumnDataType::Timestamp => {
            theme.colors.warning
        }
        ColumnDataType::Uuid | ColumnDataType::Blob => theme.colors.muted_foreground,
    }
}

/// 色相 chip（tint：同色淡底 + 同色字 + 同色描边）。
///
/// 此前一律 `Tag::outline()`：描边 chip 在卡片里几乎看不见，13 种类型的分族色
/// 被稀释没了——色相分族的设计等于白做。tint 才有存在感，又不像实心块那样
/// 在一排卡片里吵（实心块 13 种色相会把卡片的正文压过去）。
fn chip(tone: Hsla, label: impl Into<SharedString>) -> Div {
    div()
        .flex_none()
        .px_1()
        .rounded(px(3.))
        .border_1()
        .border_color(tone.opacity(0.28))
        .bg(tone.opacity(0.14))
        .text_color(tone)
        .text_xs()
        .child(label.into())
}

/// 数据类型 chip（卡片里列名旁的注解，不是卡片的主内容）。
pub fn data_type_tag(theme: &Theme, t: &ColumnDataType) -> Div {
    chip(data_type_tone(theme, t), column_type_label(t))
}

/// 置信度 chip：high=成功色（推断可信）/ manual=信息色（人手定的）/ low=灰（推断没把握）。
pub fn confidence_tag(theme: &Theme, confidence: &str) -> Div {
    let (tone, label) = match confidence {
        "high" => (theme.colors.success, "high"),
        "manual" => (theme.colors.info, "manual"),
        _ => (theme.colors.muted_foreground, "low"),
    };
    chip(tone, label)
}

/// 卡片标题行的主文字（列名 / 表名）：中号 + 半粗 + 可截断。
///
/// 一律截断：卡片宽度由面板定，长表名顶破卡片会把右端的 chip 挤出边界。
pub fn card_title(text: impl Into<SharedString>) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .text_sm()
        .font_weight(FontWeight::MEDIUM)
        .text_ellipsis()
        .child(text.into())
}

/// 小字整行（说明 / 警告 / 错误的共同骨架）。
fn line(color: Hsla, text: impl Into<SharedString>) -> Div {
    div()
        .w_full()
        .text_xs()
        .line_height(rems(1.25))
        .text_color(color)
        .child(text.into())
}

/// 对话框 / 面板里的说明行（弱化色，可换行）。
///
/// 与 [`card_note`] 的区别：卡片里的摘要是单行可截断（卡片宽度是定死的），
/// 对话框里的说明常常是两句以上的整句，截断了等于没说——所以它不 `.text_ellipsis()`。
pub fn hint_line(theme: &Theme, text: impl Into<SharedString>) -> Div {
    line(text_hint(theme), text)
}

/// 校验错误行（danger 色，可换行）。
///
/// 对话框里错误就地显示、不关窗口，所以错误行必须自己站得住——
/// 用 `danger` 而不是给输入框描边：一行里两个输入框，描边说不清错的是哪个。
pub fn error_line(theme: &Theme, text: impl Into<SharedString>) -> Div {
    line(theme.colors.danger, text)
}

/// 条件警告行（warning 色）：不是错，但会挡住下一步（例如父表没有自增列）。
pub fn warning_line(theme: &Theme, text: impl Into<SharedString>) -> Div {
    line(theme.colors.warning, text)
}

/// 预告行（info 色）：接下来**会发生什么**（例如自增起点从 MAX + 1 接续）。
pub fn info_line(theme: &Theme, text: impl Into<SharedString>) -> Div {
    line(theme.colors.info, text)
}

/// 结果行（success 色）：刚刚**做成了什么**（出口反馈：落库 / 另存为 / 草稿箱）。
pub fn success_line(theme: &Theme, text: impl Into<SharedString>) -> Div {
    line(theme.colors.success, text)
}

/// 卡片内的说明 / 摘要文字（弱化色 + 可截断）。
///
/// 用 `muted_foreground` 而不是更淡的 `text_hint`：它是卡片正文的一部分，
/// 要让位给标签与提示，但仍属于"读得到的内容"。
pub fn card_note(theme: &Theme, text: impl Into<SharedString>) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .text_xs()
        .text_color(theme.colors.muted_foreground)
        .text_ellipsis()
        .child(text.into())
}

// ===== 按钮 =====

/// 按钮语义：本模块只留三档，且**位置固定**。
///
/// 此前 47 个按钮里 primary 只有 2 个、ghost 与 secondary 各二十几个，
/// 「这一步的主行动是哪个」全靠用户读文字——按钮排一排分不出轻重。
/// 更糟的是同一句注释里写着「视觉上给 primary」，代码挂的却是 `.secondary()`
/// （面板出口的「持久化到项目分析库」）：口头的层级与画出来的层级不是一回事。
///
/// | 语义 | 形态 | 出现位置 |
/// | --- | --- | --- |
/// | [`ActionKind::Main`] | Primary（填色） | 每个容器**最多一个**（生成 / 落库 / 加列 / 对话框确定） |
/// | [`ActionKind::Soft`] | Secondary（描边） | 容器里真正会点、需要看见的（重试 / 取消 / 追加 / 导出） |
/// | [`ActionKind::Quiet`] | Ghost（无框） | 行内微操作与下拉触发器（不需要容器级的存在感） |
///
/// **Quiet 不是「更弱的 Soft」**，是另一种东西：Soft 是「这里有个按钮等你点」，
/// Quiet 是「这行字可以点」。把下拉触发器做成描边按钮，一排筛选器就会变成一排
/// 实心方块，反而把下面的内容压住。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    /// 主行动（填色）：每个容器最多一个
    Main,
    /// 次行动（描边）
    Soft,
    /// 行内微操作 / 下拉触发器（无框）
    Quiet,
}

/// 按语义造按钮（文案由调用方给，形态由这里定）。
///
/// 调用方不再写 `.primary()` / `.secondary()` / `.ghost()`：那些是形态，不是意图。
pub fn button(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    kind: ActionKind,
) -> Button {
    let button = Button::new(ElementId::Name(id.into())).label(label.into());
    match kind {
        ActionKind::Main => button.primary(),
        ActionKind::Soft => button.secondary(),
        ActionKind::Quiet => button.ghost(),
    }
}

/// 下拉触发器（语言 / 预览行数 / 场景模板 / 预设）。
///
/// 五处各自 `Button::new(..).ghost().xsmall().label(..).dropdown_caret(true)`——
/// 手抄五遍的后果是漏挂 caret 的那一处看起来像个普通按钮，点下去才知道有菜单
/// （菜单不藏在 caret 后面，用户就不知道这里能展开）。
pub fn dropdown_button(id: impl Into<SharedString>, label: impl Into<SharedString>) -> Button {
    Button::new(ElementId::Name(id.into()))
        .ghost()
        .xsmall()
        .label(label.into())
        .dropdown_caret(true)
}

/// 危险的文字按钮（模板 / 历史记录的「删除」）。
///
/// 与卡片共用「只染字不染底」的口径：这些按钮本身是行内操作（悬停才显形），
/// 常显时若给红底，一列记录看下来满屏红块，真正要讀的内容反而被盖住。
pub fn danger_text_button(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    theme: &Theme,
) -> Button {
    Button::new(ElementId::Name(id.into()))
        .ghost()
        .xsmall()
        .label(label.into())
        .text_color(theme.colors.danger)
}

/// 图标按钮（小号 + 无框 + 统一 tooltip 延迟）。
///
/// icon-only 必须带 tooltip：一个箭头图标不带说明，用户分不清是排序还是折叠。
pub fn icon_button(
    id: impl Into<SharedString>,
    icon: IconName,
    tooltip: impl Into<SharedString>,
) -> Button {
    Button::new(ElementId::Name(id.into()))
        .ghost()
        .xsmall()
        .icon(icon)
        .tooltip(tooltip.into())
        .tooltip_show_delay(TOOLTIP_DELAY)
}

/// 删除用的图标按钮（无框 + 危险色图标）。
///
/// 只染图标不染底：卡片里一排操作按钮摆着时，红底按钮会盖过卡片内容——
/// 危险色留给鼠标悬上去之后认出来的那一眼。
pub fn danger_icon_button(
    id: impl Into<SharedString>,
    icon: IconName,
    tooltip: impl Into<SharedString>,
    theme: &Theme,
) -> Button {
    Button::new(ElementId::Name(id.into()))
        .ghost()
        .xsmall()
        .icon(icon)
        .text_color(theme.colors.danger)
        .tooltip(tooltip.into())
        .tooltip_show_delay(TOOLTIP_DELAY)
}
