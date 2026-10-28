//! 资产库设置项的写入处理（`apply_slot`，由 `SettingsService::apply_by_key` 分发）与读写访问器。
use gpui_kit::App;

use crate::model::Settings;
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
            Slot::ResourcesCollapsedGroups | Slot::ResourcesDefaultGroup => return false,
            Slot::ResourcesKeepVersions => {
                let Some(value) = value.as_number() else {
                    return false;
                };
                // 页面上只给预设档（含 `-1` = 全留），取整后写入。
                SettingsService::set_keep_versions(value.round() as i64, cx);
            }
            Slot::ResourcesDefaultSort => {
                let Some(text) = value.as_text() else {
                    return false;
                };
                SettingsService::set_default_resource_sort(text, cx);
            }
        _ => return false,
    }
    true
}

impl SettingsService {
    /// 每份存档保留的历史内容副本份数（`-1` = 全留，`0` = 只留元数据）。
    ///
    /// 返回的是**落盘口径的原始数**：转成领域类型的那一步在宿主侧做
    /// （设置层不依赖 M6，见 `analytics_resource::model::KeepVersions::from_setting`）。
    pub fn keep_versions(cx: &App) -> i64 {
        cx.global::<Settings>().resources.keep_versions
    }
    /// 设置并持久化历史内容保留份数。
    pub fn set_keep_versions(value: i64, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.resources.keep_versions = value;
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
    }
    /// 资产库列表的默认排序字段（落盘 key 形如 `name` / `archived_at`）。
    ///
    /// 返回原始文本：**认不认得出这个 key 是 M6 的事**（设置层不认识 `SortField`）。
    pub fn default_resource_sort(cx: &App) -> String {
        cx.global::<Settings>().resources.default_sort.clone()
    }
    /// 设置并持久化资产库列表的默认排序字段。
    pub fn set_default_resource_sort(key: &str, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.resources.default_sort = key.to_string();
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
    }
    /// 某个项目里折叠的分组 key 列表（空 = 没有折叠项）。
    ///
    /// `project_root` 用绝对路径的字符串形式（与 `Shared::project_root` 同一来源）；
    /// 哪些 key 还算数由资产库自己判（面板会丢掉已删分组的标记）。
    pub fn collapsed_groups(project_root: &str, cx: &App) -> Vec<String> {
        cx.global::<Settings>()
            .resources
            .collapsed_groups
            .get(project_root)
            .cloned()
            .unwrap_or_default()
    }
    /// 覆盖某个项目的折叠分组列表（空列表 = 删掉该项目的记录，不留空壳）。
    pub fn set_collapsed_groups(project_root: &str, keys: &[String], cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            if keys.is_empty() {
                settings.resources.collapsed_groups.remove(project_root);
            } else {
                settings
                    .resources
                    .collapsed_groups
                    .insert(project_root.to_string(), keys.to_vec());
            }
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
    }
    /// 某个项目的默认分组 id（`None` = 没设 / 设的分组已删，即未分组）。
    ///
    /// 与 [`collapsed_groups`](Self::collapsed_groups) 同一形态（按项目根分桶）。
    /// **不在这里校验分组是否还在**：那份名单在项目库里，只有资产库自己知道——
    /// 认不出的 id 由调用方按未分组处理（面板渲染标记 / 归档对话框都这么做）。
    pub fn default_group(project_root: &str, cx: &App) -> Option<String> {
        cx.global::<Settings>()
            .resources
            .default_group
            .get(project_root)
            .cloned()
    }
    /// 设 / 清某个项目的默认分组（`None` = 清掉该项目的记录，不留空壳）。
    pub fn set_default_group(project_root: &str, folder_id: Option<&str>, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            match folder_id {
                Some(id) => {
                    settings
                        .resources
                        .default_group
                        .insert(project_root.to_string(), id.to_string());
                }
                None => {
                    settings.resources.default_group.remove(project_root);
                }
            }
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
    }
}
