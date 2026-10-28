//! 资产库设置 model（struct + 默认值 + 默认函数）。
use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resources {
    /// 每份存档保留的历史内容副本份数：`-1` = 全部保留（不裁剪），`0` = 只留版本元数据，
    /// `n ≥ 1` = 保留最近 n 份。默认 5。
    ///
    /// 用有符号数（而不是 `u32`）就是为了 `-1` 这个哨兵：它是 v1 与架构文档里已有的口径
    /// （`docs/architecture/analytics_resource/analytics-resource-architecture.md` §5.2）。
    #[serde(default = "default_keep_versions")]
    pub keep_versions: i64,
    /// 资产库列表的默认排序字段（`name` / `archived_at` / `updated_at` / `size` / `version`）。
    ///
    /// 面板里点一次排序就会写回这里（“默认排序” = 上次用的那个）——方向不入库，
    /// 由字段惯例给（名称升序，时间 / 大小 / 版本降序）。
    #[serde(default = "default_resource_sort")]
    pub default_sort: String,
    /// 分组折叠状态：**项目根路径 → 该项目里折叠的分组 key 列表**。
    ///
    /// 为什么要按项目分桶：分组 id 是每个项目自己生成的，平铺一份列表会在“打开另一个项目”
    /// 时被“清掉不认识的 id”那一步（面板会丢掉已删分组的折叠标记）抹掉别的项目的记录。
    /// 路径换名 / 移动后这一项会重新从空开始（视图状态，不值得为它做路径重写）。
    #[serde(default)]
    pub collapsed_groups: BTreeMap<String, Vec<String>>,
    /// 默认分组：**项目根路径 → 该项目的默认分组 id**（归档对话框预选它）。
    ///
    /// 为何按项目分桶：与 [`collapsed_groups`](Self::collapsed_groups) 同——分组 id 是
    /// 每项目自己生成的。为何**不跟随上次归档**（案 B）：默认分组回答的是“这个项目里我
    /// 习惯把产物放哪”（一份稳定设置），而不是“上次做了什么”；分组归属是内容属性，
    /// 猜错的代价与视图状态不对称（判据见 `analytics-resource-dev-plan.md` §4 的两案对比）。
    #[serde(default)]
    pub default_group: BTreeMap<String, String>,
}

impl Default for Resources {
    fn default() -> Self {
        Self {
            keep_versions: default_keep_versions(),
            default_sort: default_resource_sort(),
            collapsed_groups: BTreeMap::new(),
            default_group: BTreeMap::new(),
        }
    }
}

/// 日志：写文件 / 落库的级别门槛。
fn default_keep_versions() -> i64 {
    5
}
fn default_resource_sort() -> String {
    "name".to_string()
}
