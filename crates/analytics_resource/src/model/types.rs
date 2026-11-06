//! 请求 / 回执 / 目标切片 / 变更事件与写入输入。

use std::path::PathBuf;


use super::*;

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
    /// **File 档归档粒度**（默认 `Copy`，2026-10-13 裁决）：copy = 本体复制进 `resources/`；
    /// reference = 本体留原位、只存元数据。
    ///
    /// `Analysis` 档（`analysis` 为 `Some` 时）恒为 `Copy`（本体由采集侧落库），
    /// 此字段只对 `File` 档有意义。
    pub payload_mode: PayloadMode,
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

/// 移入回收站的选集条目（§12.1.1 ③）：带上归档粒度，确认框文案按 copy / reference 区分
/// 本体命运（copy 搬副本进回收站；reference 本体留原位、只删登记行）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteTarget {
    /// 存档 id。
    pub id: String,
    /// 归档粒度（视图从详情快照取；缺省回退 `Copy`）。
    pub mode: PayloadMode,
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

/// reference 型条目的回收站 id 前缀：这类条目没有实体回收站 manifest，
/// id 由登记行 id 合成，还原 / 永久删除据此分派到"只动登记行"的分支（§12.1.1 ③）。
pub const TRASH_ID_REF_PREFIX: &str = "ref:";

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
/// 归档语义独立成类型、不复用 v1 的通用资源创建入口（该入口已随 v1 CRUD 移除，
/// 无 kind / 指纹概念）：混在一起会让两条语义互相污染。
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
    /// **File 档归档粒度**（迁移 023）：copy / reference。
    pub payload_mode: PayloadMode,
    /// reference 模式的源**绝对**路径（copy 模式为 `None`）。
    ///
    /// 打开 / 取回 / 历史副本 / 指纹校验都从它解析本体；源被删 / 改 → “源已变更，快照失效”。
    pub source_path: Option<String>,
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
