//! 对话框行模型：版本历史行、回收站条目、索引修复条目。
//!
//! 只有数据与分组语义，不含任何控件装配；各对话框仍以原路径导出。

use crate::model::PayloadMode;

/// 一行版本（**宿主快照**：对话框只渲染与选中，不做计算、不查文件系统）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionRow {
    /// 版本号（列表按它降序：新的在上）。
    pub version: i32,
    /// 是否当前版本（本体上的那一版）。当前版本没有"还原 / 取回该版本"动作
    /// ——它就是本体，取回走主界面的「取回（检出）…」。
    pub is_current: bool,
    /// 版本时间（绝对时间：版本历史是查凭证的地方，相对时间不够用）。
    pub time_label: String,
    /// 大小（可能为空：缺 `file_size` 的旧快照）。
    pub size_label: String,
    /// 内容指纹前 12 位（可能为空：无指纹的旧行）。
    pub hash_short: String,
    /// 内容副本在不在（当前版本 = 本体在位；历史行 = `.RSmeta` 下有副本）。
    pub has_copy: bool,
    /// 与**上一版**（版本号 -1）的差异：`较 v1 +1.2 KB · 指纹已变`；最旧一版为空。
    ///
    /// 不做行级 diff，只给"大小变了多少、指纹是否变"这两个能一眼扫的值。
    pub delta_label: String,
}

/// 一行回收站条目（**宿主快照**：对话框只渲染与派发动作）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashRow {
    /// 回收站条目 id（还原 / 永久删除要用）。
    pub trash_id: String,
    /// 原显示名（文件名 / 目录名）。
    pub name: String,
    /// 原位置（相对项目根，如 `resources/reports/dau.sql`）——"还原到哪"就靠它回答。
    pub original_label: String,
    /// 文件 / 目录（目录的"大小"没有意义，靠这一列区分）。
    pub kind_label: String,
    /// 删除时间（绝对时间：回收站是查凭证的地方，相对时间不够用）。
    pub time_label: String,
    /// 大小（目录为空）。
    pub size_label: String,
    /// 归档粒度（copy / reference，§12.1.1 ③）：决定还原 / 永久删除的本体命运与文案分派。
    pub payload_mode: PayloadMode,
    /// 登记行 id（reference 条目按它复活 / 永久删除；copy 条目一并带上，动作分派统一口径）。
    pub resource_id: String,
    /// 是否有受管本体在回收站：copy = true（本体在 `ProjectTrash`）；reference = false（本体留源位置）。
    pub has_payload: bool,
}

/// 修复行所属的分组（与原型 §4.5 的三组一一对应）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepairGroup {
    /// 有文件、无记录。
    Untracked,
    /// 有记录、无本体。
    Missing,
    /// 指纹不匹配。
    Changed,
}

impl RepairGroup {
    /// 分组顺序（原型 §4.5 自上而下，也是"越靠上越接近新增"的顺序）。
    pub const ALL: [Self; 3] = [Self::Untracked, Self::Missing, Self::Changed];

    /// 分组标题。
    pub fn title(self) -> &'static str {
        match self {
            Self::Untracked => "有文件、无记录",
            Self::Missing => "有记录、无本体",
            Self::Changed => "指纹不匹配",
        }
    }

    /// 分组说明：一句话讲清这一组是什么、动作会做什么。
    pub fn hint(self) -> &'static str {
        match self {
            Self::Untracked => "本体在 resources/ 里但登记表不认识它；补登后就是一个正常存档",
            Self::Missing => "本体不在了：从回收站还原（若当初进了回收站），或删掉记录",
            Self::Changed => {
                "本体被绕过只读改过：接受当前内容（生成新版本），或到版本历史里挑一版还原"
            }
        }
    }
}

/// 一行待修复项（**宿主快照**：对话框只渲染与派发动作，不查文件系统）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepairRow {
    pub group: RepairGroup,
    /// 行标题：未登记的是文件名，另两类是存档名。
    pub title: String,
    /// 副文案：本体相对路径 / 期望位置。
    pub detail: String,
    /// 指纹对照（仅 `Changed` 行有值）。
    pub hash_detail: String,
    /// 未登记行的本体相对路径（补登要用）。
    pub rel_path: String,
    /// 另两类的存档 id（删记录 / 接受内容 / 打开版本历史要用）。
    pub resource_id: Option<String>,
}
