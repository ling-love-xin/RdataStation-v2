//! 结果集面板设置项的写入处理（`apply_slot`，由 `SettingsService::apply_by_key` 分发）与读写访问器。
use gpui_kit::App;

use crate::model::{FactStoragePref, Settings};
use crate::persistence::persist;
use crate::registry::{SettingValue, Slot};

/// 本域 slot 的标量写入处理；返回 `false` 表示拒绝（值形态不符 / 复合值 / 无法解析）。
pub(crate) fn apply_slot(
    slot: Slot,
    value: SettingValue,
    _window: Option<&mut gpui_kit::Window>,
    cx: &mut App,
) -> bool {
    match slot {
        Slot::ResultPanelFactStorage => {
            let Some(text) = value.as_text() else {
                return false;
            };
            // 落盘值只认登记表枚举三档；未知值拒绝（设置文件被手改坏时不让写入）。
            let Some(pref) = FactStoragePref::parse(text) else {
                return false;
            };
            {
                let settings = cx.global_mut::<Settings>();
                settings.result_panel.fact_storage = pref;
            }
            persist(&cx.global::<Settings>().clone());
            // NextUse：下次触发物化路径读取；渲染与文本导出不读本偏好，无需通知重绘。
        }
        Slot::ResultPanelColumnHeaderExtra => {
            let Some(text) = value.as_text() else {
                return false;
            };
            // 只认登记表三档；未知值拒绝（设置文件被手改坏时不让写入）。
            let Some(extra) = workbench_shell::result_style::ColumnHeaderExtra::parse(text) else {
                return false;
            };
            {
                let settings = cx.global_mut::<Settings>();
                settings.result_panel.column_header_extra = extra;
            }
            persist(&cx.global::<Settings>().clone());
            // Immediate：宿主在偏好变更钩子里推 `set_column_header_extra`，面板重绘列头。
        }
        Slot::ResultPanelShowTypeBadge => {
            let Some(on) = value.as_bool() else {
                return false;
            };
            {
                let settings = cx.global_mut::<Settings>();
                settings.result_panel.show_type_badge = on;
            }
            persist(&cx.global::<Settings>().clone());
        }
        Slot::ResultPanelLoopSelection => {
            let Some(on) = value.as_bool() else {
                return false;
            };
            {
                let settings = cx.global_mut::<Settings>();
                settings.result_panel.loop_selection = on;
            }
            persist(&cx.global::<Settings>().clone());
        }
        Slot::ResultPanelSelectOnModifierClick => {
            let Some(on) = value.as_bool() else {
                return false;
            };
            {
                let settings = cx.global_mut::<Settings>();
                settings.result_panel.select_on_modifier_click = on;
            }
            persist(&cx.global::<Settings>().clone());
        }
        Slot::ResultPanelPersistColumnWidths => {
            let Some(on) = value.as_bool() else {
                return false;
            };
            {
                let settings = cx.global_mut::<Settings>();
                settings.result_panel.persist_column_widths = on;
            }
            persist(&cx.global::<Settings>().clone());
        }
        // 【选中统计】状态栏三项（DBeaver `resultset.show.selected.rows/columns/cells`）：
        // 三条独立偏好，Immediate——面板下次 sync / 设置页切换后立即重画状态行。
        Slot::ResultPanelSelectionStatRows => {
            let Some(on) = value.as_bool() else {
                return false;
            };
            {
                let settings = cx.global_mut::<Settings>();
                settings.result_panel.selection_stat_rows = on;
            }
            persist(&cx.global::<Settings>().clone());
        }
        Slot::ResultPanelSelectionStatColumns => {
            let Some(on) = value.as_bool() else {
                return false;
            };
            {
                let settings = cx.global_mut::<Settings>();
                settings.result_panel.selection_stat_columns = on;
            }
            persist(&cx.global::<Settings>().clone());
        }
        Slot::ResultPanelSelectionStatCells => {
            let Some(on) = value.as_bool() else {
                return false;
            };
            {
                let settings = cx.global_mut::<Settings>();
                settings.result_panel.selection_stat_cells = on;
            }
            persist(&cx.global::<Settings>().clone());
        }
        // 【筛选条显隐】DBeaver `resultset.show.filterPanel`（出厂开）：
        // 设置页切换后立即生效（面板下次 render 读自身态；见 §9.34 差异第 4 条）。
        Slot::ResultPanelShowFilterPanel => {
            let Some(on) = value.as_bool() else {
                return false;
            };
            {
                let settings = cx.global_mut::<Settings>();
                settings.result_panel.show_filter_panel = on;
            }
            persist(&cx.global::<Settings>().clone());
        }
        Slot::ResultPanelSegmentRows => {
            // 【R6】一段行数：数值项，`< 1` 直接拒绝（对齐
            // `ResultSetPreferences.MIN_SEGMENT_SIZE = 1`；面板侧输入框已按同口径过滤，
            // 设置页手填一个 0 / 负数同样应当报错而不是静默变 1）。
            let Some(n) = value.as_number() else {
                return false;
            };
            if n < 1.0 || n > 100_000.0 {
                return false;
            }
            {
                let settings = cx.global_mut::<Settings>();
                settings.result_panel.segment_rows = n as i64;
            }
            persist(&cx.global::<Settings>().clone());
        }
        Slot::ResultPanelSidebarWidth => {
            // 【R5】右栏宽度：数值项，超出 192~640 直接拒绝（不静默夹取 ——
            // 手柄拖拽路径自己夹，设置页手填则按登记范围报错）。
            let Some(n) = value.as_number() else {
                return false;
            };
            if !(192.0..=640.0).contains(&n) {
                return false;
            }
            {
                let settings = cx.global_mut::<Settings>();
                settings.result_panel.sidebar_width = n as i64;
            }
            persist(&cx.global::<Settings>().clone());
        }
        Slot::ResultPanelNumberFormat => {
            let Some(text) = value.as_text() else {
                return false;
            };
            {
                let settings = cx.global_mut::<Settings>();
                settings.result_panel.number_format =
                    workbench_shell::result_style::NumberFormatPref::parse(text);
            }
            persist(&cx.global::<Settings>().clone());
        }
        Slot::ResultPanelNullPlaceholder => {
            let Some(text) = value.as_text() else {
                return false;
            };
            {
                let settings = cx.global_mut::<Settings>();
                settings.result_panel.null_placeholder =
                    workbench_shell::result_style::NullPlaceholderPref::parse(text);
            }
            persist(&cx.global::<Settings>().clone());
        }
        Slot::ResultPanelBoolDisplay => {
            let Some(text) = value.as_text() else {
                return false;
            };
            {
                let settings = cx.global_mut::<Settings>();
                settings.result_panel.bool_display =
                    workbench_shell::result_style::BoolDisplayPref::parse(text);
            }
            persist(&cx.global::<Settings>().clone());
        }
        Slot::ResultPanelAlignNumeric => {
            let Some(text) = value.as_text() else {
                return false;
            };
            {
                let settings = cx.global_mut::<Settings>();
                settings.result_panel.align_numeric =
                    workbench_shell::result_style::AlignPref::parse(text);
            }
            persist(&cx.global::<Settings>().clone());
        }
        Slot::ResultPanelAlignText => {
            let Some(text) = value.as_text() else {
                return false;
            };
            {
                let settings = cx.global_mut::<Settings>();
                settings.result_panel.align_text =
                    workbench_shell::result_style::AlignPref::parse(text);
            }
            persist(&cx.global::<Settings>().clone());
        }
        Slot::ResultPanelAlignTemporal => {
            let Some(text) = value.as_text() else {
                return false;
            };
            {
                let settings = cx.global_mut::<Settings>();
                settings.result_panel.align_temporal =
                    workbench_shell::result_style::AlignPref::parse(text);
            }
            persist(&cx.global::<Settings>().clone());
        }
        Slot::ResultPanelAlignBool => {
            let Some(text) = value.as_text() else {
                return false;
            };
            {
                let settings = cx.global_mut::<Settings>();
                settings.result_panel.align_bool =
                    workbench_shell::result_style::AlignPref::parse(text);
            }
            persist(&cx.global::<Settings>().clone());
        }
        Slot::ResultPanelAlignBlob => {
            let Some(text) = value.as_text() else {
                return false;
            };
            {
                let settings = cx.global_mut::<Settings>();
                settings.result_panel.align_blob =
                    workbench_shell::result_style::AlignPref::parse(text);
            }
            persist(&cx.global::<Settings>().clone());
        }
        Slot::ResultPanelColorNumeric => {
            let Some(text) = value.as_text() else {
                return false;
            };
            {
                let settings = cx.global_mut::<Settings>();
                settings.result_panel.color_numeric =
                    workbench_shell::result_style::ColorRolePref::parse(text);
            }
            persist(&cx.global::<Settings>().clone());
        }
        Slot::ResultPanelColorText => {
            let Some(text) = value.as_text() else {
                return false;
            };
            {
                let settings = cx.global_mut::<Settings>();
                settings.result_panel.color_text =
                    workbench_shell::result_style::ColorRolePref::parse(text);
            }
            persist(&cx.global::<Settings>().clone());
        }
        Slot::ResultPanelColorTemporal => {
            let Some(text) = value.as_text() else {
                return false;
            };
            {
                let settings = cx.global_mut::<Settings>();
                settings.result_panel.color_temporal =
                    workbench_shell::result_style::ColorRolePref::parse(text);
            }
            persist(&cx.global::<Settings>().clone());
        }
        Slot::ResultPanelColorBool => {
            let Some(text) = value.as_text() else {
                return false;
            };
            {
                let settings = cx.global_mut::<Settings>();
                settings.result_panel.color_bool =
                    workbench_shell::result_style::ColorRolePref::parse(text);
            }
            persist(&cx.global::<Settings>().clone());
        }
        Slot::ResultPanelColorBlob => {
            let Some(text) = value.as_text() else {
                return false;
            };
            {
                let settings = cx.global_mut::<Settings>();
                settings.result_panel.color_blob =
                    workbench_shell::result_style::ColorRolePref::parse(text);
            }
            persist(&cx.global::<Settings>().clone());
        }
        Slot::ResultPanelBgNumeric => {
            let Some(text) = value.as_text() else {
                return false;
            };
            {
                let settings = cx.global_mut::<Settings>();
                settings.result_panel.bg_numeric =
                    workbench_shell::result_style::BgColorPref::parse(text);
            }
            persist(&cx.global::<Settings>().clone());
        }
        Slot::ResultPanelBgText => {
            let Some(text) = value.as_text() else {
                return false;
            };
            {
                let settings = cx.global_mut::<Settings>();
                settings.result_panel.bg_text =
                    workbench_shell::result_style::BgColorPref::parse(text);
            }
            persist(&cx.global::<Settings>().clone());
        }
        Slot::ResultPanelBgTemporal => {
            let Some(text) = value.as_text() else {
                return false;
            };
            {
                let settings = cx.global_mut::<Settings>();
                settings.result_panel.bg_temporal =
                    workbench_shell::result_style::BgColorPref::parse(text);
            }
            persist(&cx.global::<Settings>().clone());
        }
        Slot::ResultPanelBgBool => {
            let Some(text) = value.as_text() else {
                return false;
            };
            {
                let settings = cx.global_mut::<Settings>();
                settings.result_panel.bg_bool =
                    workbench_shell::result_style::BgColorPref::parse(text);
            }
            persist(&cx.global::<Settings>().clone());
        }
        Slot::ResultPanelBgBlob => {
            let Some(text) = value.as_text() else {
                return false;
            };
            {
                let settings = cx.global_mut::<Settings>();
                settings.result_panel.bg_blob =
                    workbench_shell::result_style::BgColorPref::parse(text);
            }
            persist(&cx.global::<Settings>().clone());
        }
        Slot::ResultPanelFontNumeric => {
            let Some(text) = value.as_text() else {
                return false;
            };
            {
                let settings = cx.global_mut::<Settings>();
                settings.result_panel.font_numeric =
                    workbench_shell::result_style::FontStylePref::parse(text);
            }
            persist(&cx.global::<Settings>().clone());
        }
        Slot::ResultPanelFontText => {
            let Some(text) = value.as_text() else {
                return false;
            };
            {
                let settings = cx.global_mut::<Settings>();
                settings.result_panel.font_text =
                    workbench_shell::result_style::FontStylePref::parse(text);
            }
            persist(&cx.global::<Settings>().clone());
        }
        Slot::ResultPanelFontTemporal => {
            let Some(text) = value.as_text() else {
                return false;
            };
            {
                let settings = cx.global_mut::<Settings>();
                settings.result_panel.font_temporal =
                    workbench_shell::result_style::FontStylePref::parse(text);
            }
            persist(&cx.global::<Settings>().clone());
        }
        Slot::ResultPanelFontBool => {
            let Some(text) = value.as_text() else {
                return false;
            };
            {
                let settings = cx.global_mut::<Settings>();
                settings.result_panel.font_bool =
                    workbench_shell::result_style::FontStylePref::parse(text);
            }
            persist(&cx.global::<Settings>().clone());
        }
        Slot::ResultPanelFontBlob => {
            let Some(text) = value.as_text() else {
                return false;
            };
            {
                let settings = cx.global_mut::<Settings>();
                settings.result_panel.font_blob =
                    workbench_shell::result_style::FontStylePref::parse(text);
            }
            persist(&cx.global::<Settings>().clone());
        }
        Slot::ResultPanelHiddenStatusCommands => {
            // 【工具条配置】自由文本：只做空白折叠 / 去重空项 / 小写，**不校验 id**——
            // id ↔ 命令的对应表在 `result_panel::ports::StatusCommand`，settings 层不依赖它
            // （依赖只向下）；未知 id 由面板侧在读的时候忽略。
            let Some(text) = value.as_text() else {
                return false;
            };
            let normalized = normalize_command_list(text);
            {
                let settings = cx.global_mut::<Settings>();
                settings.result_panel.hidden_status_commands = normalized;
            }
            persist(&cx.global::<Settings>().clone());
        }
        Slot::ResultPanelStatsItems => {
            // 【计算页持久化】同口径：只做空白折叠 / 去重空项 / 小写，不校验 id
            // （id ↔ 项对应表在 `result_panel::ports::StatsItem::id`，未知 id 面板侧丢弃）。
            // 空串保留：它是「九项全停用」这个合法状态，不能被当成"没写"。
            let Some(text) = value.as_text() else {
                return false;
            };
            let normalized = normalize_command_list(text);
            {
                let settings = cx.global_mut::<Settings>();
                settings.result_panel.stats_items = normalized;
            }
            persist(&cx.global::<Settings>().clone());
        }
        Slot::ResultPanelGroupSort => {
            // 【分组页持久化】只认 `none` / `value` / `countdesc` 三档；未知值拒绝（设置页手填
            // 坏值时让设置页自己报错，而不是悄悄把面板排序换成别的档）。口径与
            // `set_result_panel_group_sort` 一致。
            let Some(text) = value.as_text() else {
                return false;
            };
            let normalized = text.trim().to_ascii_lowercase();
            if !matches!(normalized.as_str(), "none" | "value" | "countdesc") {
                return false;
            }
            {
                let settings = cx.global_mut::<Settings>();
                settings.result_panel.group_sort = normalized;
            }
            persist(&cx.global::<Settings>().clone());
        }
        Slot::ResultPanelGroupDuplicatesOnly => {
            // 【分组页持久化】「仅重复」开关（与 `DuplicatesOnlyAction` 的 check box 同口径）
            let Some(on) = value.as_bool() else {
                return false;
            };
            {
                let settings = cx.global_mut::<Settings>();
                settings.result_panel.group_duplicates_only = on;
            }
            persist(&cx.global::<Settings>().clone());
        }
        Slot::ResultPanelOpenTabs => {
            // 【右栏页签持久化】同口径：只做空白折叠 / 去重空项 / 小写，不校验 id
            // （id ↔ 页对应表在 `result_panel::ports::RightTab::id`，未知 id 面板侧丢弃）。
            // 空串归一为出厂五页全开：对齐 DBeaver `activateDefaultPanels:1396-1400`
            // （空集合会被补成默认面板），也避免「关空 → 下次启动页签条空白」的怪状态。
            let Some(text) = value.as_text() else {
                return false;
            };
            let mut normalized = normalize_command_list(text);
            if normalized.is_empty() {
                normalized = crate::model::ResultPanel::default().open_tabs;
            }
            {
                let settings = cx.global_mut::<Settings>();
                settings.result_panel.open_tabs = normalized;
            }
            persist(&cx.global::<Settings>().clone());
        }
        _ => return false,
    }
    true
}

impl crate::service::SettingsService {
    /// 【R5】写结果集右栏宽度（px）：手柄拖完由宿主落盘。
    ///
    /// 夹取到 `192..=640`（与面板侧 `RESULT_SIDEBAR_RIGHT_MIN_WIDTH` /
    /// `MAX_WIDTH` 同范围）；与现值相同则不写盘，避免拖拽结束时无谓 IO。
    pub fn set_result_panel_sidebar_width(px: f32, cx: &mut App) {
        let px = (px.round() as i64).clamp(192, 640);
        if cx.global::<Settings>().result_panel.sidebar_width == px {
            return;
        }
        {
            let settings = cx.global_mut::<Settings>();
            settings.result_panel.sidebar_width = px;
        }
        persist(&cx.global::<Settings>().clone());
        cx.refresh_windows();
    }

    /// 【R6 段大小】写「取下一段」的行数（状态行段框提交后由宿主落盘）。
    ///
    /// 与 `set_result_panel_sidebar_width` 同一形态：夹到 `1..=100000`（下限对齐
    /// `ResultSetPreferences.MIN_SEGMENT_SIZE`，上界防手填天文数字把一次取段拖垮），
    /// 与现值相同不写盘。
    pub fn set_result_panel_segment_rows(rows: usize, cx: &mut App) {
        let rows = (rows as i64).clamp(1, 100_000);
        if cx.global::<Settings>().result_panel.segment_rows == rows {
            return;
        }
        {
            let settings = cx.global_mut::<Settings>();
            settings.result_panel.segment_rows = rows;
        }
        persist(&cx.global::<Settings>().clone());
    }

    /// 【工具条配置】写状态栏隐藏的命令 id 列表（面板末端 ▾ 菜单点一次就落一次盘）。
    ///
    /// 与现值相同则不写盘；面板自己已经重绘，这里不 `refresh_windows`。
    pub fn set_result_panel_hidden_status_commands(list: &str, cx: &mut App) {
        let normalized = normalize_command_list(list);
        if cx.global::<Settings>().result_panel.hidden_status_commands == normalized {
            return;
        }
        {
            let settings = cx.global_mut::<Settings>();
            settings.result_panel.hidden_status_commands = normalized;
        }
        persist(&cx.global::<Settings>().clone());
    }

    /// 【计算页持久化】写启用的统计项 id 列表（＋ / 行尾 × / Reset 生效后由宿主落盘）。
    ///
    /// 与现值相同则不写盘；面板自己已经重绘，这里不 `refresh_windows`。
    pub fn set_result_panel_stats_items(list: &str, cx: &mut App) {
        let normalized = normalize_command_list(list);
        if cx.global::<Settings>().result_panel.stats_items == normalized {
            return;
        }
        {
            let settings = cx.global_mut::<Settings>();
            settings.result_panel.stats_items = normalized;
        }
        persist(&cx.global::<Settings>().clone());
    }

    /// 【分组页持久化】写分组结果排序口径的 id（分组页排序钮点一次落一次盘）。
    ///
    /// 只认 `none` / `value` / `countdesc`（小写、去空白）；其它值按出厂 `value` 处理——
    /// 手改配置文件写了个不存在的档位时，面板不该跟着乱排序。
    pub fn set_result_panel_group_sort(id: &str, cx: &mut App) {
        let normalized = match id.trim().to_ascii_lowercase().as_str() {
            "none" => "none".to_string(),
            "countdesc" => "countdesc".to_string(),
            _ => "value".to_string(),
        };
        if cx.global::<Settings>().result_panel.group_sort == normalized {
            return;
        }
        {
            let settings = cx.global_mut::<Settings>();
            settings.result_panel.group_sort = normalized;
        }
        persist(&cx.global::<Settings>().clone());
    }

    /// 【分组页持久化】写「仅显示重复分组」开关（分组页「仅重复」钮点一次落一次盘）。
    pub fn set_result_panel_group_duplicates_only(on: bool, cx: &mut App) {
        if cx.global::<Settings>().result_panel.group_duplicates_only == on {
            return;
        }
        {
            let settings = cx.global_mut::<Settings>();
            settings.result_panel.group_duplicates_only = on;
        }
        persist(&cx.global::<Settings>().clone());
    }

    /// 【选中统计】一次写状态栏三项开关（右键菜单三项切换后由宿主落盘）。
    ///
    /// 合并成一次调用：改一个开关不该触发三次写盘。三项与现值全同则不写。
    pub fn set_result_panel_selection_stats(
        rows: bool,
        columns: bool,
        cells: bool,
        cx: &mut App,
    ) {
        let current = {
            let s = cx.global::<Settings>();
            (
                s.result_panel.selection_stat_rows,
                s.result_panel.selection_stat_columns,
                s.result_panel.selection_stat_cells,
            )
        };
        if current == (rows, columns, cells) {
            return;
        }
        {
            let settings = cx.global_mut::<Settings>();
            settings.result_panel.selection_stat_rows = rows;
            settings.result_panel.selection_stat_columns = columns;
            settings.result_panel.selection_stat_cells = cells;
        }
        persist(&cx.global::<Settings>().clone());
    }

    /// 【筛选条显隐】写「显示过滤器面板」偏好（右键 layout 段切换后由宿主落盘）。
    ///
    /// 与现值相同则不写盘。
    pub fn set_result_panel_show_filter_panel(on: bool, cx: &mut App) {
        if cx.global::<Settings>().result_panel.show_filter_panel == on {
            return;
        }
        {
            let settings = cx.global_mut::<Settings>();
            settings.result_panel.show_filter_panel = on;
        }
        persist(&cx.global::<Settings>().clone());
    }

    /// 【右栏页签持久化】写启用的页 id 列表（页签条 × / 视图菜单勾选项生效后由宿主落盘）。
    ///
    /// 与现值相同则不写盘；空列表归一为出厂五页全开（见 `apply_slot` 同名分支）。
    pub fn set_result_panel_open_tabs(list: &str, cx: &mut App) {
        let mut normalized = normalize_command_list(list);
        if normalized.is_empty() {
            normalized = crate::model::ResultPanel::default().open_tabs;
        }
        if cx.global::<Settings>().result_panel.open_tabs == normalized {
            return;
        }
        {
            let settings = cx.global_mut::<Settings>();
            settings.result_panel.open_tabs = normalized;
        }
        persist(&cx.global::<Settings>().clone());
    }
}

/// 逗号分隔列表的规范化：去空白、丢空项、统一小写（`undo, Redo` → `undo,redo`）。
///
/// 不校验 id：id 与命令的对应表在面板 crate，settings 层不依赖它（依赖只向下）。
fn normalize_command_list(text: &str) -> String {
    text.split(',')
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>()
        .join(",")
}
