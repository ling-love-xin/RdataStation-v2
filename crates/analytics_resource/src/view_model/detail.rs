//! 详情快照：一条存档的属性面（宿主已格式化为可读文案）。

use crate::model::{ActionContext, ArchiveKind, ArchiveStatus, PayloadMode, RowScope};
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
    /// 资产进入本库的时间（`created_at`，只在第一版写过一次）。
    ///
    /// 与 [`modified_label`](Self::modified_label) 不是一回事：那是**当前这一版**写入的时间，
    /// 每升一版都会变；这个回答“这东西什么时候开始存在”，与草稿箱文件属性里的「创建时间」同义。
    pub created_label: String,
    /// 当前版本写入时间（`updated_at`）。
    pub modified_label: String,
    pub archived_label: String,
    /// 来源草稿相对路径（归档凭证的"出处"）。
    pub promoted_from: Option<String>,
    /// 跨项目链路的**直接上游**（`origin_kind` + `origin_ref` 拼好的一句话）。
    /// 空串 = 不画那一行（没有上游 / 老行 / ref 被写坏都归到这一档）。
    pub origin_label: String,
    pub source_connection_id: Option<String>,
    pub source_table: Option<String>,
    /// 内容指纹（完整值；展示时截断）。
    pub content_hash: Option<String>,
    /// 本体在 `resources/` 下的相对路径（`None` = 旧行没有登记）。
    ///
    /// 详情「基本信息」摆它（用户需要知道本体落在哪）；**取回**也用它推默认工作副本名
    /// （显示名与扩展名是两回事：显示名可以改成中文，扩展名不行）。
    pub payload_rel_path: Option<String>,
    /// 本体表名（**表型资产的取数依据**：`kind = AnalyticsTable` 时它是分析库里的表名）。
    ///
    /// 文件型为 `None`。详情「基本信息」摆它（表名是拿来拼 SQL 的定位信息，不给就没法照抄）；
    /// 「查看统计 / 双击打开」也靠它拼取样 SQL——过去拿不到它，于是这类行只能静默失败。
    pub analytics_table: Option<String>,
    /// 是否带**结构指纹**（`definition_sql` 非空）：指纹口径与普通文件不同（定义 + 列结构，
    /// 行数不算）。提示文案与指纹 tooltip 都按它分派——种类收成两档后，这件事不再由 `kind` 表达。
    pub has_definition: bool,
    /// 重建定义（配方）：`analysis` 型“这份东西怎么算出来的”，文件型为 `None`。
    ///
    /// **只登记、不执行**（见 `analysis::FINGERPRINT_HINT`）：摆出来是为了让人看懂
    /// 这一版的口径，不是让程序据此重建——文案不能说成“重新计算”。
    pub definition_sql: Option<String>,
    /// 版本历史摘要，如 "3 个历史版本 · 最近副本完整"。
    pub history_label: String,
    /// 版本**时间线**的节点（当前版本在最前，其后是历史版本；数量上限由装配层定）。
    ///
    /// 每个节点可点开看它自己的信息（时间 / 体积 / 指纹 / 变更 / 副本状态）：
    /// 详情面板里能"停在某一版上"看，比只有一行摘要有用。
    /// 空 = 连当前版本都没有（不该发生；渲染层据此不画），正常至少有一个节点。
    pub version_timeline: Vec<DetailVersionLine>,
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
    /// 业务断言（口径分区用；`None` = 尚未填写）。
    pub assertion: Option<crate::model::ArchiveAssertion>,
    /// 业务效力（与 `status` 的物理完整性正交：文件被改过 ≠ 结论已废弃）。
    pub lifecycle: crate::model::Lifecycle,
    /// 全局行的使用面摘要，如 "2 个项目在用（最近 v3，累计 5 次分发，最近 2026-11-14）"。
    ///
    /// 空 = 不显示这一行：项目行没有「被谁用过」这个概念（它的来源在 `promoted_from`），
    /// 全局行没人分发过也不该占一行。
    pub dispatch_label: String,
}

impl ArchiveDetail {
    /// 装成动作判定的上下文（[`crate::model::ActionContext`]）。
    ///
    /// `read_only` 是**项目**的东西（不是 `self.readonly` 那一列：那是归档只读），
    /// 所以由调用方给；`multi_count` 同理（详情默认单选，列表多选时才 > 1）。
    pub fn action_context(&self, read_only: bool, multi_count: usize) -> ActionContext {
        ActionContext {
            scope: self.scope,
            status: self.status,
            has_payload: self.payload_rel_path.is_some(),
            read_only,
            multi_count,
        }
    }
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
    /// 是不是当前版本（时间线最上面那个节点，带「当前」标）。
    pub is_current: bool,
    /// 内容指纹前 12 位（快照解析不出来时给 `—`，不伪造）。
    pub hash_short: String,
    /// 相对上一版的差异文案（`较 v2 大小+1.2 KB · 指纹已变`；最旧一版为空）。
    pub delta_label: String,
    /// 这一版还有没有内容副本（`.RSmeta/resources/versions/<id>/<v>/`）。
    ///
    /// 裁剪**只删副本、不删版本行**，所以“有这一版”和“能还原它”是两件事：
    /// 缺副本时渲染层要明说，否则用户点「还原」只会得到一句失败。
    pub has_copy: bool,
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
