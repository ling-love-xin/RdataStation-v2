//! 数据源导航设置项的写入处理（`apply_slot`，由 `SettingsService::apply_by_key` 分发）与读写访问器。
use gpui_kit::App;

use crate::model::{NavigatorFilters, ObjectMaskRule, SavedView, Settings};
use crate::persistence::persist;
use crate::registry::{SettingValue, Slot};
use crate::SettingsService;


/// 本域 slot 的标量写入处理；返回 `false` 表示拒绝（值形态不符 / 复合值 / 无法解析）。
pub(crate) fn apply_slot(
    slot: Slot,
    value: SettingValue,
    _window: Option<&mut gpui_kit::Window>,
    cx: &mut App,
) -> bool {
    match slot {
            Slot::NavigatorSourceShortCode => {
                let Some(on) = value.as_bool() else {
                    return false;
                };
                SettingsService::set_source_short_code(on, cx);
            }
            Slot::NavigatorShowTags => {
                let Some(on) = value.as_bool() else {
                    return false;
                };
                SettingsService::set_show_tags(on, cx);
            }
            Slot::NavigatorShowScope => {
                let Some(on) = value.as_bool() else {
                    return false;
                };
                SettingsService::set_show_scope(on, cx);
            }
            Slot::NavigatorShowNameTypeShort => {
                let Some(on) = value.as_bool() else {
                    return false;
                };
                SettingsService::set_show_name_type_short(on, cx);
            }
            Slot::NavigatorRowEstimate => {
                let Some(on) = value.as_bool() else {
                    return false;
                };
                SettingsService::set_row_estimate(on, cx);
            }
            Slot::NavigatorShowInlineStats => {
                let Some(on) = value.as_bool() else {
                    return false;
                };
                SettingsService::set_show_inline_stats(on, cx);
            }
            Slot::NavigatorShowInlineComment => {
                let Some(on) = value.as_bool() else {
                    return false;
                };
                SettingsService::set_show_inline_comment(on, cx);
            }
            Slot::NavigatorShowInlineSize => {
                let Some(on) = value.as_bool() else {
                    return false;
                };
                SettingsService::set_show_inline_size(on, cx);
            }
            Slot::NavigatorShowChips => {
                let Some(on) = value.as_bool() else {
                    return false;
                };
                SettingsService::set_show_chips(on, cx);
            }
            Slot::NavigatorShowSearch => {
                let Some(on) = value.as_bool() else {
                    return false;
                };
                SettingsService::set_show_search(on, cx);
            }
            Slot::NavigatorAutoConnectStartup => {
                let Some(on) = value.as_bool() else {
                    return false;
                };
                // 启动时开关只在下次启动生效，不需要即时刷新窗口。
                {
                    let settings = cx.global_mut::<Settings>();
                    settings.navigator.auto_connect_on_startup = on;
                }
                let settings = cx.global::<Settings>().clone();
                persist(&settings);
            }
            Slot::NavigatorFilters
            | Slot::NavigatorSavedViews
            | Slot::NavigatorVisibleDatabases
            | Slot::NavigatorGlobalMask => return false,
        _ => return false,
    }
    true
}

impl SettingsService {
    /// 来源标识是否用短码（`P` / `G` / `GP`）。
    pub fn source_short_code(cx: &App) -> bool {
        cx.global::<Settings>().navigator.source_short_code
    }
    /// 设置并持久化来源标识形式（短码 ⇄ 文字）。
    pub fn set_source_short_code(short_code: bool, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.navigator.source_short_code = short_code;
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
        // 导航面板按需重渲染（设置与面板是不同实体，不通知不会即时刷新）。
        cx.refresh_windows();
    }
    /// 连接名称前是否以 `[简称]` 显示类型简称。
    pub fn show_name_type_short(cx: &App) -> bool {
        cx.global::<Settings>().navigator.show_name_type_short
    }
    /// 设置并持久化「连接名称显示类型简称」开关。
    pub fn set_show_name_type_short(show: bool, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.navigator.show_name_type_short = show;
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
        cx.refresh_windows();
    }
    /// 数据库类型 → 简称（`type_id` → 简称）。未登记类型回退类型名首 2 字母
    /// （大写）；与导航徽标缩写（`nav_type_badge`）同源。
    pub fn type_short(type_id: &str, cx: &App) -> String {
        let map = &cx.global::<Settings>().navigator.type_short_map;
        map.get(type_id).cloned().unwrap_or_else(|| {
            let upper = type_id.to_uppercase();
            let short: String = upper.chars().take(2).collect();
            if short.is_empty() { "DB".into() } else { short }
        })
    }
    /// 整体替换类型简称映射表（「类型简称映射…」弹窗保存时调用），并持久化 + 刷新。
    pub fn set_type_short_map(map: std::collections::HashMap<String, String>, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.navigator.type_short_map = map;
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
        cx.refresh_windows();
    }
    /// 连接行是否显示标签 chip。
    pub fn show_tags(cx: &App) -> bool {
        cx.global::<Settings>().navigator.show_tags
    }
    /// 设置并持久化「显示标签」开关。
    pub fn set_show_tags(show: bool, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.navigator.show_tags = show;
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
        cx.refresh_windows();
    }
    /// 连接行是否显示归属域列。
    pub fn show_scope(cx: &App) -> bool {
        cx.global::<Settings>().navigator.show_scope
    }
    /// 设置并持久化「显示归属域」开关。
    pub fn set_show_scope(show: bool, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.navigator.show_scope = show;
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
        cx.refresh_windows();
    }
    /// 面板是否显示来源筛选 chips 行。
    pub fn show_chips(cx: &App) -> bool {
        cx.global::<Settings>().navigator.show_chips
    }
    /// 设置并持久化「显示来源筛选 chips 行」开关。
    pub fn set_show_chips(show: bool, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.navigator.show_chips = show;
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
        cx.refresh_windows();
    }
    /// 面板是否显示搜索行。
    pub fn show_search(cx: &App) -> bool {
        cx.global::<Settings>().navigator.show_search
    }
    /// 启动时是否自动连接上次打开的连接。
    pub fn auto_connect_on_startup(cx: &App) -> bool {
        cx.global::<Settings>().navigator.auto_connect_on_startup
    }
    /// 设置并持久化「显示搜索行」开关。
    pub fn set_show_search(show: bool, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.navigator.show_search = show;
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
        cx.refresh_windows();
    }
    /// 表/视图行右侧统计徽标是否整体显示（需求④）：分区 / 继承 / ≈行估计 / 大小 /
    /// 注释副标 的总开关。开启时仍受各驱动档案门控（PG 支持、SQLite/DuckDB 无）。
    pub fn show_inline_stats(cx: &App) -> bool {
        cx.global::<Settings>().navigator.show_inline_stats
    }
    /// 设置并持久化「表行统计徽标」总开关。
    pub fn set_show_inline_stats(show: bool, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.navigator.show_inline_stats = show;
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
        cx.refresh_windows();
    }
    /// 表行「注释副标」子开关（需求②）：总开关开时仍受此项门控。
    pub fn show_inline_comment(cx: &App) -> bool {
        cx.global::<Settings>().navigator.show_inline_comment
    }
    /// 设置并持久化「注释副标」子开关。
    pub fn set_show_inline_comment(show: bool, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.navigator.show_inline_comment = show;
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
        cx.refresh_windows();
    }
    /// 表行「大小徽标」子开关（需求②）。
    pub fn show_inline_size(cx: &App) -> bool {
        cx.global::<Settings>().navigator.show_inline_size
    }
    /// 设置并持久化「大小徽标」子开关。
    pub fn set_show_inline_size(show: bool, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.navigator.show_inline_size = show;
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
        cx.refresh_windows();
    }
    /// 导航 facet 筛选（类型 / 驱动 / 标签 / 归属域）。
    pub fn nav_filters(cx: &App) -> NavigatorFilters {
        cx.global::<Settings>().navigator.filters.clone()
    }
    /// 设置并持久化导航 facet 筛选。
    pub fn set_nav_filters(filters: NavigatorFilters, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.navigator.filters = filters;
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
        cx.refresh_windows();
    }
    /// 全部已保存视图（按视图名排序；`name` 来自 map 键）。
    pub fn saved_views(cx: &App) -> Vec<SavedView> {
        cx.global::<Settings>()
            .navigator
            .saved_views
            .iter()
            .map(|(name, filters)| SavedView {
                name: name.clone(),
                filters: filters.clone(),
            })
            .collect()
    }
    /// 保存一个命名视图并持久化。
    ///
    /// 同名**拦截**（不覆盖、不自动改名）：返回 `Err("已存在同名视图…")`，由调用方提示改名。
    pub fn save_saved_view(name: String, filters: NavigatorFilters, cx: &mut App) -> Result<(), String> {
        {
            let settings = cx.global_mut::<Settings>();
            if settings.navigator.saved_views.contains_key(&name) {
                return Err(format!("已存在同名视图「{name}」，请换个名称。"));
            }
            settings.navigator.saved_views.insert(name, filters);
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
        cx.refresh_windows();
        Ok(())
    }
    /// 删除一个命名视图并持久化。名称不存在时返回 `Err`。
    pub fn delete_saved_view(name: &str, cx: &mut App) -> Result<(), String> {
        {
            let settings = cx.global_mut::<Settings>();
            if settings.navigator.saved_views.remove(name).is_none() {
                return Err(format!("视图「{name}」不存在。"));
            }
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
        cx.refresh_windows();
        Ok(())
    }
    /// 某连接的可见库白名单（S3-①）。键不存在 = 默认模式（`None`）；
    /// 键存在 = 白名单模式（`Some(集合)`，可空 = 全隐藏）。
    pub fn visible_databases(conn_id: &str, cx: &App) -> Option<Vec<String>> {
        cx.global::<Settings>()
            .navigator
            .visible_databases
            .get(conn_id)
            .cloned()
    }
    /// S3-④ 行估计徽标开关（默认开）：关闭后导航不排估计任务、行尾不显示「≈ N」。
    pub fn row_estimate(cx: &App) -> bool {
        cx.global::<Settings>().navigator.row_estimate
    }
    /// 设置并持久化「行估计徽标」开关。
    pub fn set_row_estimate(on: bool, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.navigator.row_estimate = on;
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
        cx.refresh_windows();
    }
    /// B-1 某连接的对象名 mask（键不存在 = 该连接不过滤，返回默认空规则）。
    pub fn object_mask(conn_id: &str, cx: &App) -> ObjectMaskRule {
        cx.global::<Settings>()
            .navigator
            .object_masks
            .get(conn_id)
            .cloned()
            .unwrap_or_default()
    }
    /// B-1 面板全局对象名 mask（空规则 = 不启用）。
    pub fn global_mask(cx: &App) -> ObjectMaskRule {
        cx.global::<Settings>().navigator.global_mask.clone()
    }
    /// B-1 保存某连接的对象名 mask 并持久化。
    ///
    /// `None` = 删除该连接的规则记录（回到不过滤）。
    pub fn set_object_mask(conn_id: &str, rule: Option<ObjectMaskRule>, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            match rule {
                Some(r) => {
                    settings
                        .navigator
                        .object_masks
                        .insert(conn_id.to_string(), r);
                }
                None => {
                    settings.navigator.object_masks.remove(conn_id);
                }
            }
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
        cx.refresh_windows();
    }
    /// B-1 保存面板全局对象名 mask 并持久化。
    pub fn set_global_mask(rule: ObjectMaskRule, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.navigator.global_mask = rule;
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
        cx.refresh_windows();
    }
    /// 保存某连接的可见库白名单并持久化（S3-①）。
    ///
    /// `None` = 删除该连接的白名单记录（回到默认模式）；
    /// `Some(空)` = 显式空白名单（弹框里全取消 = 默认隐藏系统库，但不删记录）。
    pub fn set_visible_databases(
        conn_id: &str,
        visible: Option<Vec<String>>,
        cx: &mut App,
    ) {
        {
            let settings = cx.global_mut::<Settings>();
            match visible {
                Some(v) => {
                    settings
                        .navigator
                        .visible_databases
                        .insert(conn_id.to_string(), v);
                }
                None => {
                    settings.navigator.visible_databases.remove(conn_id);
                }
            }
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
        cx.refresh_windows();
    }
}
