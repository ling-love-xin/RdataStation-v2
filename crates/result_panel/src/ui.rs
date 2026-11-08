//! 结果面板尺寸常量（**rem 倍率**；视图里用 `rems(...)` 换算）
//!
//! 为什么住在本 crate 而不是 `workbench_shell::ui`：结果区尺寸是**结果面板私有**的
//! 结构尺寸（shell 只收跨面板共用资产）；与 editor 的 `ui.rs` 同规则——
//! **结构尺寸进常量表 → 视图换算引用**，局部间距直接用 Tailwind 尺度方法，
//! 视图里不得出现裸 `px(N.)`。
//! 注意 `rems(x)` 的基准是主题字号（默认 16px），与 `gap_1`（=4px）不是一套单位。

use gpui_kit::component::Size as ComponentSize;
use gpui_kit::{Pixels, px};

/// 固定描边（1px，不随字号缩放）：与全局 `workbench_shell::ui::HAIRLINE` 同一值，
/// 结果面板直接沿用（单一来源在 shell，这里只做重导）。
pub use workbench_shell::ui::HAIRLINE;

/// 状态点直径（0.375rem = 6px）：结果集失败点
pub const STATUS_DOT_SIZE: f32 = 0.375;

/// 结果区默认高度（20rem = 320px，约占窗口高度的 40-50%；可拖拽后以用户拖到的高度为准）
///
/// 结果区出现/消失时编辑区高度会跳一下，但比“按比例分栏”在无结果时不占位更符合
/// “只显示真实内容”。拖拽改高走组件库的 `ResizablePanel`（不手搓拖动条）。
///
/// 2026-11-02 收敛：这一档原先在 `editor/src/support/ui.rs` 另有一份（20.0）。两份里
/// **生效的是 editor 那份**（`view/host/mod.rs` 用它初始化 `result_height`），本文件这份
/// 全仓无人引用、是死值，且 18.0 与 20.0 已经分叉。现统一为 20.0 并由 editor re-export，
/// 单一来源在本文件（依赖方向 `editor → result_panel`，正向引用成立）。
///
/// 历史教训（8rem→10rem 那版，来自 editor 侧注释）：工具栏迁 Toolbar 组件后高度
/// 24px→36px，8rem 下结果主体只剩 44px、编辑行被虚拟列表裁剪会丢 Input 焦点链
/// （`cell_edit_esc` 回归实测）；12rem 又过大。20rem 是这两个教训之后的落点。
pub const RESULT_PANE_HEIGHT: f32 = 20.0;

/// 结果区可拖拽的最大高度（48rem = 768px：再高编辑区就没地方了）
pub const RESULT_MAX_HEIGHT: f32 = 48.0;

/// 结果区状态行高（1.5rem = 24px，与编辑器状态栏同档；结果工具栏同高）
pub const RESULT_STATUS_BAR_HEIGHT: f32 = 1.5;

/// 右栏面板自带工具条高（1.75rem = 28px）。
/// 来源：vendor `toolbar.rs:242-259` 的 XSmall 档自带 `min_h_7`（= 28px），即
/// gpui-kit `Toolbar` 组件在 XSmall 下的实际托底高度。对齐 DBeaver 侧面板自带
/// 工具条（`ResultSetViewer.java:1337-1374` 的 `ToolBarManager(SWT.RIGHT)`）时，
/// 容器高必须与组件托底高一致：写 24px（状态行档）会让 28px 的 Toolbar 溢出，
/// 压住下方表头那一行。
pub const RESULT_PANEL_TOOLBAR_HEIGHT: f32 = 1.75;

/// 结果区工具栏高（2.5rem = 40px）
///
/// 2026-10-28 工具栏迁 `Toolbar` 组件后独立成档：当时按「`min_h_7`(28px) + `p_1`(8px)」
/// 算成 36px 给档。
///
/// 2026-11-08 抬到 40px：筛选框 `Input` 当时还是默认 Medium 档（`h_8` = 32px），
/// 32 + Toolbar 的 `p_1` 上下 8px = 40，故定 40px。
///
/// **2026-11-08 收回到 32px（对齐 DBeaver）**：上面两个算术都不对，真实高度是
/// **由内容撑出来的**——
/// - `Toolbar` XSmall 是 `min_h_7`(28) + `p_1`(4px 全向)（vendor `toolbar.rs:253`），
///   `min_h` 是**最小总高**不是内容高，所以它不是 36；
/// - 右段所有控件（`Button` / `DropdownButton` 的 `.small()`）真实高 24px
///   （vendor `button.rs:688-700`：图标按钮 `size_6`、带文字 `h_6`），加 `p_1` 上下
///   8px → 自然高 **32px**；
/// - 筛选框 `Input` 已降到 `.small()`（24px），不再需要 Medium 档那 8px。
///
/// 于是 40px 里多出的 8px 是 Input 降档后没回收的空白（上下各 4px），观感上工具栏
/// 比标签条"虚胖"。收回 32px 后与 [`RESULT_TABS_HEIGHT`] 等高，标签条 + 工具栏连成
/// 一条整齐的 chrome 带（DBeaver 这两块同高、同 widget 底色）。
/// 工具栏高 30px。DBeaver `ResultSetFilterPanel` 没有总高常量，但它由四圈 margin
/// 加上一个**明确的 20px 行高**撑出来：外层 `marginHeight = 3`（`:164`）×2 +
/// `filterComposite` 的 `marginHeight = 2`（`:175`）×2 + `MIN_FILTER_TEXT_HEIGHT = 20`
/// （`:103`，同时也是展开 / 执行 / 历史三个面板的 `heightHint`，见 `:1067/:1304/:1351`）
/// = 30px。本仓控件取同一 20px 档（`Input::xsmall()` / `Button::xsmall()`，见
/// `vendor/gpui-component/src/sizing.rs:281-289` 的 `h_5`），容器 1px 边框 + 1px 内距
/// 得 24px，在 30px 里上下各 3px，与 DBeaver 的 `3 + 2 + 20 + 2 + 3` 逐段相等。
pub const RESULT_TOOLBAR_HEIGHT: f32 = 1.875;

/// 结果工具栏水平内距（3px）
///
/// 【DBeaver 对齐 2026-11-08】DBeaver 的筛选条面板是 `GridLayout(2, false)` 且
/// `marginHeight = 3` / `marginWidth = 3`（`ResultSetFilterPanel.java:163-166`），
/// 四边各留 3px。此前这里用 `px_2`（8px，Tailwind 档位），比 DBeaver 宽 5px。
///
/// 只给水平：垂直方向 DBeaver 的 3px 由「面板 26px 内容 + 32px 行高居中」自然形成
/// （我们的筛选区外框 = Input 24px + 上下各 1px 边框 = 26px，32px 行里居中 → 上下各 3px）。
/// 不能给 `.py()`：右段 `Toolbar` 的自然高就是 32px（`min_h_7` 28 + `p_1` 上下 8 的内容高），
/// 一旦把可用高度压到 26px 就会裁掉它的内边距。
pub const RESULT_TOOLBAR_PADDING: Pixels = px(3.);

/// 结果集标签条高（2rem = 32px）
///
/// 2026-11-08 抬到 32px：此前是 1.5rem(24px)，与 `TabBar::with_size(Size::Small)`
/// 的 Tab 档高**正好相等**，容器高 == 组件高、零余量，看着比编辑器的文件标签条矮一截。
/// 编辑器文件标签条走 dock 的 `TabBar::new("tab-bar")`，**没写 size** → `Size::default()`
/// = Medium，`TabVariant::Tab` 在 Medium 档是 32px（vendor `tab/tab.rs:79-82`）。
/// 所以这里把结果集也放到 Medium 档，高度 / gap（Medium 12px vs Small 8px）/ 内边距
/// （Medium 12px vs Small 10px）三样都与编辑器同档，字号不变（两档都是 `text_sm` 14px）。
pub const RESULT_TABS_HEIGHT: f32 = 2.0;

/// 底部动作状态行**中段行数框**宽（4.5rem = 72px：只读显示「共 N[+] 行」，
/// 行号跳转输入是 backlog；窄框对齐 DBeaver 状态行中段的小输入槽位）
pub const RESULT_ROW_COUNT_BOX_WIDTH: f32 = 4.5;

/// 筛选输入框宽（11.25rem = 180px；原型 §5.5 的 `⌕ 筛选`，够放下十来个字符的词）
///
/// 【2026-11-08 起只剩一个消费者】工具栏筛选框已改为 `flex_1` 吃满剩余宽（对齐 DBeaver
/// 左列 `GridData(FILL_HORIZONTAL)`，`ResultSetFilterPanel.java:180`），本常量现在只给
/// **结果集重命名输入框**用（`panel/render.rs` 的重命名条）。
pub const RESULT_FILTER_WIDTH: f32 = 11.25;

/// 结果网格列宽（128px；`Column::width` 只收 `Pixels`，故这里不做 rem 换算）
pub const RESULT_COLUMN_WIDTH: Pixels = px(128.);

/// 类型默认列宽档（用户**没拖过、也没双击自适应过**的列用哪一档）
///
/// 口径：按 `ColumnKind`（归一化语义类型）选，不按驱动原始类型名——MySQL 报 `int`、
/// PG 报 `int4`，语义都是"数字列"，宽该一样。数值推导见
/// `docs/architecture/editor/result-panel.md` §8.24；未知类型仍走
/// [`RESULT_COLUMN_WIDTH`]（128），守住「类型缺失时一个像素都不变」的既有原则。
pub const RESULT_COLUMN_WIDTH_NUMERIC: Pixels = px(130.);

/// 布尔列（`true` / `TRUE` / 中文映射都很短，宽的是表头 badge + 列名 + 列头菜单）
///
/// 2026-11-08 96 → 120：96px 是按「布尔值本身短」给的，但列宽真正要装的是**表头**。
/// 算术（`vendor/gpui-component/src/sizing.rs` XSmall 档 + `grid/header.rs` 构成）：
/// 96 − 8（组件左右 padding）= 88；再扣 badge 约 19（11px 符号 + `px_1` 8）、
/// 列名与 badge / 菜单之间两个 `gap_1` = 8、列头菜单 24 → 列名只剩 **约 37px**，
/// 也就是 5–6 个西文字符（`is_active` 会渲染成 `is_a…`）。120 下留 61px（约 9 字符）。
pub const RESULT_COLUMN_WIDTH_BOOLEAN: Pixels = px(120.);

/// 日期 / 时间列（`2026-11-08` / `12:34:56` 十来字符）
pub const RESULT_COLUMN_WIDTH_DATE_TIME: Pixels = px(120.);

/// 时间戳列（`2026-11-08 12:34:56.789 +08` 二十多字符）
pub const RESULT_COLUMN_WIDTH_TIMESTAMP: Pixels = px(210.);

/// UUID 列（36 字符，不截断能读全）
pub const RESULT_COLUMN_WIDTH_UUID: Pixels = px(300.);

/// JSON 列（美化成多行前，首行常见一个对象头）
pub const RESULT_COLUMN_WIDTH_JSON: Pixels = px(260.);

/// 二进制列（我们画的是 `BLOB · N B` 摘要而非整串 hex，所以不照抄 tusk 的 bytea 220）
pub const RESULT_COLUMN_WIDTH_BINARY: Pixels = px(160.);

/// 文本列（长文本靠悬停全文与自适应，初值只给个能读一小段话的宽度）
pub const RESULT_COLUMN_WIDTH_TEXT: Pixels = px(200.);

/// 结果网格首列的行号槽宽（48px，原型 §2.4 的固定 `#` 列）
pub const RESULT_ROW_NUMBER_WIDTH: Pixels = px(48.);

/// 结果网格的**密度档**（组件尺寸，不是 rem 倍率）。
///
/// 组件给的表行高只有 26 / 30 / 32 / 40 四档（`XSmall` / `Small` / `Medium`（默认）/ `Large`）；
/// 取 `XSmall` = 26px：这是数据网格应有的密度，也与 Mock 预览表（`crates/mock/src/ui.rs` 的
/// `PREVIEW_TABLE_SIZE`）同档。
///
/// 订正（2026-09-20）：原型稿 §8 写的「行高 22px（V1 AG Grid）」被实现默默落到了组件**默认档**
/// 32px——三方面三个数（稿子 22 / 实现 32 / 预览表 26），而这一档此前根本没有常量。
/// 精确给 22px 虽然可行（`Size::Size(px(22.))`），但组件会按 `size * 0.875` 反推字号（19px），
/// 不划算；所以口径统一到组件的 `XSmall`。
/// 两个表格共用这一档：改这里就是改「表格类组件的行高」这一条口径。
pub const RESULT_TABLE_SIZE: ComponentSize = ComponentSize::XSmall;

/// 结果网格列最小宽（80px）
///
/// 2026-11-08 64 → 80：64px 时列头 badge(19) + 菜单(24) + 两个 gap(8) = 51 已经吃掉
/// 56px 可用宽（64 − 8 组件 padding）里的一大半，列名只剩 **5px**——拖到最窄的列
/// 完全读不出列名，连省略号都摆不下。最小宽的职责是**兜住不可用态**、不是保证可读：
/// 80px 下留 21px（约 3 个西文字符 + 省略号），看全名靠悬停 tooltip。
pub const RESULT_COLUMN_MIN_WIDTH: Pixels = px(80.);

/// 列宽自适应（`grid/fit.rs`）：单元格自身左右内边距合计（`render_td` 的 `px_2`）。
///
/// 组件还会按 [`RESULT_TABLE_SIZE`]（XSmall 档，左右各 4px）再加 8px——两层合起来才是
/// 肉眼看到的余量。下面三档原先散在 `fit.rs` 里，按「结构尺寸进常量表」收上来。
pub const RESULT_FIT_CELL_PADDING: Pixels = px(16.);

/// 列宽自适应：表头 `main_row` 的 `gap_1`（badge / 列名 / 排序箭头 / 筛选 ▽ 之间）
pub const RESULT_FIT_HEADER_GAP: Pixels = px(4.);

/// 列宽自适应：类型 badge 的 `px_1` 左右合计
pub const RESULT_FIT_BADGE_PADDING: Pixels = px(8.);

/// 结果网格单元格 / 表头悬停全文的最大宽度（24rem = 384px）。
///
/// 悬停提示跟着鼠标走，比这更宽就会被窗口边缘截掉；超出的部分**折行**（长 JSON / 长文本
/// 要能从头读到尾，截尾就失去看全的意义）。与 Mock 预览表同值同口径
/// （`crates/mock/src/ui.rs` 的 `PREVIEW_TOOLTIP_MAX_WIDTH`）：两张表是同一类东西，
/// 悬停提示的宽度不一致会在并列对照时显形。
pub const RESULT_TOOLTIP_MAX_WIDTH: f32 = 24.0;

/// 值查看器 JSON 展开树：每层缩进（0.75rem = 12px）
pub const JSON_TREE_INDENT_STEP: f32 = 0.75;

/// 值查看器 JSON 展开树：箭头指示符宽度（0.8rem = 12.8px，folder 与非 folder 行对齐）
pub const JSON_TREE_ARROW_WIDTH: f32 = 0.8;

/// 表头带抬到「两行」时的高度（40px）
///
/// **只有「注释开关开启」这一档才用**（`ResultGridDelegate::header_row_height`）；单行
/// 表头不给覆盖值，用组件默认 `table_row_height()` = [`RESULT_TABLE_SIZE`] 档 = 26px。
///
/// 为什么是 40：组件按 `DataTable::header_row_height(px)` 给整条表头带（`header_band_height`
/// = 该值 × 列组行数，`vendor table/state.rs:807-818`），`render_cell` 再吃掉上下 padding
/// 4px → 内容高 36px。两行各 18px（`text_xs` 的行高）刚好装下。
///
/// 历史：这里原先有个 `RESULT_HEADER_HEIGHT = 2.0`（2rem = 32px，"结果网格列名行高"），
/// 全仓零引用——表头高一直由组件按密度档给 26px，那个常量是设计稿口径的遗留，**32 也与
/// 现实不符**（26px 连一行 badge + 列名都紧）。2026-11-08 删掉它，改成这个真生效的覆盖值。
pub const RESULT_HEADER_COMMENT_ROW_HEIGHT: Pixels = px(40.);

/// 表头**类型小标签**的字号倍率（0.6875rem = 11px）
///
/// 比单元格正文与列名（`text_xs` = 12px）再小一档：规格要求它“小字、次级色”，而表头行
/// 只有一行高度（组件按密度档给 26px），两行排不下——所以做成同行内、比列名小一档的标签，
/// 层级靠**字号 + 族色**两条分开。`grid/header.rs` 画 badge、`grid/fit.rs` 量 badge 符号宽
/// 都用这一档（两边字号必须一致，否则双击自适应算出的宽度会和实际渲染差一两个字符）。
pub const RESULT_HEADER_TYPE_FONT_SIZE: f32 = 0.6875;

/// 结果区最小高度（7rem = 112px：工具栏 24 + 多结果时的标签条 24 + 至少两行网格）
///
/// 拖拽不许把它压得比这更小——再小就只剩一条工具栏，看不出自己看的是哪份结果。
pub const RESULT_MIN_HEIGHT: f32 = 7.0;

/// 值查看器弹层：值显示区最大高（22rem = 352px：约 20 行等宽文本；
/// 再长就滚动——值查看器要的是「原样看全」，不是截断）
pub const VALUE_PREVIEW_MAX_HEIGHT: f32 = 22.0;

/// 结果面板**左缘竖条**宽（2.25rem = 36px，对齐 DBeaver 左缘独立窄条）：
/// 网格 / 文本 / 记录三图标竖排互斥；常驻网格左缘，不与列结构栏挤在同一栏
pub const RESULT_VERTICAL_BAR_WIDTH: f32 = 2.25;

/// 结果面板**右侧辅助栏**宽（20rem = 320px）：列浏览器清单 / 选中格完整值 / 计算 / 分组
/// 共用这一栏、页签切换。比清单单栏宽一档：长文本 / JSON 要在这里尽量多显示
/// （截断交给网格，详情看全）。
pub const RESULT_SIDEBAR_RIGHT_WIDTH: f32 = 20.0;

/// 右栏「计算 / 列浏览器明细」键值行的**标签列宽**（4.5rem = 72px）：
/// 「最小值 / 来源表 / 默认值」这类最长约四个字，72px 放下不挤值列
pub const RESULT_SIDEBAR_KV_LABEL_WIDTH: f32 = 4.5;

/// 列浏览器页签底部元数据明细区高（7.75rem = 标题行 1.5 + 五行键值；超出内部滚动，
/// 不给清单挤占无限滚动空间）
pub const RESULT_BROWSER_DETAIL_HEIGHT: f32 = 7.75;
