//! 详情快照：一条存档的属性面（宿主已格式化为可读文案）。

use crate::model::{ArchiveKind, ArchiveStatus, PayloadMode, RowScope};
use crate::preview::Preview;

/// 一条存档的详情快照（**宿主已格式化**：大小 / 时间 / 标签等都已是人读文案）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveDetail {
    pub id: String,
    pub name: String,
    pub alias: Option<String>,
    pub kind: ArchiveKind,
    pub version: i32,
    pub status: ArchiveStatus,
    /// 归档后只读（`kind = analysis` 也可能为 false：表本身不可加只读属性）。
    pub readonly: bool,
    /// File 档归档粒度（copy / reference，迁移 023）；决定本体位置与详情文案分派。
    pub payload_mode: PayloadMode,
    /// reference 模式的源绝对路径（copy 为 `None`）。
    pub source_path: Option<String>,
    /// 实例归属（§13 双实例）：项目行可操作，全局行只读 + 可分发。
    pub scope: RowScope,
    /// 大小（文件型）或规模（分析表型，如 "12,480 行 × 18 列"）；旧行可能为空。
    pub size_label: String,
    pub modified_label: String,
    pub archived_label: String,
    /// 来源草稿相对路径（归档凭证的"出处"）。
    pub promoted_from: Option<String>,
    pub source_connection_id: Option<String>,
    pub source_table: Option<String>,
    /// 内容指纹（完整值；展示时截断）。
    pub content_hash: Option<String>,
    /// 本体在 `resources/` 下的相对路径（`None` = 旧行没有登记）。
    ///
    /// 不在面板上展示（原型 §3.1 没有这一行），但**取回**要用它推默认工作副本名
    /// （显示名与扩展名是两回事：显示名可以改成中文，扩展名不行）。
    pub payload_rel_path: Option<String>,
    /// 版本历史摘要，如 "3 个历史版本 · 最近副本完整"。
    pub history_label: String,
    /// 这存档挂的标签（带 id：去标要用它）；名字已就绪，不在渲染期查库。
    pub tags: Vec<ArchiveTagChip>,
    /// 内容预览（原型 §3.1 的最后一块）：文本型给前 20 行，其余给一句说明。
    ///
    /// 缺省是 `Preview::default()`（「仅元信息」）：分析表 / 引用型本就没有可预览的文本，
    /// 宿主没给预览时也不假装有内容。
    pub preview: Preview,
    pub group: Option<String>,
}

/// 详情面板里的一枚标签 chip。
///
/// `color` 是标签自己的可选项（用户填的 hex），但**不拿它上色**：颜色一律走主题 token
/// （原型 §6 的零裸色约束），标签颜色留给后续“按颜色分组”一类需求再谈。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveTagChip {
    pub id: String,
    pub name: String,
}
