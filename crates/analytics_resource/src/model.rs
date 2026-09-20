//! 领域语义类型（Phase 0）：存档种类 / 复现强度 / 本体状态 / 归档凭证三件套。
//!
//! 与 `models.rs` 的分工：`models.rs` 是 v1 搬运的**持久层行模型**（表结构镜像，字段一一对应），
//! 本文件是**领域语义**（"这个存档能不能复现""本体还在不在"）。
//! 两者最终合并为一份（见 `docs/architecture/analytics_resource/analytics-resource-architecture.md` §8.1），
//! 迁移期并存、且**不要**在两个文件里重复定义同一概念。

use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::PathBuf;

/// 存档种类：决定**本体存在哪**，进而决定复现强度（架构 §2.2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveKind {
    /// 受管文件：本体在 `{项目}/resources/`，归档后只读（第一期唯一落地的种类）。
    #[default]
    File,
    /// 分析表：本体在 `{项目}/.RSmeta/analytics.duckdb`，靠定义重建（第二期）。
    Analysis,
    /// 远端表引用：本体不在本机，失效风险最高（不承诺，见架构 §2.2）。
    TableRef,
}

impl ArchiveKind {
    /// 全部种类：顺序即**筛选菜单的候选顺序**与状态行的桶顺序（两处都按它渲染，避免各写一份）。
    pub const ALL: [Self; 3] = [Self::File, Self::Analysis, Self::TableRef];

    /// 界面文案：回答"本体在哪"（`文件` / `分析表` / `引用`）。
    ///
    /// 与 `resource_view::strength_badge` 的文案刻意分开：那个回答"能不能复现"，
    /// 两者在 `File` 上恰好同词（`已归档` vs `文件`），不要合并——合并后必然要在
    /// 其中一处的语义上撒谎。
    pub fn label(self) -> &'static str {
        match self {
            Self::File => "文件",
            Self::Analysis => "分析表",
            Self::TableRef => "引用",
        }
    }

    /// 落库值（迁移 020 的 `kind` 列，带 `CHECK`）。
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Analysis => "analysis",
            Self::TableRef => "table_ref",
        }
    }

    /// 从库值解析：**未知值回退 `File`**。
    ///
    /// 回退而非报错是刻意的：将来新增种类时，旧版本读到新行不应整表失败；
    /// 旧行（007 时代、迁移时默认 `file`）也必须能读。
    pub fn from_db_str(value: &str) -> Self {
        match value {
            "analysis" => Self::Analysis,
            "table_ref" => Self::TableRef,
            _ => Self::File,
        }
    }

    /// 复现强度：由种类派生，**不落库**（架构 §2.2）。
    pub fn strength(self) -> ReproductionStrength {
        match self {
            Self::File => ReproductionStrength::Strong,
            Self::Analysis => ReproductionStrength::Medium,
            Self::TableRef => ReproductionStrength::Weak,
        }
    }
}

impl std::fmt::Display for ArchiveKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_db_str())
    }
}

/// 复现强度：界面必须常显（"这东西半年后还打不打得开"）——原型 §1 原则 1。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReproductionStrength {
    /// 内容冻结，重跑得同解（`File`）。
    Strong,
    /// 定义冻结，数据可重建（`Analysis`）。
    Medium,
    /// 只记"当时指向哪"（`TableRef`）。
    Weak,
}

/// 本体健康状态：由**索引 ↔ 文件系统**比对得出，**不落库**（架构 §7.2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveStatus {
    /// 索引有记录、本体在位；`content_hash` 与记录一致。
    Normal,
    /// 索引有记录、本体不在（被手工删除 / 移动）。
    Missing,
    /// 本体在位，但与记录的内容指纹不一致（只读属性被绕过 / 外部工具改过）。
    ContentChanged,
}

/// 归档凭证里的"出处"（架构 §2.3）：回答"这结论是用什么数据得出的"。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArchiveBinding {
    /// 来源草稿的相对路径（`scratchpad/` 下，归档时定）。
    pub promoted_from: Option<String>,
    /// 来源连接 id（可空；值取自草稿 `file_meta.last_connection_id`）。
    pub source_connection_id: Option<String>,
    /// 来源表 `schema.table`（可空）。
    pub source_table: Option<String>,
}

/// 历史内容副本的保留策略（设置项 `resources.keep_versions` 的领域口径，架构 §5.2）。
///
/// 保留的只是**内容副本**：版本行（元数据）永久保留，界面上以"副本缺失"标注。
/// 设置项那边是有符号数（`-1` 是哨兵），转换成领域类型的那一步在宿主侧做——
/// 设置层不依赖 M6，转换放在两边都看得见的地方（`from_setting` / `to_setting`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeepVersions {
    /// 全部保留（设置项写 `-1`）：不裁剪任何副本。
    All,
    /// 只留版本元数据，不留内容副本（设置项写 `0`）。
    MetadataOnly,
    /// 保留最近 `n` 份内容副本（`n ≥ 1`）。
    Keep(u32),
}

impl KeepVersions {
    /// 设置项 → 领域口径（`-1` 及以下都是全留；正数封顶在 `u32`）。
    pub fn from_setting(value: i64) -> Self {
        match value {
            n if n <= -1 => Self::All,
            0 => Self::MetadataOnly,
            n => Self::Keep(n.min(u32::MAX as i64) as u32),
        }
    }

    /// 领域口径 → 设置项（页面与 JSON 里看到的就是这个数）。
    pub fn to_setting(self) -> i64 {
        match self {
            Self::All => -1,
            Self::MetadataOnly => 0,
            Self::Keep(n) => i64::from(n),
        }
    }

    /// 裁剪时的保留份数；`None` = 不裁剪（全留）。
    pub fn limit(self) -> Option<u32> {
        match self {
            Self::All => None,
            Self::MetadataOnly => Some(0),
            // `Keep(0)` 与 `MetadataOnly` 同义（类型上仍可能出现，此处归一）。
            Self::Keep(n) => Some(n),
        }
    }

    /// 人读文案（对话框提示与回执用）。
    pub fn label(self) -> String {
        match self {
            Self::All => "全部保留".to_string(),
            Self::MetadataOnly => "只留版本元数据".to_string(),
            Self::Keep(n) => format!("保留最近 {n} 份"),
        }
    }
}

/// 归档请求：上游（草稿箱 / 本地文件选择）发起，M6 只收**路径 + 元数据**。
///
/// 刻意不接收 `ScratchpadStore` 之类的上游类型——依赖方向是 `scratchpad → analytics_resource`，
/// M6 不认识草稿箱的内部结构（架构 §6.1）。
#[derive(Debug, Clone)]
pub struct ArchiveRequest {
    /// 源文件绝对路径（草稿箱内或系统任意位置）。
    pub source_path: PathBuf,
    /// 目标相对路径（`resources/` 下，通常保留来源目录结构）。
    pub rel_path: String,
    /// 显示名（可与文件名不同；重命名只改它，不动物理路径）。
    pub name: String,
    /// 别名（可空）。
    pub alias: Option<String>,
    /// 存档种类。
    pub kind: ArchiveKind,
    /// **分析表事实**（`kind = Analysis` 时由宿主采集后随请求带入）。
    ///
    /// `Some` 时：指纹取 [`crate::analysis::AnalysisFacts::fingerprint`]（定义 + 结构）
    /// 而**不是**文件字节指纹，并把定义 / 行数×列数一并登记；`None` = 按文件型归档。
    pub analysis: Option<crate::analysis::AnalysisFacts>,
    /// 来源绑定。
    pub binding: ArchiveBinding,
    /// 初始标签（可空）。
    pub tags: Vec<String>,
    /// 归入分组（可空）。
    pub group_id: Option<String>,
    /// 历史内容保留策略；`None` = 跟随设置默认（架构 §5.2）。
    pub keep_versions: Option<KeepVersions>,
    /// 再归档时由上游带回的来源存档 id（取回后改完再归档，见架构 §6.4）。
    ///
    /// `None` = 首次归档（新存档）；`Some` 时命中已有存档：内容指纹未变则不产生新版本。
    pub existing_resource_id: Option<String>,
}

/// 一次归档的**撤销凭据**（原型 §4.1 的"立即反悔窗口"）。
///
/// 为什么原路径跟着凭据走、而不入库：归档**不往库里记"本体原来在哪"**
/// （`ArchiveBinding.promoted_from` 记的是来源草稿的**相对**路径，本地文件归档时它还是空的），
/// 而撤销窗口只有几秒——在内存里传比给所有存档加一列更诚实：过期就没了，
/// 不会在库里留一个"看起来能撤销"的字段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveUndo {
    /// 存档 id（撤销要删掉它的登记行）。
    pub resource_id: String,
    /// 显示名（撤销栏文案用）。
    pub name: String,
    /// 本体被搬走前的绝对路径（撤销时移回去；**不覆盖**已有文件，见 `undo_archive`）。
    pub source_path: std::path::PathBuf,
}

/// 一次「编辑标签」的目标（单击是一元、多选是 N 元）。
///
/// 为何把**显示名**一起带上：对话框标题与动作回执都要报「哪一条 / 哪几项」，
/// 而回执在宿主侧拼（不能回头读面板的选中态——那是渲染期正在被借用的对象，已踩过）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagTarget {
    /// 存档 id。
    pub id: String,
    /// 显示名（回执与对话框标题用）。
    pub name: String,
}

/// 归档结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveOutcome {
    /// 存档 id（再归档时保持不变）。
    pub resource_id: String,
    /// 归档后的版本号。
    pub version: i32,
    /// 本次内容的指纹。
    pub content_hash: String,
    /// 本体在 `resources/` 下的相对路径。
    pub file_rel_path: String,
    /// `false` = 内容与当前版本相同，**未产生新版本**（幂等，见架构 §5.1）。
    pub created_new_version: bool,
    /// 附属项（标签 / 分组）未落上的说明（空 = 全部落上）。
    ///
    /// 归档本身成功了才算成功——这两项都能在面板上补做，所以它们失败**不回滚**主操作，
    /// 但也不能沉默：宿主把这里的内容写进回执（状态栏）。
    pub notes: Vec<String>,
}

/// 取回（检出）请求：把存档**复制**成草稿箱里的工作副本，本体不动（架构 §6.4）。
#[derive(Debug, Clone)]
pub struct CheckoutRequest {
    /// 目标存档 id。
    pub resource_id: String,
    /// 工作副本的**绝对**目标路径。
    ///
    /// 由调用方（草稿箱）给出，而不是 M6 拼 `scratchpad/`：M6 不认识上游模块的目录结构，
    /// 只知道自己的 `resources/`（依赖方向 `scratchpad → analytics_resource`）。
    pub dest_path: PathBuf,
}

/// 取回结果。
#[derive(Debug, Clone)]
pub struct CheckoutOutcome {
    /// 存档 id（再次归档时保持不变）。
    pub resource_id: String,
    /// 取回时的存档版本（用于提示"修改后再归档将生成 vN+1"）。
    pub version: i32,
    /// 落地的草稿绝对路径。
    pub dest_path: PathBuf,
}

/// 回收站条目的来源标记：本模块（原型 §4.7）。
///
/// 与 `scratchpad::ORIGIN_SCRATCHPAD` 对位：回收站层只如实记录来源，
/// “跨模块还原必须被拒”的校验在各自的服务层。值取本体目录名——
/// 两者概念上是同一个东西，不要各写一份字面量。
pub const ORIGIN_RESOURCES: &str = crate::payload::RESOURCES_DIR_NAME;

/// 移入回收站的结果（一条存档一项；顺序与入参一致）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashArchiveEntry {
    pub resource_id: String,
    /// 存档显示名（回执文案用）。
    pub name: String,
    /// 回收站条目 id（还原要用它）。
    pub trash_id: String,
}

/// 变更原因：事件必须带原因，面板据此决定局部更新还是整表刷新（架构 §6.2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeReason {
    /// 归档（草稿/本地文件 → 存档）。
    Archived,
    /// 再归档产生新版本。
    Updated,
    /// 取回（检出）出工作副本。
    CheckedOut,
    /// 撤销归档（本体移回原位 + 删登记行）。
    Undone,
    /// 从版本历史还原（旧内容 → 生成新版本，与 `Updated` 的区别在发起方与文案）。
    Restored,
    /// 移入项目级回收站（本体进 `.RSmeta/trash` + 登记行软删）。
    Trashed,
    /// 从项目级回收站还原（本体回原位 + 登记行复活）。
    Untrashed,
}

/// 资产库变更事件。
///
/// v1 的 `analytics-resource-changed` 是"后端发了、前端没人听"的死事件；v2 用
/// 服务上的广播通道，**发/收两端必须同批落地**才有意义。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourcesChanged {
    pub reason: ChangeReason,
    /// 相关存档 id；批量/结构变更时为 `None`。
    pub resource_id: Option<String>,
}

/// 新归档行：写入索引层的最小输入，字段与迁移 020 的新列一一对应。
///
/// 刻意不扩 `CreateResourceRequest`：那是 v1 的通用创建入口（无 kind / 指纹概念），
/// 混在一起会让两条语义互相污染。
#[derive(Debug, Clone)]
pub struct NewArchiveInput {
    /// 资源类型词表（待统一到 shared 枚举，见开发方案 P0.7）。
    pub resource_type: String,
    /// 显示名。
    pub name: String,
    /// 别名（可空）。
    pub alias: Option<String>,
    /// 存档种类。
    pub kind: ArchiveKind,
    /// 内容指纹。
    ///
    /// 文件型 = 本体字节的 sha256；分析表型 = 定义 + 结构摘要
    /// （`crate::analysis::AnalysisFacts::fingerprint`，**行数不进**——见那里头的裁决）。
    pub content_hash: String,
    /// 重建定义（`analysis` 型的配方；其余型为 `None`）。
    pub definition_sql: Option<String>,
    /// 规模（`analysis` 型的行数 / 列数：**元信息**，不参与指纹）。
    pub row_count: Option<i32>,
    pub column_count: Option<i32>,
    /// 本体相对路径。
    pub file_rel_path: String,
    /// 本体字节数（登记时现读；读不到给 `None`，不假装 0）。
    ///
    /// 三个去处：行的体积尾巴、详情面板的大小、列表的「大小」排序——排序比的是这个原始值，
    /// 而不是格式化后的 `1.2 KB`（那会静默排错）。
    pub file_size: Option<i64>,
    /// 来源绑定（归档凭证的"出处"）。
    pub binding: ArchiveBinding,
    /// 作用域（派生只读量，当前只产 `project`；架构 §4.3）。
    pub scope: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_db_roundtrip() {
        for kind in [
            ArchiveKind::File,
            ArchiveKind::Analysis,
            ArchiveKind::TableRef,
        ] {
            assert_eq!(ArchiveKind::from_db_str(kind.as_db_str()), kind);
        }
        assert_eq!(ArchiveKind::default(), ArchiveKind::File);
    }

    #[test]
    fn kind_labels_cover_all_kinds_in_menu_order() {
        let labels: Vec<&str> = ArchiveKind::ALL.iter().map(|kind| kind.label()).collect();
        assert_eq!(labels, vec!["文件", "分析表", "引用"]);
    }

    #[test]
    fn kind_unknown_value_falls_back_to_file() {
        // 新种类由旧版本读到、或 007 时代旧行，都不应整表失败。
        assert_eq!(ArchiveKind::from_db_str("future_kind"), ArchiveKind::File);
        assert_eq!(ArchiveKind::from_db_str(""), ArchiveKind::File);
    }

    #[test]
    fn strength_is_derived_from_kind() {
        assert_eq!(ArchiveKind::File.strength(), ReproductionStrength::Strong);
        assert_eq!(
            ArchiveKind::Analysis.strength(),
            ReproductionStrength::Medium
        );
        assert_eq!(ArchiveKind::TableRef.strength(), ReproductionStrength::Weak);
    }

    #[test]
    fn binding_default_is_all_empty() {
        let binding = ArchiveBinding::default();
        assert!(binding.promoted_from.is_none());
        assert!(binding.source_connection_id.is_none());
        assert!(binding.source_table.is_none());
    }
}
