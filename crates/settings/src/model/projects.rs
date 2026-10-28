//! 项目设置 model（struct + 默认值 + 默认函数）。
use serde::{Deserialize, Serialize};

/// 项目：项目名册的展示偏好（排序方式）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Projects {
    /// 项目列表排序方式（`last_opened` / `name` / `created`）。
    #[serde(default = "default_project_sort")]
    pub sort_mode: String,
}

impl Default for Projects {
    fn default() -> Self {
        Self {
            sort_mode: default_project_sort(),
        }
    }
}

/// 数据源导航的 facet 筛选（UI 偏好；`None` = 未启用）。
///
/// 这是**复合值**：在登记表里作为一项（`navigator.filters`）登记，子键不单独登记。
/// 归属域为唯一常驻 chips，其余为「筛选 ▾」弹层；`db_type` 存 `drivers.type_id`，
/// `driver` 存驱动 id，`tag` 存标签文本，`source` 存 `project` / `global` / `shared`。
fn default_project_sort() -> String {
    "last_opened".to_string()
}
