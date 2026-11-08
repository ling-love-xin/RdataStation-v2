//! 编辑器尺寸常量（**rem 倍率**；视图里用 `rems(...)` 换算）
//!
//! 为什么不复用 workbench 的 `ui.rs`：依赖方向是 `workbench → editor`，editor 不能反向依赖。
//! 因此编辑器自己的结构尺寸在这里登记（架构 §12 #6），跨模块共用的常量将来再上提到 shared。
//!
//! 规则同全仓：**结构尺寸进常量表 → 视图换算引用**；局部间距直接用 Tailwind 尺度方法
//! （`gap_1` / `px_2` …）；视图里不得出现裸 `px(N.)`。
//! 注意 `rems(x)` 的基准是主题字号（默认 16px），与 `gap_1`（=4px）不是一套单位。

use gpui_kit::{Pixels, px};

/// 固定描边（1px，不随字号缩放）
pub const HAIRLINE: Pixels = px(1.);

/// 编辑器工具栏高（2.25rem = 36px，与面板头同档）
pub const EDITOR_TOOLBAR_HEIGHT: f32 = 2.25;

/// 编辑器状态栏高（1.5rem = 24px，与列表行同档）
pub const EDITOR_STATUS_BAR_HEIGHT: f32 = 1.5;

/// 编辑区最小高度（防止被上方工具栏压成 0）
pub const EDITOR_MIN_HEIGHT: f32 = 6.0;

/// 状态点直径（0.375rem = 6px）：标签脏点与结果集失败点共用一档
pub const STATUS_DOT_SIZE: f32 = 0.375;

/// 结果区尺寸常量：**单一来源在 `result_panel::ui`**（依赖方向 `editor → result_panel`，
/// 正向引用成立；反向不行，所以不能反过来放）。
///
/// 2026-11-02 收敛：此前这 13 个 `RESULT_*` 在 `result_panel/src/ui.rs` 与这里各定义一份。
/// 其中 `RESULT_PANE_HEIGHT` 已实际漂移到两个值（这里 20.0 / 那边 18.0，而 result_panel
/// 那份全仓无人引用、是死值，真正生效的是 20.0）。现在统一 re-export，改一处即改全部。
///
/// 2026-11-08：`RESULT_HEADER_HEIGHT` 已从 `result_panel::ui` 删除（那边实测全仓零使用、
/// 32px 也与组件实际表头高 26px 不符，替换为 `RESULT_HEADER_COMMENT_ROW_HEIGHT`）。
/// 这里同步去掉该项 re-export——漏掉这一处会让 `rds-editor` 编译失败（E0432）。
pub use result_panel::ui::{
    RESULT_COLUMN_MIN_WIDTH, RESULT_COLUMN_WIDTH, RESULT_FILTER_WIDTH,
    RESULT_HEADER_TYPE_FONT_SIZE, RESULT_MAX_HEIGHT, RESULT_MIN_HEIGHT, RESULT_PANE_HEIGHT,
    RESULT_ROW_NUMBER_WIDTH, RESULT_STATUS_BAR_HEIGHT, RESULT_TABLE_SIZE, RESULT_TABS_HEIGHT,
    RESULT_TOOLTIP_MAX_WIDTH,
};

/// 结果区可拖拽的最小高度（4rem = 64px：再小就只剩工具栏了）
pub const RESULT_PANE_MIN_HEIGHT: f32 = 4.0;

/// 自定义分析 SQL 输入框高（9rem = 144px：约八行，写一条聚合 SQL 够用）
pub const ANALYSIS_SQL_HEIGHT: f32 = 9.0;

/// 【T2c-5】工具栏自定义面板宽（20rem = 320px）：三栏（图标｜可见｜移动）+ 组头行
/// 能放下“执行当前语句”这种最长的成员名；再窄就要挤掉可见性开关了。
pub const TOOLBAR_CUSTOMIZE_WIDTH: f32 = 20.0;

/// 【T2b-3】管理环境对话框宽（38rem = 608px）：环境配置表要放下
/// 名称 / 类型 / 命令 / 参数 / 版本 / 操作 六列，窄了会挤；
/// 编辑表单同一宽度，进出切换不跳。
pub const MANAGE_ENV_DIALOG_WIDTH: f32 = 38.0;

/// 【T2b-3】管理环境列表最大高（16rem = 256px：约十行；再多就滚动）
pub const MANAGE_ENV_LIST_MAX_HEIGHT: f32 = 16.0;

/// 【T2b-3】管理环境表各列宽（rem）：列宽进常量表，视图不写裸 px。
/// 名称列 flex_1 吃掉剩余宽度，其余固定。
pub const MANAGE_ENV_COL_KIND: f32 = 4.5;
pub const MANAGE_ENV_COL_COMMAND: f32 = 9.0;
pub const MANAGE_ENV_COL_ARGS: f32 = 6.5;
pub const MANAGE_ENV_COL_VERSION: f32 = 4.5;
pub const MANAGE_ENV_COL_ACTION: f32 = 7.0;

/// 【T2b-3】管理环境表单标签列宽（4rem = 64px：放「名称 / 命令 / 参数」这类两字标签）
pub const MANAGE_ENV_FORM_LABEL: f32 = 4.0;

/// 【T2c-5】工具栏自定义面板与触发按钮的间距（4px；结构尺寸进常量表，视图不写裸 px）
pub const TOOLBAR_CUSTOMIZE_OFFSET: Pixels = px(4.);

/// 【2d】分析笔记本：单元卡片的最大宽度（48rem = 768px：卡片居中于编辑区，
/// 比这更宽会被窗口边缘截掉；窄窗口里自动收窄）
pub const NOTEBOOK_CELL_MAX_WIDTH: f32 = 48.0;

/// 【2d】分析笔记本：单元卡片间距（0.5rem = 8px）
pub const NOTEBOOK_CELL_GAP: f32 = 0.5;

/// 【2d】分析笔记本：空态引导卡的最大宽度（36rem = 576px）
pub const NOTEBOOK_EMPTY_MAX_WIDTH: f32 = 36.0;

/// 【2d】分析笔记本：单元卡片顶部工具行（徽标 + 序号 + 动作）高（1.5rem = 24px）
pub const NOTEBOOK_CELL_HEADER_HEIGHT: f32 = 1.5;

/// 【遗留 #10】分析笔记本：单元源编辑器（内核实例）的定高（12rem = 192px，
/// 与输出区展开上限同档；多行超出由内核滚动——单元卡片不是整页，不做无限长）
pub const NOTEBOOK_CELL_EDITOR_HEIGHT: f32 = 12.0;

/// 【#14】分析笔记本：图片输出的最大宽度（36rem = 576px：与空态卡同档；
/// matplotlib 图按 Contain 缩放，防原图撑破单元卡片）
pub const NOTEBOOK_OUTPUT_IMAGE_MAX_WIDTH: f32 = 36.0;

/// 【遗留 #7】值查看器弹层：值显示区最大高（22rem = 352px：约 20 行等宽文本；
/// 再长就滚动——值查看器要的是「原样看全」，不是截断）
pub const VALUE_PREVIEW_MAX_HEIGHT: f32 = 22.0;

/// 【#54-C5】函数手册对话框宽度（38rem = 608px，与「管理环境」同档：搜索框 +
/// 两行结果（函数名+分类 / 中文描述）需要横向余量）
pub const FUNCTION_MANUAL_DIALOG_WIDTH: f32 = 38.0;

/// 【#54-C5】函数手册结果行高（3.0rem = 48px：两行式——函数名+分类标签 / 中文描述）
pub const FUNCTION_MANUAL_ROW_HEIGHT: f32 = 3.0;

/// 【#54-C4】签名帮助对话框宽度（32rem = 512px：签名 + 参数表 + 描述，无需函数手册
/// 那么宽——没有搜索框与结果列表）
pub const SIGNATURE_HELP_DIALOG_WIDTH: f32 = 32.0;

/// 【2026-10-27】工具栏右区「执行通道」pill 固定宽（7.5rem = 120px：图标 + 源库/加速/联邦
/// 短标签 + caret；固定大小占用区域，不随内容把布局推挤变形）
pub const CHANNEL_SLOT_WIDTH: f32 = 7.5;

/// 【2026-10-27】工具栏右区「连接名称」槽固定宽（9.0rem = 144px：运行点 + 短码·名称截断
/// 12 字符 + caret；见 `view/host.rs::render_conn_slots`）
pub const CONN_SLOT_WIDTH: f32 = 9.0;

/// 【2026-10-27】工具栏右区「schema@库」槽固定宽（10.0rem = 160px：schema@catalog 截断
/// 12 字符 + caret）
pub const SCHEMA_SLOT_WIDTH: f32 = 10.0;

/// 【2026-10-27】编码选择对话框宽度（20rem = 320px：单列短列表，5 项编码）
pub const ENCODING_DIALOG_WIDTH: f32 = 20.0;

/// 【2026-10-27】状态栏换行段固定宽（以 CRLF 为基准，2.2rem：LF/CRLF 两种标签中最宽者；
/// 循环切换时宽度不变，状态栏布局不抖动——见 `view/host.rs` 的 eol_ctrl）
pub const EOL_SLOT_WIDTH: f32 = 2.2;

/// 【2026-10-27】不可见字符配置对话框宽度（rems：双栏——左栏类别开关 + 右栏符号明细）
pub const INVISIBLE_DIALOG_WIDTH: f32 = 46.0;

/// 【2026-10-27】不可见字符面板右栏（符号明细）固定宽（rems）
pub const INVISIBLE_DETAIL_WIDTH: f32 = 26.0;

/// 【2026-10-27】缩进对话框宽度（rems：双语列表 + 右侧分组标签）
pub const INDENT_DIALOG_WIDTH: f32 = 34.0;

/// 【2026-10-27】tab_size 自定义输入对话框宽度（rems）
pub const TAB_SIZE_DIALOG_WIDTH: f32 = 22.0;

/// 【2026-10-27】编码选择行高（2.0rem = 32px：单行式——编码名 + 当前勾选）
pub const ENCODING_DIALOG_ROW_HEIGHT: f32 = 2.0;

/// 【2026-10-27】连接 / 库 / Schema 选择器对话框列表最大高度（rems：超长滚动）
pub const PICKER_LIST_MAX_HEIGHT: f32 = 18.0;

/// 【2026-10-27】Database / Schema 选择器对话框宽度（rems：两栏——左库右 Schema）
pub const SCHEMA_PICKER_DIALOG_WIDTH: f32 = 30.0;
