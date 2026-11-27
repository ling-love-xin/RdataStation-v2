//! 结果状态 / 渲染域（`grid/` 第十块）：工具栏（`ResultToolbar`）、状态行
//! （`ResultStatus`）、提示文案、`ResultControls` / `ResultPane` 结构与 `render` 家族。
//! 注：`crate::status`（根模块）提供 `elapsed_text` 等通用文案，与本文件同名不冲突。

use gpui_kit::assets::IconName;
use gpui_kit::base::Disableable as _;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::table::{DataTable, TableState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::delegate::{RESULT_GRID_CONTEXT, ResultGridDelegate};
use crate::ports::StatusCommand;
use crate::status::elapsed_text;
use crate::ui;
use ::shared::CurrentTier;
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
    /// 【选中统计】选中的行数（DBeaver `IResultSetSelectionExt.getSelectedRowCount`；
    /// 本仓单区域选中：行选中 / 格选中都算 1 行，未选中 0）
    pub selected_rows: usize,
    /// 【选中统计】选中的列数（本仓：格选中 1；整行选中 = 可见列数）
    pub selected_columns: usize,
    /// 【选中统计】选中的单元格数（本仓：格选中 1；整行选中 = 可见列数）
    pub selected_cells: usize,
}

/// 【DBeaver 对齐】状态行选中统计的三项开关
/// （`resultset.show.selected.rows/columns/cells`，`ResultSetPreferences.java:64-66`；
/// 出厂全关，见 `DataEditorsPreferencesInitializer.java:86-88`）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SelectionStatPrefs {
    pub rows: bool,
    pub columns: bool,
    pub cells: bool,
}

/// 状态行选中统计段（**纯函数**；对齐 `ResultSetStatListener.java:80-92`）。
///
/// 只拼启用的项，顺序固定 行 → 列 → 单元格，项间 `", "`（DBeaver `StringJoiner(", ")`）；
/// 文案照中文包 `行数: {0}` / `列数: {0}` / `单元格数: {0}`
/// （`ResultSetMessages_zh.properties:633-637`，注意是半角冒号 + 空格）。
/// 三项全关 → 空串：状态行不摆这一段（出厂状态，与用户 2026-10-28「只显示三项」口径一致）。
pub fn selection_statistics(status: &ResultStatus, prefs: SelectionStatPrefs) -> String {
    let mut parts: Vec<String> = Vec::new();
    if prefs.rows {
        parts.push(format!("行数: {}", thousands(status.selected_rows)));
    }
    if prefs.columns {
        parts.push(format!("列数: {}", thousands(status.selected_columns)));
    }
    if prefs.cells {
        parts.push(format!("单元格数: {}", thousands(status.selected_cells)));
    }
    parts.join(", ")
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
pub const WINDOW_FETCH_NOTICE: &str = "分段加载：每段重跑查询；无稳定 ORDER BY 时行可能重复或跳过";

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

/// 底部动作状态行的**占位动作钮**（写操作段仍未接线，翻页段已由面板注入真钮）。
///
/// 顺序对齐 DBeaver 底部动作条（`plugin.xml` 的三个 status 子贡献点
/// `editCmds` / `navCmds` / `exportCmd`，1380-1522），三段：
/// - 写操作段（`0..FIRST_PAGER_ACTION`）：**提交** · **回滚** · 撤销 · 重做 ‖ 修改 · 新增行 ·
///   复制行 · 删除行（DBeaver 这里有个 `rowCmds.separator` 竖分隔：前四颗是变更集动作、
///   后四颗是行级动作）；
/// - 翻页段（`FIRST_PAGER_ACTION..FIRST_SYSTEM_ACTION`）：首页 · 上页 · 下页 · 末页
///   ——**四颗都已接线**（面板注入 `first_page` / `prev_page` / `next_page` / `last_page`，
///   纯视图滚动）；其后才是取下一段（`controls.more`，DBeaver `fetch.page`）与取全部
///   （`controls.fetch_all`，DBeaver `fetch.all`），与 DBeaver 同序（都不进占位清单）；
/// - 系统段（`FIRST_SYSTEM_ACTION..`）：导出。
///
/// 未注入时（无结果集）回退到这里，按钮一律 `disabled(true)` 占位。
pub fn status_action_placeholders() -> Vec<(IconName, &'static str)> {
    vec![
        // 写操作 · 变更集
        (IconName::Save, "提交（待行级模型落地）"),
        (IconName::X, "回滚待提交变更"),
        (IconName::Undo2, "撤销"),
        (IconName::Redo2, "重做"),
        // 写操作 · 行级
        (IconName::Pencil, "修改选中格（双击单元格进入编辑）"),
        (IconName::Plus, "新增行（暂未支持下发）"),
        (IconName::CopyPlus, "复制行（暂未支持下发）"),
        (IconName::Trash, "删除行（暂未支持下发）"),
        // 翻页
        (IconName::SkipBack, "首页（滚到第 1 行）"),
        (IconName::ChevronLeft, "上一页（上移一屏）"),
        (IconName::ChevronRight, "下一页（下移一屏）"),
        (IconName::SkipForward, "末页（滚到已抓到的最后一行）"),
        // 系统
        (IconName::Download, "导出"),
    ]
}
/// 占位动作钮分组下标的分界（写操作段 `0..FIRST_PAGER_ACTION`；翻页段 `FIRST_PAGER_ACTION..FIRST_SYSTEM_ACTION`；
/// 系统段 `FIRST_SYSTEM_ACTION..`）。render 层据此在段间插发丝分隔线。
pub(crate) const FIRST_PAGER_ACTION: usize = 8;
pub(crate) const FIRST_SYSTEM_ACTION: usize = 12;

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
    /// 【分页】首页（滚到第 1 行；面板注入，无结果集时不注入 → 回退禁用占位）
    pub first_page: Option<AnyElement>,
    /// 【分页】上一页（以可见首行为基准上移一屏；同上）
    pub prev_page: Option<AnyElement>,
    /// 【分页】下一页（以可见首行为基准下移一屏；到底且还有下一段时顺带取数；同上）
    pub next_page: Option<AnyElement>,
    /// 【分页】末页（滚到已抓到的最后一行；同上）
    pub last_page: Option<AnyElement>,
    /// 【第 57 批】修改当前选中单元格（DBeaver 状态条 `row.edit`；面板注入：只读结果集
    /// 不注入 → 回退禁用占位钮）
    pub edit_row: Option<AnyElement>,
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
    /// 【计算行数】状态行中段那颗动作钮（对齐 DBeaver `rowCountLabel:2155-2180`：
    /// `ActiveStatusMessage` + `UIIcon.COMPILE`，点它异步 `readRowCount`，算完把数字
    /// **就地显示在标签上**）。面板构造：点击 → 宿主跑 `count(*)`；`None` = 不摆
    pub count_rows: Option<AnyElement>,
}

/// 结果区一次要画的东西（面板一次读齐；`render` 只负责画）
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
    /// 外层选择器（默认 `editor-result-pane`）。同一面板里出现第二个结果区时
    /// （分区页的内嵌结果集）必须换名，否则测试用的 `debug_bounds` 会拿到先渲染的那个。
    pub pane_selector: &'static str,
    /// 网格层选择器（默认 `editor-result-grid`），理由同上
    pub grid_selector: &'static str,
}

impl Default for ResultPane {
    fn default() -> Self {
        Self {
            toolbar: ResultToolbar::default(),
            status: None,
            controls: ResultControls::default(),
            card: None,
            tabs: None,
            notice: None,
            row_count_input: None,
            pane_selector: "editor-result-pane",
            grid_selector: "editor-result-grid",
        }
    }
}

/// 【DBeaver 对齐】渲染工具栏（横跨整个顶部）
///
/// 从 `result_grid::render` 里抽出来，放到最顶部——用户反馈「工具栏遮挡了结果集的字段」。
/// 2026-10-28 重构（组件统一化 §3）：
/// - 右段纯按钮序列 → `toolbar::Toolbar` 组件（统一容器 + a11y；按钮已是标准 Button /
///   DropdownButton，经 `.contents()` 挂入，保留各自 ghost/small 外观）
/// - 左段 = 本地筛选（复合体：第 1 格「显示 SQL」+ 展开 + 输入 + 运行 + 清除，
///   由面板自己拼好；工具栏只当容器）
/// - 【1.3 对齐】下发源库 / 注释两个开关也收进右段 Toolbar（统一容器管高度与
///   基线，避免开关默认档与按钮小号高度不一致导致的错位）；DBeaver 对齐后两者
///   已改成图标 toggle（按钮本体在 `panel/render.rs` 构造，这里只负责容器分组）
/// - 执行信息（行数/耗时/失败/血缘/连接…）**不在工具栏**：移入状态栏（见
///   `render_status_row` 的 `memory` / `affected_rows` 参数）——用户口径：执行信息归结果集状态栏
/// - 导出 / 刷新同样移入状态栏（左动作组系统段），工具栏不再摆
pub fn render_toolbar(
    filter: Option<AnyElement>,
    // 【DBeaver 对齐 2026-11-25】框外工具条（`ResultSetFilterPanel.filterToolbar`）。
    // 它是**外层 GridLayout 的第 2 列**（`ResultSetFilterPanel.java:163` `new GridLayout(2, false)`），
    // 与带边框的 `filterComposite`（第 1 列，只含 5 格）是**两个并列控件**：
    // 边框框不住工具钮，工具钮也不在输入框里。此前本仓把 clear/settings/back/forward/find
    // 全塞进 `filter` 那条带边框的行里，框线绕到了工具钮外面——这是「工具栏看起来不像
    // DBeaver」的结构性原因。
    tools: Option<AnyElement>,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let muted = theme.colors.muted_foreground;
    div()
        .h_flex()
        .items_center()
        .justify_between()
        // 两列间距 5px：DBeaver 外层 `GridLayout(2, false)` 用的是 SWT 默认
        // `horizontalSpacing = 5`（未显式设置，`:163-166` 只改了 margin）。
        .gap(px(5.))
        // 水平内距 3px（`ui::RESULT_TOOLBAR_PADDING`）：DBeaver 的筛选条面板 `marginWidth = 3`
        // （`ResultSetFilterPanel.java:163-166`），此前 `px_2`(8px) 宽了 5px。垂直方向不加，
        // 由「30px 行高居中 24px 筛选区」自然形成 3px（见 `ui.rs` 常量注释）。
        .px(ui::RESULT_TOOLBAR_PADDING)
        // 高度 30px（`ui::RESULT_TOOLBAR_HEIGHT`）= DBeaver 的 `3 + 2 + 20 + 2 + 3` 逐段相等：
        // 右段控件 20px（XSmall 档，`sizing.rs:281-289` 的 `h_5`）+ 筛选区 24px
        // （20 + 边框 1px 与内距 1px 各两道）。**不再与标签条（32px）等高**，
        // 这是 DBeaver 的原生关系：筛选条比它上方的 TabFolder 矮。
        //
        // 【DBeaver 对齐 2026-11-08】底色铺 `tab_bar`：DBeaver 的筛选条与上方 TabFolder
        // 同属「非活动标签 / widget」背景档（`UIStyles.getDefaultWidgetBackground()`，
        // `UIStyles.java:122-129` → 主题色 `INACTIVE_TAB_BG_START`，缺失回退系统
        // `COLOR_WIDGET_BACKGROUND`），而网格是列表底（`getDefaultTextBackground()` →
        // `COLOR_LIST_BACKGROUND`，白）。本仓对应角色就是 `tab_bar.background`
        // （`assets/themes/rds-theme.json`：`#F8F8F8` / `#181818`）。不铺底色时工具栏继承
        // 面板背景（白），与标签条之间断开一层，观感上「没对齐」。
        //
        // 注意：编辑行被虚拟列表裁剪会丢 Input 焦点链（`cell_edit_esc` 曾因此回归），
        // 再压缩工具栏之前必须先量网格实际剩余高度；40→32→30 三次回收的都是控件降档
        // 让出的空白（容器边距与控件间距未动），网格可用高度反而增加。
        .h(rems(ui::RESULT_TOOLBAR_HEIGHT))
        .flex_none()
        .bg(theme.colors.tab_bar)
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
                // 左段吃满剩余宽：DBeaver 左列（筛选复合体）是 `GridData(FILL_HORIZONTAL)`
                // （`ResultSetFilterPanel.java:180`），右列才按内容宽。不给 `flex_1` 时左段
                // 会收缩到内容宽，里面那个 `flex_1` 的筛选框也就撑不开（flex_basis 无处分配）。
                .flex_1()
                .min_w_0()
                // 【1.1】工具栏最左侧是筛选复合体（第 1 格即「显示 SQL」，见
                // `panel/render.rs` 的 `sql_chip`）
                .children(filter),
        )
        // 【DBeaver 对齐 2026-11-25】右段 = `filterToolbar`（框外工具条）。
        // 它是 `ToolBar(SWT.HORIZONTAL | SWT.RIGHT)` 且 GridData 为
        // `HORIZONTAL_ALIGN_BEGINNING | VERTICAL_ALIGN_CENTER`（`ResultSetFilterPanel.java:315-316`）
        // —— 也就是**紧贴第 1 列右侧、按内容宽、垂直居中**，不是推到最右。
        // 本仓 `justify_between` 已把左段吃掉剩余宽（`flex_1`），右段自然紧贴其后。
        .children(tools)
        // 【T1 工具栏收口 2026-11-24】右段业务按钮（对比/分析/提交撤销/复制）不在这里：
        // DBeaver 筛选栏只有左段复合体 + 这条无业务的工具条，动作项全在底部状态条。
        // 本仓对比/分析入口迁到结果集标签页右键（基线文档 §21.1），列注释迁到结果集设置（§14.2）。
        .into_any_element()
}

/// 【工具栏配置】状态栏配置菜单的条目清单（纯函数：顺序 / 文案 / 勾选态）
///
/// DBeaver 的口径是「每个命令一项、勾选 = 显示」；默认勾选态来自
/// [`StatusCommand::default_visible`]（`undo` / `redo` / `fetch.page` 默认不显示），
/// 用户勾选后由面板把该命令从隐藏集合里拿掉。
pub fn status_config_items(
    hidden: &std::collections::BTreeSet<StatusCommand>,
) -> Vec<(StatusCommand, &'static str, bool)> {
    StatusCommand::ALL
        .into_iter()
        .map(|command| (command, command.menu_label(), !hidden.contains(&command)))
        .collect()
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
            .col_movable(true) // 列序拖拽（拖拽列头改变列顺序）
            .sortable(true) // 列头排序（点击列头排序）
            .col_selectable(true) // 选中整列
            .loop_selection(true) // 键盘导航循环选择
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
        pane_selector,
        grid_selector,
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
        .debug_selector(move || pane_selector.to_string())
        // 表头带高度由 delegate 恒给（单行 28 / 两行 50，真值 `GridColumn.getHeaderHeight`）；
        // 见 `ResultGridDelegate::header_row_height`
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
                    .debug_selector(move || grid_selector.to_string())
                    // 【B14】`Ctrl+C` 的上下文只挂这一层（键位在 `crates/app` 注册）：
                    // 焦点在网格里时它才在 dispatch path 上，编辑区里按 `Ctrl+C`
                    // 仍是内核的文本复制。焦点本身不用我们管——组件的 `DataTable`
                    // 自己 `track_focus`，点一下就在它身上了（用例从真点击一路验到剪贴板）
                    .key_context(RESULT_GRID_CONTEXT)
                    // 密度档走常量表（`RESULT_TABLE_SIZE` = `Size::Size(20.)`，真值
                    // `itemHeight = fontMetrics.getHeight() + 3` ≈ 19~20）——不写 size 就落到
                    // 组件默认档 32px（原型稿写的 22px、组件默认 32、旧实现 26 三个数都不一致，
                    // 见常量注释）
                    .child(
                        // 表头带高由 delegate 恒给：单行 28 / 两行（注释或类型名小字）50，
                        // 真值 `GridColumn.getHeaderHeight:196-208`
                        DataTable::new(state)
                            .stripe(true)
                            .bordered(false)
                            .with_size(ui::RESULT_TABLE_SIZE)
                            .when_some(
                                state.read(cx).delegate().header_row_height(),
                                |table, header_height| table.header_row_height(header_height),
                            ),
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
    // 【R6 段大小】「一段」行数输入框（对齐 DBeaver `resultSetSize` Text；
    // `None` = 不摆，只读文案位照旧）
    segment_input: Option<&gpui::Entity<gpui_kit::component::input::InputState>>,
    // 【选中统计】状态行选中统计段（面板用 `selection_statistics` 纯函数算好传入；
    // `None` = 三项全关或空串，不摆）。位置对齐 DBeaver `selectionStatLabel`
    // （`ResultSetViewer.java:2182-2184`，紧跟 `rowCountLabel` 之后）。
    selection_stat: Option<String>,
    elapsed_ms: Option<u64>,
    left_switch: Option<AnyElement>,
    right_switch: Option<AnyElement>,
    // 【状态栏三项】右段只显示 内存 / 查询时间 / 查询条数（用户口径 2026-10-28：
    // 只留这三项，其它信息不显示）。内存文案由面板算好传入（内存档 fact_bytes →
    // 「内存 X」；表档 / 无事实档 = None）；写语句影响行数单独传（有值时条数文案
    // 用「影响 N 行」）。
    memory: Option<String>,
    affected_rows: Option<u32>,
    // 【工具栏配置】被隐藏的命令项（DBeaver 工具条配置的 `defaultVisibility`；
    // 撤销 / 重做 / 取下一段默认在隐藏集合里，用户可在配置菜单里重新打开）
    hidden_commands: &std::collections::BTreeSet<StatusCommand>,
    // 状态行右端最末的配置菜单（面板构造；`None` = 不摆，键位仍可用）
    config_menu: Option<AnyElement>,
    // 状态行最右端的「结果集设置」齿轮（对齐 DBeaver 状态栏 `ConfigAction`；
    // 面板构造，`None` = 不摆）
    settings_button: Option<AnyElement>,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let muted = theme.colors.muted_foreground;
    let (left, _old_right) = status.map(status_segments).unwrap_or_default();
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
    // 【工具栏配置】显隐过滤：对齐 DBeaver 的 `defaultVisibility`（`plugin.xml:1381-1397`），
    // 撤销 / 重做 / 取下一段默认不显示；用户可在状态行末端的配置菜单里勾回来。
    let shown = |command: StatusCommand| !hidden_commands.contains(&command);
    let separator = || div().w_px().h_3().flex_none().bg(border).into_any_element();
    // 写操作段（对齐 DBeaver `toolbar:…status.editCmds`，plugin.xml:1407-1466）：
    // 前半 = 变更集动作「提交 · 回滚 · 撤销 · 重做」，后半（越过 `rowCmds.separator`）
    // = 行级动作「修改 · 新增行 · 复制行 · 删除行」。保存 / 回滚复用工具栏的提交 / 回滚对
    // （状态栏只摆占位，不重复挂载），行级动作与撤销重做由面板注入真钮，未注入回退占位。
    let mut write_change: Vec<AnyElement> = Vec::new();
    if shown(StatusCommand::Commit) {
        write_change.push(placeholder_btn(placeholders[0]));
    }
    if shown(StatusCommand::Reject) {
        write_change.push(placeholder_btn(placeholders[1]));
    }
    if shown(StatusCommand::Undo) {
        write_change.push(
            controls
                .undo
                .unwrap_or_else(|| placeholder_btn(placeholders[2])),
        );
    }
    if shown(StatusCommand::Redo) {
        write_change.push(
            controls
                .redo
                .unwrap_or_else(|| placeholder_btn(placeholders[3])),
        );
    }
    let mut write_row: Vec<AnyElement> = Vec::new();
    if shown(StatusCommand::EditRow) {
        // 【第 57 批】「修改」钮接线：面板注入真钮（进入行内编辑），
        // 只读结果集不注入时回退禁用占位（布局不跳）
        write_row.push(
            controls
                .edit_row
                .unwrap_or_else(|| placeholder_btn(placeholders[4])),
        );
    }
    if shown(StatusCommand::InsertRow) {
        write_row.push(
            controls
                .insert_row
                .unwrap_or_else(|| placeholder_btn(placeholders[5])),
        );
    }
    if shown(StatusCommand::DuplicateRow) {
        write_row.push(
            controls
                .duplicate_row
                .unwrap_or_else(|| placeholder_btn(placeholders[6])),
        );
    }
    if shown(StatusCommand::DeleteRow) {
        write_row.push(
            controls
                .delete_row
                .unwrap_or_else(|| placeholder_btn(placeholders[7])),
        );
    }
    // 翻页段（对齐 `toolbar:…status.navCmds`，plugin.xml:1470-1512）：首页 · 上页 · **下页** ·
    // 末页，其后才是取下一段（`more` ≈ `fetch.page`）与取全部（`fetch_all` ≈ `fetch.all`）——
    // 四颗行导航都在面板注入（纯视图滚动），未注入回退禁用占位。
    let mut pager_ops: Vec<AnyElement> = Vec::new();
    if shown(StatusCommand::First) {
        pager_ops.push(
            controls
                .first_page
                .unwrap_or_else(|| placeholder_btn(placeholders[FIRST_PAGER_ACTION])),
        );
    }
    if shown(StatusCommand::Prev) {
        pager_ops.push(
            controls
                .prev_page
                .unwrap_or_else(|| placeholder_btn(placeholders[FIRST_PAGER_ACTION + 1])),
        );
    }
    if shown(StatusCommand::Next) {
        pager_ops.push(
            controls
                .next_page
                .unwrap_or_else(|| placeholder_btn(placeholders[FIRST_PAGER_ACTION + 2])),
        );
    }
    if shown(StatusCommand::Last) {
        pager_ops.push(
            controls
                .last_page
                .unwrap_or_else(|| placeholder_btn(placeholders[FIRST_PAGER_ACTION + 3])),
        );
    }
    let fetch_page = shown(StatusCommand::FetchPage).then(|| controls.more).flatten();
    let fetch_all = shown(StatusCommand::FetchAll).then(|| controls.fetch_all).flatten();
    // 系统段拆成两颗、各归其位（真值组序见 `createStatusBar0`）：刷新是**最左第一组**
    // （`rsToolbar` 的 `populateRefreshButton`，`ResultSetViewer.java:2059-2074`），导出走的是
    // 行导航之后的「添加 / 导出」组（`TOOLBAR_EXPORT_CONTRIBUTION_ID` + `TOOLBAR_GROUP_ADDITIONS`
    // + `TOOLBAR_CONTRIBUTION_ID`，`:2098-2111`）——此前合成一「系统段」摆在末尾，与真值不符
    let refresh_placeholder = (IconName::RotateCw, "刷新（重跑当前 SQL）");
    let refresh_op = shown(StatusCommand::Refresh)
        .then(|| controls.refresh.unwrap_or_else(|| placeholder_btn(refresh_placeholder)));
    let export_op = shown(StatusCommand::Export).then(|| {
        controls
            .export
            .unwrap_or_else(|| placeholder_btn(placeholders[FIRST_SYSTEM_ACTION]))
    });
    // 分隔线只在两侧都还有内容时才画（DBeaver 用 `visibleWhen` 做同一件事，
    // 全隐一段时不该留一条孤线）
    let change_empty = write_change.is_empty();
    let row_empty = write_row.is_empty();
    let mut left_ops: Vec<AnyElement> = Vec::new();
    left_ops.extend(refresh_op);
    if !left_ops.is_empty() && !(change_empty && row_empty) {
        left_ops.push(separator()); // 刷新组之后才有东西
    }
    left_ops.append(&mut write_change);
    if !change_empty && !row_empty {
        left_ops.push(separator()); // DBeaver `rowCmds.separator`（plugin.xml:1434）
    }
    left_ops.append(&mut write_row);
    if !left_ops.is_empty() && !pager_ops.is_empty() {
        left_ops.push(separator());
    }
    left_ops.append(&mut pager_ops);
    left_ops.extend(fetch_page);
    left_ops.extend(fetch_all);
    if !left_ops.is_empty() && export_op.is_some() {
        left_ops.push(separator());
    }
    left_ops.extend(export_op);

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
        // 左·动作组（写操作 ‖ 翻页 ‖ 系统，段间发丝分隔；已按配置菜单的显隐过滤）
        .child(
            div()
                .h_flex()
                .items_center()
                .gap_1()
                .flex_none()
                .children(left_ops),
        )
        // 齿轮在「段大小」之左（真值 `ToolbarSeparatorContribution(true)` + `ConfigAction`
        // 位于添加/导出组之后、`resultSetSize` 之前，`ResultSetViewer.java:2113-2122`）——
        // 此前挂在一行最右端，与「两栏开关」的本仓增强混排了
        .when_some(settings_button, |row, btn| {
            row.child(separator()).child(btn)
        })
        // 中段·段大小框（DBeaver `resultSetSize` Text：默认值占位，改成 >0 的行数才生效）
        .children(segment_input.map(|state| {
            div()
                .h_flex()
                .items_center()
                .flex_none()
                .w(rems(ui::RESULT_SEGMENT_BOX_WIDTH))
                // 调试选择器挂在**外层行**：`Input` 自己是 RenderOnce 组件，其内部元素
                // 的 id 进不了观测（与页签条同一口径，见 panel/tests.rs 的环境事实注释）
                .debug_selector(|| "editor-result-segment-size".to_string())
                .child(
                    div()
                        .text_color(muted)
                        .flex_none()
                        .child(SharedString::from("段")),
                )
                .child(
                    div().flex_1().min_w_0().child(
                        gpui_kit::component::input::Input::new(state)
                            .with_size(gpui_kit::component::Size::Small),
                    ),
                )
        }))
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
        // 中段·「计算行数」（对齐 DBeaver `resultSetSize` 之后那颗 `rowCountLabel`：
        // 点一下对原 SQL 跑 `count(*)`，算完把总数就地显示在这颗钮上）
        .children(controls.count_rows)
        // 中段·选中统计（对齐 DBeaver `selectionStatLabel`：`rowCountLabel` 之后紧邻；
        // 文本由 `selection_statistics` 按三项开关拼好，全关时这里不摆）
        .when_some(selection_stat, |row, text| {
            row.child(
                div()
                    .flex_none()
                    .truncate()
                    .debug_selector(|| "editor-result-selection-stat".to_string())
                    .child(SharedString::from(text)),
            )
        })
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
                    row.child(div().min_w_0().truncate().child(SharedString::from(text)))
                })
                .when_some(elapsed, |row, text| {
                    row.child(div().min_w_0().truncate().child(SharedString::from(text)))
                })
                .when_some(row_text, |row, text| {
                    row.child(div().min_w_0().truncate().child(SharedString::from(text)))
                }),
        )
        // 最右：右栏开关（面板注入；`None` 不摆——文本视图无右栏语义时不注入）
        .children(right_switch)
        // 【工具栏配置】最末端的 ▾：勾选要显示的命令项（DBeaver 工具条配置的等价物）
        .children(config_menu)
        .into_any_element()
}
