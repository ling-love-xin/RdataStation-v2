//! 结果状态 / 渲染域（`grid/` 第十块）：工具栏（`ResultToolbar`）、状态行
//! （`ResultStatus`）、提示文案、`ResultControls` / `ResultPane` 结构与 `render` 家族。
//! 注：`crate::status`（根模块）提供 `elapsed_text` 等通用文案，与本文件同名不冲突。

use gpui_kit::assets::IconName;
use gpui_kit::*;
use gpui_kit::base::Disableable as _;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::table::{DataTable, TableState};
use gpui_kit::component::toolbar::Toolbar;
use gpui_kit::prelude::FluentBuilder as _;

use crate::status::elapsed_text;
use crate::ui;
use ::shared::CurrentTier;
use super::delegate::{RESULT_GRID_CONTEXT, ResultGridDelegate};
/// 结果工具栏（⑥，原型 §2.4）：左段 `行数 N` │ `耗时 1.2s` │ `连接名`
///
/// 全是可选真值：没有的东西不显示，也不用占位文案撑场面。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResultToolbar {
    /// 行数（`None` = 这次没有网格：写语句 / 失败）
    pub rows: Option<usize>,
    /// 写语句的影响行数（`Some` = 这次只有影响行数，没有结果集）
    pub affected_rows: Option<u32>,
    /// 这次执行失败了吗（原因在错误卡片里，这里只标一个「失败」）
    pub failed: bool,
    /// 耗时（毫秒；`None` = 还没有结论）
    pub elapsed_ms: Option<u64>,
    /// 来源连接文案（`●P·orders`；`None` = 当时未绑定 / 认不出）
    pub connection: Option<String>,
    /// 【B15】来源摘要（血缘）：`原查询` / `下发筛选` / `排序下发` / `取下一段` / 标题（执行计划 / 分析）
    pub lineage: Option<String>,
    /// 【B15】本地筛选生效时的（视图行数，已抓总行数）——原型 §5.5 要求“统计跟随并明示已筛选”
    pub filtered: Option<(usize, usize)>,
    /// 【B5】事实档载体档位（`None` = 无事实档，回退显示矩阵降级档；不摆载体段）。
    pub fact_tier: Option<CurrentTier>,
    /// 【B5】事实档 Rust 堆估算字节：仅内存档有意义；表档 batches 已释放报 0，段上不摆字节。
    pub fact_bytes: usize,
    /// 【DBeaver 对齐】来源 SQL 全文（工具栏最左侧 SQL 按钮点击显示；`None` = 不摆按钮）
    pub sql: Option<String>,
}

impl ResultToolbar {
    /// 左段文案（**纯函数**：哪几段出现、数字怎么写都在这儿定，可逐条断言）
    pub fn segments(&self) -> Vec<String> {
        let mut parts = Vec::new();
        if let Some(affected) = self.affected_rows {
            parts.push(format!("影响 {} 行", thousands(affected as usize)));
        } else if let Some(rows) = self.rows {
            parts.push(format!("行数 {}", thousands(rows)));
        }
        if let Some((visible, total)) = self.filtered {
            parts.push(format!(
                "已筛选 {} / {} 行",
                thousands(visible),
                thousands(total)
            ));
        }
        if self.failed {
            parts.push("失败".to_string());
        }
        if let Some(elapsed_ms) = self.elapsed_ms {
            parts.push(format!("耗时 {}", duration_text(elapsed_ms)));
        }
        // 【B5】事实档载体可见性：内存档报列式占用；表档只标载体（表内占用归 DuckDB 侧，
        // 不在这里估一份会失真的 Rust 堆字节）；无事实档不摆段。
        match self.fact_tier {
            Some(CurrentTier::InMemory) => {
                if self.fact_bytes > 0 {
                    parts.push(format!("内存档 {}", bytes_text(self.fact_bytes)));
                } else {
                    parts.push("内存档".to_string());
                }
            }
            Some(CurrentTier::OnDuckDb) => parts.push("表档".to_string()),
            Some(CurrentTier::None) | None => {}
        }
        // 【B15】血缘（原型 §2.4：结果集要带来源摘要）——排在连接之前：
        // “这份是怎么来的”比“从哪条连接来”更常被问
        if let Some(lineage) = &self.lineage {
            parts.push(lineage.clone());
        }
        if let Some(connection) = &self.connection {
            parts.push(connection.clone());
        }
        parts
    }
}

/// 结果状态行（⑦）的输入：那一行是分页 / 总行数 / 已选 / 截断提示的家
///
/// 分页与「取下一段」属 B5b：接上之后它们也放这一行（原型 §2.4 的 `‹ 1 2 3 … ›`）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResultStatus {
    /// 已抓到的行数
    pub total_rows: usize,
    /// 当前选中的行（1 基；`None` = 没选中）
    pub selected_row: Option<usize>,
    /// 当前选中的格的值预览（DBeaver 对齐：状态行左下显示当前格；`None` = 没选中格）
    pub selected_cell: Option<String>,
    /// 截断提示（`Some` = 数据不完整）
    pub truncated_hint: Option<String>,
    /// 【B5b】还能取下一段吗（`true` 时总行数显示成 `1,000+`，并摆「取下一段」）
    pub has_more: bool,
}

/// 状态行左右两段（纯函数；截断提示单独渲染——它是警告，要另一种颜色）
pub fn status_segments(status: &ResultStatus) -> (String, String) {
    // 【B5b】还有下一段时总数不是“共”而是“已抓”：`1,000+` 说的就是“至少这么多”
    let left = if status.has_more {
        format!("共 {}+ 行", thousands(status.total_rows))
    } else {
        format!("共 {} 行", thousands(status.total_rows))
    };
    let right = match status.selected_row {
        Some(row) => format!("已选第 {} 行", thousands(row)),
        None => "未选中行".to_string(),
    };
    (left, right)
}

/// 单次查询的行数上限（对齐 engine `services/sql_service.rs` 的 `MAX_QUERY_ROWS = 10_000`；
/// 本 crate 不跨模块搬 engine 常量，仅状态行文案引用同一口径）。
pub const MAX_SINGLE_SEGMENT_ROWS: usize = 10_000;

/// 窗口分段加载的代价说明（落实 engine `sql_service.rs:78-79`「这条取舍要写在界面上，
/// 不能假装它和游标等价」）：每段重跑一次查询，`ORDER BY` 不稳时两段之间可能重复 /
/// 跳过行。只挂在「还有更多」/「截断」提示的 tooltip 上，不常驻占状态行宽度。
pub const WINDOW_FETCH_NOTICE: &str =
    "分段加载：每段重跑查询；无稳定 ORDER BY 时行可能重复或跳过";

/// 状态行「还有更多」提示：这段拿满、可能还有下一段（`has_more`）。
pub fn fetch_more_hint(status: &ResultStatus) -> Option<&'static str> {
    status.has_more.then_some("还有更多 · 滚动到底继续加载")
}

/// 撞单段上限且已到底（`truncated_hint` 有值、`!has_more`）：文案明确是「被截断、结果
/// 不完整」，区别于「正常取完」。分段中同时挂截断信号属防御组合，此处不重复出警告
/// （旧 `truncated_hint` 由渲染层兜底显示）。
pub fn truncation_notice(status: &ResultStatus) -> Option<String> {
    if status.truncated_hint.is_some() && !status.has_more {
        Some(format!(
            "已达单段上限 {} 行 · 结果不完整",
            thousands(MAX_SINGLE_SEGMENT_ROWS)
        ))
    } else {
        None
    }
}

/// 是否需要挂窗口代价 tooltip：分段中或被截断时给。
pub fn window_fetch_active(status: &ResultStatus) -> bool {
    status.has_more || status.truncated_hint.is_some()
}

/// 底部动作状态行的**占位动作钮**（#26 视图骨架：本轮全部禁用灰态）。
///
/// 顺序对齐 DBeaver 底部动作条，三段：
/// - 写操作段（0..7）：保存 · 取消 · 新增行 · 复制行 · 删除行 · 撤销 · 重做；
/// - 翻页段（7..10）：首页 · 上页 · 末页——「下页」是现有取下一段真钮，由
///   `ResultControls.more` 注入，不进占位清单；
/// - 系统段（10..）：导出 · 设置。
///
/// 行级叠加层模型（新增 / 复制 / 删除 + 行级撤销重做）是下一轮任务，见
/// `result-panel.md` §8.10 ④；本轮只摆位、不接线，按钮一律 `disabled(true)`。
pub fn status_action_placeholders() -> Vec<(IconName, &'static str)> {
    vec![
        (IconName::Save, "保存（待行级模型落地）"),
        (IconName::X, "取消（回滚待提交变更）"),
        (IconName::Pencil, "修改选中格（双击单元格进入编辑）"),
        (IconName::Plus, "新增行（暂未支持下发）"),
        (IconName::CopyPlus, "复制行（暂未支持下发）"),
        (IconName::Trash, "删除行（暂未支持下发）"),
        (IconName::Undo2, "撤销"),
        (IconName::Redo2, "重做"),
        (IconName::SkipBack, "首页（窗口化取数暂不支持）"),
        (IconName::ChevronLeft, "上页（窗口化取数暂不支持）"),
        (IconName::SkipForward, "末页（窗口化取数暂不支持）"),
        (IconName::Download, "导出"),
    ]
}
/// 占位动作钮分组下标的分界（写操作段 `0..FIRST_PAGER_ACTION`；翻页段 `FIRST_PAGER_ACTION..FIRST_SYSTEM_ACTION`；
/// 系统段 `FIRST_SYSTEM_ACTION..`）。render 层据此在段间插发丝分隔线。
pub(crate) const FIRST_PAGER_ACTION: usize = 8;
pub(crate) const FIRST_SYSTEM_ACTION: usize = 11;

/// 结果工具栏右段的动作控件（由面板构造：它知道点击该干什么）
///
/// 与状态栏的 `StatusControls` 同口径：文案交给可穷举的纯函数，控件交给面板。
#[derive(Default)]
pub struct ResultControls {
    /// 【B15】本地筛选框（原型 §5.5：只作用于视图层，不重查）
    pub filter: Option<AnyElement>,
    /// 【侧栏】右侧辅助栏开关（面板构造：翻转 `show_value_sidebar`；列浏览器 / 值详情 /
    /// 计算 / 分组共用这一栏、页签切换）
    pub value_sidebar: Option<AnyElement>,
    /// 【并排对比】对比模式开关（两份以上结果时才给；开启后结果区双栏并排）
    pub compare: Option<AnyElement>,
    /// 【T2a-7】批量提交/回滚对（有待提交变更才给；参考 DBeaver 的提交/回滚对）
    pub commit: Option<AnyElement>,
    /// 【B15】本地分析（下拉：计数 / 逐列分组计数；数据是桥接过来的行）
    pub analysis: Option<AnyElement>,
    /// 复制当前结果集（TSV）
    pub copy: Option<AnyElement>,
    /// 【B7】导出当前结果集（下拉：格式 × 仅已抓取 / 抓全量）
    pub export: Option<AnyElement>,
    /// 重跑当前结果集的 SQL（结果集换一份新的，不是新开一份）
    pub refresh: Option<AnyElement>,
    /// 【B5b】取下一段（只在这份结果还有下一段时给）
    pub more: Option<AnyElement>,
    /// 【DBeaver 对齐】取全部行（一次性抓全量；只在这份结果还有下一段时给）
    pub fetch_all: Option<AnyElement>,
    /// 【#28-B3】新增行（面板注入；只读结果集不注入 → 回退禁用占位钮）
    pub insert_row: Option<AnyElement>,
    /// 【#28-B3】复制当前选中行为本地行（同上）
    pub duplicate_row: Option<AnyElement>,
    /// 【#28-B3】删除当前选中行（同上）
    pub delete_row: Option<AnyElement>,
    /// 【#28-B3】撤销（组合语义：格级栈优先，空走行级栈；面板注入禁用态由栈状态决定）
    pub undo: Option<AnyElement>,
    /// 【#28-B3】重做（组合语义与撤销对称）
    pub redo: Option<AnyElement>,
}

/// 结果区一次要画的东西（面板一次读齐；`render` 只负责画）
#[derive(Default)]
pub struct ResultPane {
    pub toolbar: ResultToolbar,
    /// 结果状态行（只有网格时才给；失败 / 写语句不给）
    pub status: Option<ResultStatus>,
    pub controls: ResultControls,
    /// 失败时的错误卡片（有卡片就不画网格——网格里本来也没东西）
    pub card: Option<AnyElement>,
    /// 结果集标签条（两份以上结果才给）
    pub tabs: Option<AnyElement>,
    /// 【B13】结果区顶部那一行提示（切通道后“旧结果来自 X”）；`None` = 不摆
    pub notice: Option<String>,
    /// 【DBeaver 对齐】中段行数输入框（行号跳转；`None` = 摆只读文案）
    pub row_count_input: Option<gpui::Entity<gpui_kit::component::input::InputState>>,
}

/// 【DBeaver 对齐】渲染工具栏（横跨整个顶部）
///
/// 从 `result_grid::render` 里抽出来，放到最顶部——用户反馈「工具栏遮挡了结果集的字段」。
/// 2026-10-28 重构（组件统一化 §3）：
/// - 右段纯按钮序列 → `toolbar::Toolbar` 组件（统一容器 + a11y；按钮已是标准 Button /
///   DropdownButton，经 `.contents()` 挂入，保留各自 ghost/small 外观）
/// - 左段 = SQL 预览 + 本地筛选（非按钮内容，Toolbar child 只收 Sizable，走外层 content）
/// - 【1.3 对齐】下发源库 / 注释两个 Switch 也收进右段 Toolbar（统一容器管高度与
///   基线，避免 Switch 默认档与按钮小号高度不一致导致的错位）
/// - 执行信息（行数/耗时/失败/血缘/连接…）**不在工具栏**：移入状态栏（见
///   `render_status_row` 的 `memory` / `affected_rows` 参数）——用户口径：执行信息归结果集状态栏
/// - 导出 / 刷新同样移入状态栏（左动作组系统段），工具栏不再摆
pub fn render_toolbar(
    sql_preview: Option<AnyElement>,
    filter: Option<AnyElement>,
    pushdown: Option<AnyElement>,
    comments: Option<AnyElement>,
    compare: Option<AnyElement>,
    commit: Option<AnyElement>,
    analysis: Option<AnyElement>,
    copy: Option<AnyElement>,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let muted = theme.colors.muted_foreground;
    div()
        .h_flex()
        .items_center()
        .justify_between()
        .gap_2()
        .px_2()
        // 工具栏比状态栏高：Toolbar XSmall（min_h_7 + p_1 = 36px）放不进 24px 状态栏档。
        // 注意：36px 会压缩结果主体——结果区默认高度已同步提到 10rem（editor/ui.rs
        // RESULT_PANE_HEIGHT），保证网格表头 + 前两行可见（编辑行被虚拟列表裁剪会
        // 丢 Input 焦点链，cell_edit_esc 曾因此回归）
        .h(rems(ui::RESULT_TOOLBAR_HEIGHT))
        .flex_none()
        .border_b(ui::HAIRLINE)
        .border_color(theme.colors.border)
        .text_xs()
        .text_color(muted)
        .debug_selector(|| "editor-result-toolbar".to_string())
        .child(
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .min_w_0()
                // 【1.1】工具栏最左侧 SQL 预览：面板构造的可点击元素（Button 自带 hover
                // 高亮 + 手型提示可点击；点击展开完整 SQL 条，见 panel.rs render）
                .children(sql_preview)
                // 执行信息（segments：行数/耗时/失败/血缘/连接…）已移状态栏（用户口径）
                .children(filter),
        )
        .child(
            // 右段：纯按钮序列 → Toolbar 组件（统一容器 + a11y 语义；contents 槽收
            // AnyElement，按钮保留 ghost/small 外观；顺序照原型 §2.4 右段：
            // ⌕ 筛选（左段）· 下发源库 / 注释 switch · ⧉ 对比 · 提交变更 · ⚗ 分析 ▾ ·
            // 复制；导出 ▾ / ⟳ 刷新已移状态栏系统段）
            Toolbar::new("editor-result-toolbar-actions")
                .xsmall()
                .contents(
                    [pushdown, comments, compare, commit, analysis, copy]
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>(),
                ),
        )
        .into_any_element()
}

/// 截断提示文案（达到驱动行数上限时；原型 §2.4 的口径是 `已截断至 N 行`）
///
/// 只报**我们真拿到多少行**；上限是驱动侧的事（`engine` 侧 10000 行），编辑器不猜那个数。
pub fn truncated_hint(rows: usize) -> String {
    format!("已截断至 {} 行", thousands(rows))
}

/// 千分位（原型里的 `1,204`；用于行数 / 统计计数等非负整数）
pub fn thousands(value: usize) -> String {
    group_digits(&value.to_string())
}

/// 纯数字串按三位一组插逗号（分组内核；`thousands` 与单元格整数展示共用一份口径）
pub(crate) fn group_digits(digits: &str) -> String {
    let mut text = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            text.push(',');
        }
        text.push(ch);
    }
    text
}

/// 【P2】整数单元格展示千分位：能解析成 `i64` 的整数值加分组逗号，负数保留负号。
///
/// 只改**展示**：复制 / 悬停 / 导出 / 编辑仍取原始 `text`（见 `render_td`）。浮点 / 文本 /
/// `NULL` / 空串返回 `None`，交回普通展示路径；小数本轮不分组，避免破坏精度与科学计数法口径。
pub(crate) fn format_integer_cell(text: &str) -> Option<String> {
    let trimmed = text.trim();
    let value: i64 = trimmed.parse().ok()?;
    let (sign, abs) = if value < 0 {
        ("-", value.unsigned_abs().to_string())
    } else {
        ("", value.to_string())
    };
    Some(format!("{sign}{}", group_digits(&abs)))
}

/// 耗时文案（原型 §2.4 的 `1.2s`）
///
/// 不到一秒直接报毫秒：`0.0s` 什么也没说（而且首屏结果常常就在这个量级）。
pub fn duration_text(elapsed_ms: u64) -> String {
    if elapsed_ms < 1000 {
        format!("{elapsed_ms} ms")
    } else {
        elapsed_text(std::time::Duration::from_millis(elapsed_ms))
    }
}

/// 【B5】事实档占用字节归一化文案（KiB / MiB / GiB；保留一位小数，小于 1 KiB 报字节）。
/// 表档 batches 已释放、Rust 堆占用为 0，调用方只摆「表档」标签、不摆字节段。
pub fn bytes_text(bytes: usize) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    let value = bytes as f64;
    if value >= GIB {
        format!("{:.1} GiB", value / GIB)
    } else if value >= MIB {
        format!("{:.1} MiB", value / MIB)
    } else if value >= KIB {
        format!("{:.1} KiB", value / KIB)
    } else {
        format!("{bytes} B")
    }
}

/// 建一个结果表状态（面板构造时一次；结果变化用 `refresh` 而不是重建）
pub fn new_table_state(
    delegate: ResultGridDelegate,
    window: &mut Window,
    cx: &mut App,
) -> Entity<TableState<ResultGridDelegate>> {
    cx.new(|cx| {
        TableState::new(delegate, window, cx)
            .col_resizable(true)
            .row_selectable(true)
            .cell_selectable(true)
            // 组件自带的窄行头去掉（`cell_selectable` 下默认会画一条）：我们的 `#` 列
            // 就是行把手，两条并列各能点一半是纯粹的干扰。关掉后仍能选整行——
            // **再点一下已选中的格子**升级为整行（组件自己的口径，`row_selectable` 已开）
            .row_header(false)
            // 【DBeaver 对齐】释放 gpui-kit table 组件全部能力：
            .col_movable(true)           // 列序拖拽（拖拽列头改变列顺序）
            .sortable(true)              // 列头排序（点击列头排序）
            .col_selectable(true)        // 选中整列
            .loop_selection(true)        // 键盘导航循环选择
            .select_on_modifier_click(true) // Ctrl+点击多选
    })
}

/// 画结果区（⑤ 标签条 → ⑥ 工具栏 → 网格 / 错误卡片 → ⑦ 状态行）
///
/// **高度不在这里定**：结果区是 `ResizablePanel` 的一个面板（可拖拽），
/// 它的高度由分栏给出——这里只保证自己撑满那个面板。
pub fn render(
    state: &Entity<TableState<ResultGridDelegate>>,
    pane: ResultPane,
    cx: &App,
) -> impl IntoElement {
    // 状态行已抽走（render_status_row 面板级调用），render 主体不再取色

    // 先把各段拆开：工具栏先拿走 refresh/copy，状态行⑦再拿 more（部分移动会让字段不可用）
    let ResultPane {
        card,
        tabs,
        notice,
        // 工具栏 / 状态行 / 行数框已抽到面板级（render_toolbar / render_status_row），
        // 状态动作按钮只由 render_status_row 消费（面板级按值传入），pane 内不再读
        ..
    } = pane;
    // 有卡片就不画网格（网格里本来也没东西，画出来只是一块空白）
    let has_card = card.is_some();

    div()
        .v_flex()
        .w_full()
        .h_full()
        .min_h_0()
        // 测试按选择器断言“结果区在不在、多高”：分栏是结构，不是装饰
        .debug_selector(|| "editor-result-pane".to_string())
        // ⑤ 结果集标签条（原型里在工具栏上面）
        .children(tabs)
        // 【B13】切通道后的那一行提示（原型 §5.7 规则 2）：旧结果不删，但要能看出它不是
        // 现在这档跑的——颜色（标签标灰）只是一个通道，白话在这一行
        .children(notice.map(|text| {
            let theme = cx.theme();
            let warning = theme.colors.warning;
            div()
                .h_flex()
                .items_center()
                .gap_1()
                .px_2()
                .py_1()
                .text_xs()
                .text_color(warning)
                .bg(warning.opacity(0.18))
                .border_b(ui::HAIRLINE)
                .border_color(warning)
                // 测试按选择器断言“切了通道要说一句”
                .debug_selector(|| "editor-result-notice".to_string())
                .child(SharedString::from(text))
        }))
        // ⑥ 结果工具栏——已抽出来放到最顶部（render_toolbar 函数）
        // 有卡片：卡片替掉网格；否则画网格
        .children(card)
        .when(!has_card, |pane| {
            pane.child(
                div()
                    .flex_1()
                    .min_h_0()
                    .debug_selector(|| "editor-result-grid".to_string())
                    // 【B14】`Ctrl+C` 的上下文只挂这一层（键位在 `crates/app` 注册）：
                    // 焦点在网格里时它才在 dispatch path 上，编辑区里按 `Ctrl+C`
                    // 仍是内核的文本复制。焦点本身不用我们管——组件的 `DataTable`
                    // 自己 `track_focus`，点一下就在它身上了（用例从真点击一路验到剪贴板）
                    .key_context(RESULT_GRID_CONTEXT)
                    // 密度档走常量表（`RESULT_TABLE_SIZE` = 组件 XSmall = 26px）——
                    // 不写 size 就落到组件默认档 32px（原型稿写的是 22px，三个数都不一致，见常量注释）
                    .child(
                        DataTable::new(state)
                            .stripe(true)
                            .bordered(false)
                            .with_size(ui::RESULT_TABLE_SIZE),
                    ),
            )
        })
        // ⑦ 结果状态行——已抽到 `render_status_row`（面板级第四行调用：状态栏横跨
        // 左右栏之下，左端 / 右端挂左右栏开关，见 panel.rs `render`）。这里不再渲染。
}

/// 【DBeaver 对齐】结果状态行（面板级第四行）：左动作组（写操作 ‖ 翻页 ‖ 系统）→
/// 中段行数框 → 右段执行信息（【状态栏三项】内存 / 查询时间 / 查询条数）。
///
/// 从 `render` 里抽出（2026-10-28 面板四行布局）：状态栏从网格 pane 内提到面板级，
/// 横跨左右栏之下；左端 / 右端由面板注入左右栏开关（`left_switch` / `right_switch`，
/// `None` 不摆——文本视图无左栏语义时不注入左栏开关）。
/// 2026-10-28 用户口径：状态栏信息只显示 内存 / 查询时间 / 查询条数，其它不显示。
pub fn render_status_row(
    // 状态行（只有网格时才给）。2026-10-28 起写语句 / 失败也有状态栏（用户口径：
    // 导出 / 刷新 / 执行信息都在状态栏），`None` 时跳过网格专属段（行数框只读文案），
    // 左动作组与系统段（导出 / 刷新）照常渲染
    status: Option<&ResultStatus>,
    // 按值传入（移动）：状态动作按钮（more / fetch_all / 行级 / 撤销重做 / 导出 / 刷新）
    // 只在这里渲染，主 pane 不再读它们（面板解构后重组这份，见 panel.rs `render`）
    controls: ResultControls,
    row_count_input: Option<&gpui::Entity<gpui_kit::component::input::InputState>>,
    elapsed_ms: Option<u64>,
    left_switch: Option<AnyElement>,
    right_switch: Option<AnyElement>,
    // 【状态栏三项】右段只显示 内存 / 查询时间 / 查询条数（用户口径 2026-10-28：
    // 只留这三项，其它信息不显示）。内存文案由面板算好传入（内存档 fact_bytes →
    // 「内存 X」；表档 / 无事实档 = None）；写语句影响行数单独传（有值时条数文案
    // 用「影响 N 行」）。
    memory: Option<String>,
    affected_rows: Option<u32>,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let muted = theme.colors.muted_foreground;
    let (left, _old_right) = status
        .map(status_segments)
        .unwrap_or_default();
    let border = theme.colors.border;
    // 【状态栏三项】右段信息 = 内存 · 查询时间 · 查询条数
    let mem_text = memory;
    let elapsed = elapsed_ms.map(|ms| format!("耗时 {}", duration_text(ms)));
    // 查询条数：写语句显示「影响 N 行」，网格显示「查询条数 N 行」（还有下一段 = N+）
    let row_text = match affected_rows {
        Some(affected) => Some(format!("影响 {} 行", thousands(affected as usize))),
        None => status.map(|status| {
            if status.has_more {
                format!("查询条数 {}+ 行", thousands(status.total_rows))
            } else {
                format!("查询条数 {} 行", thousands(status.total_rows))
            }
        }),
    };

    // 【#26 视图骨架】底部动作状态行三段：左动作组（占位灰钮 + 取下一段真钮）、
    // 中段只读行数框、右段当前格 / 截断 / 选中行元信息。占位钮全部 disabled，
    // 行级叠加层模型落地后逐个接线（result-panel.md §8.10 ④）。
    let placeholders = status_action_placeholders();
    let placeholder_btn = |(icon, tooltip): (IconName, &'static str)| {
        let id = format!("editor-result-action-{:?}", icon);
        Button::new(id)
            .ghost()
            .small()
            .icon(icon)
            .tooltip(tooltip)
            .disabled(true)
            .debug_selector(|| format!("editor-result-action-{:?}", icon))
            .into_any_element()
    };
    let separator = || {
        div()
            .w_px()
            .h_3()
            .flex_none()
            .bg(border)
            .into_any_element()
    };
    // 写操作段：保存 · 取消 · 编辑 · 新增行 · 复制行 · 删除行 · 撤销 · 重做。
    // 保存 / 取消复用工具栏的提交 / 回滚对（状态栏不重复挂载），槽位保留禁用占位；
    // 编辑 / 新增 / 复制 / 删除 / 撤销 / 重做由面板注入真钮，未注入（只读结果集等）回退占位。
    let write_ops = vec![
        placeholder_btn(placeholders[0]),
        placeholder_btn(placeholders[1]),
        placeholder_btn(placeholders[2]), // 编辑（Pencil，禁用占位）
        controls
            .insert_row
            .unwrap_or_else(|| placeholder_btn(placeholders[3])),
        controls
            .duplicate_row
            .unwrap_or_else(|| placeholder_btn(placeholders[4])),
        controls
            .delete_row
            .unwrap_or_else(|| placeholder_btn(placeholders[5])),
        controls
            .undo
            .unwrap_or_else(|| placeholder_btn(placeholders[6])),
        controls
            .redo
            .unwrap_or_else(|| placeholder_btn(placeholders[7])),
    ];
    // 翻页段占位：首页 · 上页（下页 = 现有取下一段真钮；末页占位）
    let pager_before = [
        placeholder_btn(placeholders[FIRST_PAGER_ACTION]),
        placeholder_btn(placeholders[FIRST_PAGER_ACTION + 1]),
    ];
    let last_pager = placeholder_btn(placeholders[FIRST_SYSTEM_ACTION - 1]);
    // 系统段：导出 · 刷新（2026-10-28 从工具栏移入状态栏；面板注入真钮，
    // 未注入（无导出上下文 / 不可刷新）回退禁用占位——占位仍占位，布局不跳）
    let refresh_placeholder = (IconName::RotateCw, "刷新（重跑当前 SQL）");
    let system_ops = vec![
        controls
            .export
            .unwrap_or_else(|| placeholder_btn(placeholders[FIRST_SYSTEM_ACTION])),
        controls
            .refresh
            .unwrap_or_else(|| placeholder_btn(refresh_placeholder)),
    ];

    div()
        .h_flex()
        .items_center()
        .gap_2()
        .px_2()
        .h(rems(ui::RESULT_STATUS_BAR_HEIGHT))
        .text_xs()
        .text_color(muted)
        .debug_selector(|| "editor-result-status-row".to_string())
        // 最左：左栏开关（面板注入；`None` 不摆——无左栏语义时不注入）
        .children(left_switch)
        // 左·动作组（写操作 ‖ 翻页 ‖ 系统，段间发丝分隔）
        .child(
            div()
                .h_flex()
                .items_center()
                .gap_1()
                .flex_none()
                .children(write_ops)
                .child(separator())
                .children(pager_before)
                // 【B5b】下页 = 取下一段真钮（只在还有下一段时由面板注入）
                .children(controls.more)
                // 【DBeaver 对齐】取全部行（一次性抓全量）
                .children(controls.fetch_all)
                .child(last_pager)
                .child(separator())
                .children(system_ops),
        )
        // 中段·行数框（DBeaver：输入行号跳转；未注入 InputState 时摆只读文案）
        .child(
            div()
                .h_flex()
                .items_center()
                .justify_center()
                .flex_none()
                .w(rems(ui::RESULT_ROW_COUNT_BOX_WIDTH))
                .border_b(ui::HAIRLINE)
                .border_color(border)
                .children(row_count_input.as_ref().map(|state| {
                    gpui_kit::component::input::Input::new(state)
                        .with_size(gpui_kit::component::Size::Small)
                        .into_any_element()
                }))
                .when(row_count_input.is_none(), |d| {
                    d.child(SharedString::from(left))
                }),
        )
        // 右·元信息（【状态栏三项】只显示 内存 / 查询时间 / 查询条数，其它不显示；
        // 右段弹性收缩 + 每项 truncate：宽度不足时截断而非重叠/溢出）
        .child(
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .min_w_0()
                .ml_auto()
                .when_some(mem_text, |row, text| {
                    row.child(
                        div()
                            .min_w_0()
                            .truncate()
                            .child(SharedString::from(text)),
                    )
                })
                .when_some(elapsed, |row, text| {
                    row.child(
                        div()
                            .min_w_0()
                            .truncate()
                            .child(SharedString::from(text)),
                    )
                })
                .when_some(row_text, |row, text| {
                    row.child(
                        div()
                            .min_w_0()
                            .truncate()
                            .child(SharedString::from(text)),
                    )
                }),
        )
        // 最右：右栏开关（面板注入；`None` 不摆——文本视图无右栏语义时不注入）
        .children(right_switch)
        .into_any_element()
}
