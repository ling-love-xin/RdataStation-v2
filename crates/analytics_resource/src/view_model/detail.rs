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
    /// 版本区「最近 3 条」明细（数量上限由装配层定，不在这里裁）。
    ///
    /// 只有**历史**版本（当前版本在资源行上、不重复列一行）；空 = 没有明细可列，
    /// 渲染层据此不画那几行（只留 `history_label` 那一行与「查看全部…」）。
    pub recent_versions: Vec<DetailVersionLine>,
    /// 可选分组（详情里的「移动…」选择项；全局行走空，没有项目分组归属）。
    pub groups: Vec<DetailGroupOption>,
    /// 当前分组 id（「移动…」菜单据此置灰当前项；`None` = 未分组）。
    ///
    /// 与 `group`（名字）并存：菜单要比 id，显示要比名字——名字可能重名，id 不会。
    pub group_id: Option<String>,
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

/// 详情「版本」区的一条历史版本（宿主已格式化，渲染层只拼字）。
///
/// 与版本历史对话框的行**不是同一个结构**：那边是七列表格、要选中与动作；
/// 这边只是 280px 宽面板里的三行摘要，给版本号 / 时间 / 体积。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetailVersionLine {
    pub version: i32,
    /// 归档时间（`%Y-%m-%d %H:%M`，与版本对话框同一格式）。
    pub time_label: String,
    /// 体积（分析表型给规模，快照里没有时为空串）。
    pub size_label: String,
}

/// 详情「组织」区「移动…」菜单里的一项分组。
///
/// 只带 id + 名字（菜单只需要这两个）；「未分组」不是选项而是菜单里**恒有**的一项，
/// 故不在这里占一条。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetailGroupOption {
    pub id: String,
    pub name: String,
}
