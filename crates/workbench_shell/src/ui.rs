//! 工作台 UI 尺寸约束（rem 基准 + 固定描边）。
//!
//! 目的：给视图层提供**唯一尺寸来源**，避免各组件各写 `px(N.)` 导致布局漂移
//! （栏宽、栏高、控件高、Quick Open 尺寸等）。
//!
//! 两套基准：
//! - **rem 倍率常量**（`f32`）：结构化尺寸（栏宽栏高、控件高、Quick Open 尺寸）。
//!   样式场景直接用 gpui 原生 `rems(ui::TITLE_BAR_HEIGHT)`；需要 `Pixels` 的 API
//!   （如 `set_dock_size`）用 `cx.theme().font_size * ui::LEFT_DOCK_WIDTH`。
//!   本模块只声明**设计倍率**（数值来源），换算交给 gpui / 主题字号，随字号 / DPI 缩放。
//! - **固定描边**（`Pixels`）：1px 边框、2px 激活条等不应随字号缩放的细线。
//!
//! 间距 / 内距等局部尺寸直接用 gpui 的 Tailwind 尺度方法（`gap_1` / `px_2` / `h_9`），
//! 其取值天然受限，无需登记常量。
//!
//! 边界：本模块只固定**尺寸与间距**；颜色、圆角、字号一律走主题 token
//! （`cx.theme().colors.*`、`theme.radius`），不在此定义。
//! 规范与数值来源见 `docs/architecture/ui/ui-design-spec.md`。
//!
//! VS Code 参照换算依据（2026-10-12，来源 `docs/architecture/ui/vscode-ui-design-system.md`）：
//! VS Code 以 13px 为 UI 根字号基准，本模块是 16px rem 基准——新增尺寸一律按
//! 「参照 → ×16/13≈1.23 → 圆整到 4px 网格」取值，不引入 13px 基准下的绝对 px：
//! 行高 22→28 或保留 24、按钮高 26→32（= CONTROL_HEIGHT_MD）、状态栏 22→28、
//! tab compact 22→28、徽标字号 11→13–14（text_sm）、树箭头 10→12（= ICON_SIZE_XS）、
//! 状态栏字号 12→14–15（text_sm）、tab 字号 13→15–16（text_sm/base）。
//! 结论：本模块常量与 VS Code 处于同一密度区间（换算落点大多命中现有档位）；
//! 11px 元信息字号与 22px 紧凑档**不可引入**（16px 基准下不可读 / 过小）。
//!
//! 使用约定：
//! 1. 新增**结构性尺寸**先在本模块登记倍率，再在视图中换算引用；
//! 2. 视图不得出现与布局结构相关的裸 `px(N.)`；局部间距用 Tailwind 尺度方法；
//! 3. 数值调整只改此处，全工作台同步生效。

use gpui_kit::*;

// ===== 五段布局（rem 基准；注释为 16px 字号下的实际值） =====

/// 标题栏高度（2.75rem ≈ 44px，2026-10-19 由 36px 调高）
pub const TITLE_BAR_HEIGHT: f32 = 2.75;
/// 活动栏宽度，左右一致（3rem = 48px）
pub const ACTIVITY_BAR_WIDTH: f32 = 3.0;

// ===== 活动栏（rem 基准） =====

/// 活动栏图标按钮尺寸（1.75rem ≈ 28px）
pub const ACTIVITY_ICON_SIZE: f32 = 1.75;
/// 活动栏单项宽（2.75rem ≈ 44px）
pub const ACTIVITY_ITEM_WIDTH: f32 = 2.75;
/// 活动栏单项高（2.5rem = 40px）
pub const ACTIVITY_ITEM_HEIGHT: f32 = 2.5;

// ===== Dock 边栏（rem 基准） =====

/// 左侧边栏起步宽度（18rem = 288px，2026-10-19 由 240px 调大）
pub const LEFT_DOCK_WIDTH: f32 = 18.0;
/// 右侧边栏起步宽度（20rem = 320px，2026-10-19 由 280px 调大）
pub const RIGHT_DOCK_WIDTH: f32 = 20.0;
/// 底部结果 dock 起步高度（12rem = 192px，约窗口 1/3；Dock 化阶段 3 用户确认，可拖拽）
pub const RESULT_DOCK_HEIGHT: f32 = 12.0;

// ===== 标题栏元素（rem 基准） =====

/// 软件图标尺寸（1.25rem = 20px）
pub const TITLE_LOGO_SIZE: f32 = 1.25;
/// 标题栏左侧内边距（0.625rem = 10px）
pub const TITLE_BAR_PADDING_X: f32 = 0.625;
/// 挖空项目槽高度（1.75rem ≈ 28px，随标题栏调高）
pub const TITLE_SLOT_HEIGHT: f32 = 1.75;
/// 挖空项目槽水平内边距（0.875rem ≈ 14px）
pub const TITLE_SLOT_PADDING_X: f32 = 0.875;
/// Quick Open 入口宽度（20rem = 320px）
pub const QUICK_OPEN_ENTRY_WIDTH: f32 = 20.0;
/// Quick Open 入口高度（1.75rem ≈ 28px，随标题栏调高）
pub const QUICK_OPEN_ENTRY_HEIGHT: f32 = 1.75;
/// Quick Open 入口水平内边距（0.625rem = 10px）
pub const QUICK_OPEN_ENTRY_PADDING_X: f32 = 0.625;
/// Quick Open 弹层宽度（35rem = 560px）
pub const QUICK_OPEN_PANEL_WIDTH: f32 = 35.0;
/// Quick Open 弹层距窗口顶部距离（2.625rem ≈ 42px）
pub const QUICK_OPEN_PANEL_TOP: f32 = 2.625;
/// Quick Open 弹层最大高度（26.25rem ≈ 420px）
pub const QUICK_OPEN_PANEL_MAX_HEIGHT: f32 = 26.25;
/// Quick Open 结果区最大高度（21.25rem ≈ 340px）
pub const QUICK_OPEN_LIST_MAX_HEIGHT: f32 = 21.25;
/// Quick Open 分组头行高（1.125rem = 18px）
pub const QUICK_OPEN_GROUP_HEADER_HEIGHT: f32 = 1.125;
/// Quick Open 底部提示行高（1.5rem = 24px）
pub const QUICK_OPEN_HINT_HEIGHT: f32 = 1.5;
/// Quick Open 单组渲染上限（条；超出显示「还有 N 条」）。
pub const QUICK_OPEN_MAX_ROWS_PER_GROUP: usize = 8;
/// Quick Open 总渲染上限（条）。
pub const QUICK_OPEN_MAX_ROWS: usize = 50;
/// Quick Open 内容档（`#`）的行高：两行式（标题行 + snippet 行；2.75rem = 44px）。
///
/// 高度按档切而不是按行：`List` 只量一个样本行并要求同行同高。
pub const QUICK_OPEN_ROW_HEIGHT_FULLTEXT: f32 = 2.75;
/// Quick Open 异步元数据搜索的防抖窗口（毫秒）。
pub const QUICK_OPEN_SEARCH_DEBOUNCE_MS: u64 = 150;

// ===== 列表与树（rem 基准） =====

/// 列表 / 树单行高度（1.5rem = 24px）
pub const ROW_HEIGHT: f32 = 1.5;
/// **紧凑行高**（1.375rem = 22px）——比 `ROW_HEIGHT` 矮一档的次级行。
///
/// 用于「信息行 + 行内动作」这一类行：分组头、外部引用行、回收站头 / 行（草稿箱底部
/// 固定区，不在虚拟列表内）。数值与导航对象树行（`NAV_ROW_TREE`）相同，但**语义独立**：
/// 改一处不动另一处。
pub const ROW_HEIGHT_COMPACT: f32 = 1.375;
/// 树层级缩进步长（0.875rem ≈ 14px）
pub const TREE_INDENT: f32 = 0.875;
/// 树基础左内边距（0.5rem = 8px）
pub const TREE_BASE_PADDING: f32 = 0.5;
/// 导航树类别文件夹的**取数**每批条数（非 rem 尺寸，是条目计数）。
///
/// 用法已收窄（2026-09-20）：它只是 `nav_jobs::PAGE_SIZE`（每次拉多少条），
/// **不再**是「屏上允许出现多少条」的渲染窗口——那个半套（`page_limit`）已随虚拟列表退场。
pub const NAV_FOLDER_PAGE_SIZE: usize = 200;
/// 导航搜索结果最多**渲染**多少条命中（超出只在结果区标题下提示“缩小搜索词”）。
///
/// 它不再是「一个 128px 小窗里塞多少条」（`NAV_SEARCH_SECTION_MAX` 已随结果区并入
/// 虚拟列表而退场）：命中行与其他行共用同一片滚动区，上限因此只是「DOM 元素数」的护栏。
pub const NAV_SEARCH_MAX_ROWS: usize = 100;
/// 连接行徽标尺寸（1.125rem ≈ 18px；颜色=状态 / 形状=类型）。
pub const NAV_BADGE_SIZE: f32 = 1.125;
/// 「连接中」徽标的呼吸周期（毫秒；原型设计 §2.3 的 `info` + 脉冲）。
///
/// 非尺寸常量，是**动画节奏**：1.5s 一次完整呼吸——慢到不抓眼睛，快到能看出「在动」。
/// 只有暂态（建连 / 预热中）才挂这个动画；其余状态不渲染动画元素（见 `render_connection_row`）。
pub const NAV_BADGE_PULSE_MS: u64 = 1500;
/// 导航状态的**落库防抖窗口**（毫秒）：展开 / 选中 / 搜索词在窗口内的连续变更攒成一次写。
///
/// 为何需要：展开与箭头键漫游都是高频动作，而落库是 UI 线程上的 SQLite 写——
/// 逐个写会把点击响应放大成一串磁盘操作（架构 §11 #25 ① 点名的「展开态写库无防抖」）。
/// 600ms 是「手指离开键盘就落」与「连按不写库」之间的取值。
pub const NAV_STATE_SAVE_DEBOUNCE_MS: u64 = 600;
/// 索引搜索的**输入防抖窗口**（毫秒）：快速连击（每个字符一次 `InputEvent::Change`）
/// 在窗口内被合并成一次真正的搜索任务。对齐竞品取值（navop 250ms / zqlz 150ms，
/// 见竞品分析 §5.3-A①）；250ms 是「打完一串词就出结果」与「不每键发任务」之间的取值。
pub const NAV_SEARCH_DEBOUNCE_MS: u64 = 250;
/// 连接行尾操作按钮尺寸（`+` 加标签 / `✎` 编辑；1.25rem = 20px）。
///
/// 为何不是 16px：可点区下限按 20px 取——宿主行高只有 26px（`NAV_ROW_CONNECTION`），
/// 再大就吃掉名称列；这两个按钮只在 hover / 选中时显，指针此时已在行内。
pub const NAV_ROW_ACTION_SIZE: f32 = 1.25;
/// 连接行归属域列宽（短码模式；2.4rem ≈ 38px）。
pub const NAV_SCOPE_COL_SHORT: f32 = 2.4;
/// 连接行归属域列宽（文字模式；3.4rem ≈ 54px）。
pub const NAV_SCOPE_COL_TEXT: f32 = 3.4;
/// 分组头行高（1.5rem = 24px）。
pub const NAV_ROW_GROUP: f32 = 1.5;
/// 连接行高（1.625rem = 26px；与旧的 `h(rems(1.625))` 同值）。
pub const NAV_ROW_CONNECTION: f32 = 1.625;
/// 对象树行高（1.375rem = 22px；与旧的 `h(rems(1.375))` 同值）。
pub const NAV_ROW_TREE: f32 = 1.375;
/// 树行下附加行高（「加载中…」/错误 / 标签 chip / 未连接提示；1.125rem = 18px）。
///
/// 虚拟列表不会反过来测量行高（`v_virtual_list` 按给定高度定义布局），所以这些附加行
/// 在渲染里也显式钉高：改这里就要同步改渲染（见 `nav_row_height`）。
pub const NAV_SUBLINE: f32 = 1.125;
/// 行内标签编辑器块高（3.5rem = 56px；内容超出时内部滚动）。
pub const NAV_EDITOR_TAG: f32 = 3.5;
/// 行内「复制为模板」编辑器块高（4.5rem = 72px）。
pub const NAV_EDITOR_COPY: f32 = 4.5;
/// 行内归组编辑器块高（5.5rem = 88px；分组多时内部滚动）。
pub const NAV_EDITOR_GROUP: f32 = 5.5;
/// 面板头部高度（2.25rem ≈ 36px）
pub const PANEL_HEADER_HEIGHT: f32 = 2.25;
/// 失败连接行的行内错误摘要最大宽（6.5rem ≈ 104px；超出省略，hover 卡看全量）。
pub const NAV_ERR_SUMMARY_W: f32 = 6.5;

// ===== 控件与图标（rem 基准） =====

/// 小控件高度（搜索框、工具栏控件；1.625rem ≈ 26px）
pub const CONTROL_HEIGHT_SM: f32 = 1.625;
/// 标准控件高度（输入框、按钮；2rem = 32px）
pub const CONTROL_HEIGHT_MD: f32 = 2.0;
/// 行内动作按钮最小命中区（1.25rem = 20px；可点区下限，ui-constraints §8.3）。
///
/// 导航行尾操作按钮（`NAV_ROW_ACTION_SIZE`）同值——本常量是**通用语义名**，
/// 草稿箱等面板的行内轻量动作（hover 才显的 `+` / `✎` / `✕` 类）用它；
/// 18px（1.125rem）小于命中区下限，曾导致草稿箱行尾按钮难点（2026-10-12 收口）。
pub const ROW_ACTION_SIZE: f32 = 1.25;
/// 内联图标尺寸（0.75rem = 12px）——行内小图标（展开指示 / 行尾操作 / 勾选）。
///
/// 图标阶梯的第三档（`docs/architecture/theme/ui-constraints.md` §2：12 / 14 / 16px）。
pub const ICON_SIZE_XS: f32 = 0.75;
/// 小图标尺寸（0.875rem ≈ 14px）
pub const ICON_SIZE_SM: f32 = 0.875;
/// 标准图标尺寸（1rem = 16px）
pub const ICON_SIZE_MD: f32 = 1.0;

// ===== 间距（rem 倍率） =====
//
// 局部间距 / 内距优先用 gpui 的 Tailwind 尺度方法（`gap_1` / `px_2` 等），
// 以下倍率供需要 `Pixels`（如传给 `set_dock_size` 类 API）或需集中调整的场合。

/// 紧凑间距（0.25rem = 4px）
pub const GAP_SM: f32 = 0.25;
/// 常规间距（0.5rem = 8px）
pub const GAP_MD: f32 = 0.5;
/// 宽松间距（0.75rem = 12px）
pub const GAP_LG: f32 = 0.75;
/// 面板内容内边距（0.5rem = 8px）
pub const PANEL_PADDING: f32 = 0.5;
/// 可见库管理弹框：候选列表最大高度（18.75rem = 300px，超过滚动）
pub const VIS_DIALOG_LIST_MAX_H: f32 = 18.75;
/// 可见库管理弹框：schema 行相对 catalog 行的缩进（1.25rem = 20px）
pub const VIS_DIALOG_SCHEMA_INDENT: f32 = 1.25;

// ===== 固定描边（不随字号缩放） =====

/// 1px 细线（边框 / 分隔线）
pub const HAIRLINE: Pixels = px(1.);
/// 激活项侧条宽度（2px，左栏在左、右栏在右）
pub const ACTIVITY_ACCENT_BAR: Pixels = px(2.);
/// 属性面板最小宽度（13.75rem = 220px，可拖拽下限）
pub const PROPERTY_PANEL_MIN_WIDTH: f32 = 13.75;
/// 属性面板最大宽度（47.5rem = 760px，可拖拽上限）
pub const PROPERTY_PANEL_MAX_WIDTH: f32 = 47.5;
/// 树 / 列表激活项侧条宽度
pub const TREE_ACTIVE_BAR: Pixels = px(2.);
/// 树 / 列表激活项侧条的上下内缩（3px；避免色条贴满整行高）
pub const TREE_ACTIVE_BAR_INSET: Pixels = px(3.);
/// 导航分组头左侧色条宽度（2px；与激活条同宽，语义独立）
pub const NAV_GROUP_BAR_WIDTH: Pixels = px(2.);

// ===== ER 建模视图（M4）专用尺寸（rem 基准；登记在此，视图只引用） =====
//
// 数值来源：`docs/architecture/database/database-er-model-view-design.md` §7 / §8。
// 布局度量由视图经 `er::view::metrics_for_theme` 把下列倍率 × 主题字号换算成逻辑像素。

/// ER 实体卡片宽（15rem = 240px）
pub const ER_CARD_WIDTH: f32 = 15.0;
/// ER 卡片表头（表名行）高（2rem = 32px）
pub const ER_CARD_HEADER_HEIGHT: f32 = 2.0;
/// ER 卡片列行高（1.5rem = 24px）
pub const ER_CARD_ROW_HEIGHT: f32 = 1.5;
/// ER 层间水平间距（5rem = 80px）
pub const ER_LAYER_H_GAP: f32 = 5.0;
/// ER 同层卡片垂直间距（1.5rem = 24px）
pub const ER_NODE_V_GAP: f32 = 1.5;
/// ER 画布内边距（1.5rem = 24px）
pub const ER_CANVAS_PADDING: f32 = 1.5;
/// ER 外键连线线宽（1.5px 细线，不随字号缩放）
pub const ER_EDGE_WIDTH: Pixels = px(1.5);

// ===== 草稿箱（M5）专用尺寸（rem 基准；登记在此，视图只引用） =====
/// 外部引用 / 回收站区域最大高度（7.5rem = 120px；超出内部滚动，保证草稿树始终有可用高度）
pub const SCRATCHPAD_GROUP_MAX_HEIGHT: f32 = 7.5;
/// 空态大图标尺寸（2.25rem = 36px）
pub const SCRATCHPAD_EMPTY_ICON_SIZE: f32 = 2.25;
/// 内联新建行的**模板 chip 块**高（1.625rem = 26px，含上下内距与行间隙）。
///
/// 与渲染成对：26px = `py_0p5`×2（4）+ `gap_0p5`（2）+ chip 行 `h_5`（20）。虚拟列表
/// **不实测回写**行高（`v_virtual_list` 按给定高度定义布局），所以增删这一块必须同时
/// 改 `ScratchpadView::scratchpad_row_height` 的附加块，否则后一行被压。
pub const SCRATCHPAD_ROW_CHIPS: f32 = 1.625;

// ===== 连接对话框（M3）专用尺寸（rem 基准；登记在此，视图只引用） =====
//
// 对话框是一个独立模态层，尺寸自成一套（比主界面行高略大、控件更高），
// 因此单独登记而不强行套用主界面常量（数值来源：connection-prototype-design §2/§3.1）。

/// 表单行标签列宽（4.25rem = 68px；再宽会显得“标签离输入框太远”）
pub const DIALOG_FORM_LABEL_WIDTH: f32 = 4.25;
/// 对话框行高（1.75rem ≈ 28px：暂存条目 / 分组标题 / 表单行）
pub const DIALOG_ROW_HEIGHT: f32 = 1.75;
/// 对话框「两列行」高度（32.5rem = 520px；左栏类型树 / 右列共用）。
///
/// 这是**设计基准**：两列同处一行、行高是唯一尺寸来源，切 Tab / 加草稿 / 目录变化都不改变
/// 对话框高度（设计 §2「布局恒定」）。取值按本工程侧栏自然高（搜索 + 暂存 7.5rem + 标签行 +
/// 类型树全部可见）取整；类型目录增长时类型树内部滚动，不再撑高对话框。
///
/// 实际行高还要再按窗口可用高度夹住（矮窗口 / 高缩放不把 footer 顶出屏幕）——
/// 见 `connection_dialog::helpers::dialog_row_height`（决策 #105）。
pub const DIALOG_BODY_HEIGHT: f32 = 32.5;
/// 新建 / 编辑数据源连接对话框宽度（61.25rem = 980px；与设置页弹层同档 `SETTINGS_PAGE_WIDTH`）
pub const DIALOG_EDIT_WIDTH: f32 = 61.25;
/// 配置管理对话框宽度（认证 / 网络 / 环境管理；35rem = 560px）
pub const DIALOG_MANAGER_WIDTH: f32 = 35.0;
/// Tab 内容区历史高度（20.5rem ≈ 328px；**保留作为设计参考值与其它面板的尺寸来源**）。
///
/// 注：右列自上而下是 Header → Tab 条 → **Tab 内容区（`flex_1 + min_h_0` 吃剩余高度）** → 结果行，
/// 内容区不再有自己的固定高（写死过 32.5rem，导致右列越出行高 168px 被裁，见决策 #101）；
/// 本常量在该对话框内不再引用（`insight` 的对比视图仍在用）。
pub const DIALOG_TAB_BODY_HEIGHT: f32 = 20.5;
/// 暂存列表固定高度（7.5rem = 120px；超出内部滚动）
pub const DIALOG_STAGING_HEIGHT: f32 = 7.5;
/// Header 驱动下拉定宽（11rem = 176px）
pub const DIALOG_DRIVER_WIDTH: f32 = 11.0;
/// Header 项目栏定宽（17rem = 272px）
pub const DIALOG_PROJECT_WIDTH: f32 = 17.0;
/// Header 类型徽标宽（1.75rem ≈ 28px，仅图标）
pub const DIALOG_BADGE_WIDTH: f32 = 1.75;
/// Header 类型徽标高（1.5rem = 24px）
pub const DIALOG_BADGE_HEIGHT: f32 = 1.5;
/// 作用域分段项高（1.25rem = 20px）——**已被 `TabBar::segmented` 组件尺寸档取代**（决策 #84）：
/// 保留常量供原型文档 / 契约测试对照，视图不再引用。
pub const DIALOG_SEGMENT_ITEM_HEIGHT: f32 = 1.25;
/// 暂存条目状态点直径（0.4375rem = 7px；无类型信息的旧草稿回退显示）
pub const DIALOG_STATUS_DOT_SIZE: f32 = 0.4375;
/// 短码徽标圆角（0.125rem = 2px）；横向内距用 Tailwind 尺度 `px_1()`（契约禁止 `.px(...)` 字面量）
pub const DIALOG_CHIP_RADIUS: f32 = 0.125;
/// 「数据(JSON)」输入框宽（12.5rem = 200px；网络页 JSON 数据行）
pub const DIALOG_JSON_INPUT_WIDTH: f32 = 12.5;
/// 网络 / 认证配置字段输入框宽（14rem = 224px；`managers.rs` 配置管理表单）
pub const DIALOG_NET_FIELD_WIDTH: f32 = 14.0;
/// 名称输入框宽（11.25rem = 180px；新建文件/网络页名称行）
pub const DIALOG_NAME_INPUT_WIDTH: f32 = 11.25;
/// 对话框侧栏宽（12.5rem = 200px；`conn-side-panel`，与「两列行」同底等高）
pub const DIALOG_SIDEBAR_WIDTH: f32 = 12.5;
/// 认证方法 / 引用配置标签列宽（5.75rem = 92px；`flex_shrink_0` 定宽让值列对齐）
pub const DIALOG_LABEL_COLUMN_WIDTH: f32 = 5.75;
/// 测试消息 / 结果占位高（1.125rem = 18px = `text_xs` 行高；无消息时空 div 占位防跳动）
pub const DIALOG_HINT_PLACEHOLDER_HEIGHT: f32 = 1.125;
/// 类型树小节头高（1.25rem = 20px；恒定高让 List 按同一高度测量，不随文字撑）
pub const DIALOG_SECTION_HEADER_HEIGHT: f32 = 1.25;

// ===== 日志对话框（日志系统的读路径）专用尺寸 =====
//
// 日志查看是**只读快照 + 手动刷新**（没有自动轮询），尺寸只需要三件事：
// 列表的滞高上限、以及时间 / 级别两列的定宽（定宽才能让消息列对齐）。
// 形态与口径：`docs/architecture/runtime/logging.md`。

/// 日志列表滚动高度上限（22.5rem = 360px；条目少时按内容高度，不撑空）
pub const DIALOG_LOG_LIST_MAX_HEIGHT: f32 = 22.5;
/// 时间列定宽（3.75rem = 60px：`HH:MM:SS`）
pub const DIALOG_LOG_TIME_WIDTH: f32 = 3.75;
/// 级别列定宽（3.25rem = 52px：`ERROR` 最宽，定宽让消息列对齐）
pub const DIALOG_LOG_LEVEL_WIDTH: f32 = 3.25;
/// 模块列定宽（9.75rem = 156px；超长截断，不抢消息列）
pub const DIALOG_LOG_TARGET_WIDTH: f32 = 9.75;

// ===== 状态栏 LSP 管理器浮层专用尺寸 =====

/// LSP 管理器浮层宽度（15rem = 240px；容纳状态行 + 在用面板数 + 启停按钮）
pub const STATUS_LSP_MANAGER_WIDTH: f32 = 15.0;
/// LSP 管理器浮层与状态栏指示的间距（4px；浮层锚在状态栏上方）
pub const STATUS_LSP_OFFSET: Pixels = px(4.);

// ===== 设置页（应用级）专用尺寸（rem 基准；登记在此，视图只引用） =====
//
// 设置页是居中模态弹层，两栏 = 分节导航 + 内容区；尺寸自成一套，不与主界面行高混用。
// 数值来源：`docs/architecture/settings/settings-prototype-design.md` §7。

/// 弹层宽度（61.25rem = 980px；与「新建数据源连接」同档）
pub const SETTINGS_PAGE_WIDTH: f32 = 61.25;
/// 弹层高度（35rem = 560px；**固定高**——切节 / 加行都不跳高）
pub const SETTINGS_PAGE_HEIGHT: f32 = 35.0;
/// 分节导航宽度（12.5rem = 200px；与连接对话框侧栏同宽）
pub const SETTINGS_NAV_WIDTH: f32 = 12.5;
/// 设置行标签列宽（15rem = 240px；再窄中长标签要换行）
pub const SETTINGS_LABEL_WIDTH: f32 = 15.0;
/// 设置行最小高度（2.5rem = 40px；带说明行时自动增高）
pub const SETTINGS_ROW_MIN_HEIGHT: f32 = 2.5;
/// 节标题行高度（1.75rem = 28px）
pub const SETTINGS_SECTION_HEAD_HEIGHT: f32 = 1.75;
/// 分组卡与控件圆角（0.375rem = 6px；弹层圆角走 `theme.radius`）
pub const SETTINGS_CARD_RADIUS: f32 = 0.375;
/// 分组卡内边距（0.75rem = 12px）
pub const SETTINGS_CARD_PADDING: f32 = 0.75;

// ===== 属性面板 / 连接详情卡专用尺寸（rem 基准；登记在此，视图只引用） =====
//
// 来源：`panels/property_panel.rs` / `panels/editor.rs` 既有布局值（2026-10-15 收口登记，
// 消除面板内裸 `rems(N.)` 倍率）。数值为既有实现原值，视觉零变化。

/// 数据页行号列宽（2rem = 32px；行号 `#N` 定宽让名称列对齐）
pub const PROPERTY_ROW_NUMBER_WIDTH: f32 = 2.0;
/// 洞察页状态点直径（0.625rem = 10px）
pub const PROPERTY_STATUS_DOT_SIZE: f32 = 0.625;
/// 指标卡最小宽（4.5rem = 72px；卡内两行文本的换行下限）
pub const PROPERTY_CARD_MIN_WIDTH: f32 = 4.5;
/// 关键信息 label 列宽（5.5rem = 88px；值与 label 各占半栏对齐）
pub const PROPERTY_KEY_LABEL_WIDTH: f32 = 5.5;
/// 子项类别左栏宽（10.5rem = 168px；类别 rail 定宽，右侧内容列吃剩余）
pub const PROPERTY_CAT_RAIL_WIDTH: f32 = 10.5;
/// 子项类别行高（1.75rem = 28px；行内含计数徽标，比普通行高一档）
pub const PROPERTY_CAT_ROW_HEIGHT: f32 = 1.75;
/// 连接详情卡「导航」列名宽（8.125rem = 130px；对象名定宽截断，右侧状态 / 徽标对齐）
pub const DATA_NAV_COL_NAME_WIDTH: f32 = 8.125;
