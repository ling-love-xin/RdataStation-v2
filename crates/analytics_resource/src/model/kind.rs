//! 归档种类、状态、本体模式与保留策略。

use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RowScope {
    /// 项目实例（登记在项目 SQLite，数据在项目 DuckDB / resources/）。
    #[default]
    Project,
    /// 全局实例（登记在全局 SQLite 镜像表，数据在全局 DuckDB）。
    Global,
}

impl RowScope {
    /// 落库值（迁移 025 的 `scope` 列）。
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Global => "global",
        }
    }

    /// 从库值解析：未知 / 空值回退 `Project`（旧行与脏数据不致整表失败）。
    pub fn from_db_str(value: &str) -> Self {
        match value {
            "global" => Self::Global,
            _ => Self::Project,
        }
    }
}

/// 存档种类：决定**本体存在哪**，进而决定复现强度（架构 §2.2）。
///
/// 2026-11-12 口径修正：`Analysis` 并不具备「靠定义重建数据」的能力——
/// `definition_sql` 全仓只被登记与算指纹，**从不执行**；它的本体与 `File` 一样是
/// `resources/` 下的受管文件快照。因此种类只描述「形态」，不承诺重建能力。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveKind {
    /// 受管文件：本体在 `{项目}/resources/`，归档后只读（第一期唯一落地的种类）。
    #[default]
    File,
    /// 分析表：本体同样是受管文件快照，额外登记定义 / 列结构指纹；**定义不执行、不重建数据**。
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
    ///
    /// 2026-11-12 降级：`Analysis` 与 `File` 同级（都是内容冻结的受管文件）。
    /// 曾经把它标成 `Medium` 并承诺「定义冻结，数据可重建」，但定义从不执行、
    /// 数据也不会重建——对一个主打消除误解的模块，界面不该承诺做不到的事。
    pub fn strength(self) -> ReproductionStrength {
        match self {
            Self::File | Self::Analysis => ReproductionStrength::Strong,
            Self::TableRef => ReproductionStrength::Weak,
        }
    }
}

impl std::fmt::Display for ArchiveKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_db_str())
    }
}

/// 复现强度：回答「这东西半年后还打不打得开」（原型 §1 原则 1）。
///
/// 只保留能被现有实现兑现的两档：**本体是否冻结在本项目内**。
/// 分析表不再单列中档——它的定义只是元信息，不构成第三种复现能力。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReproductionStrength {
    /// 内容冻结在本项目，重跑得同解（`File` / `Analysis`）。
    Strong,
    /// 只记"当时指向哪"，本体不在本项目（`TableRef`）。
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
    /// 出处（一字段两义，读时按 [`PromotedFrom`](crate::model::PromotedFrom) 解释）：
    /// 从草稿归档写 `scratchpad/<rel>`，升级到全局 / 分发落项目写 `global:<全局实例 id>`。
    pub promoted_from: Option<String>,
    /// 来源连接 id（可空；值取自草稿 `file_meta.last_connection_id`）。
    pub source_connection_id: Option<String>,
    /// 来源表 `schema.table`（可空）。
    pub source_table: Option<String>,
}

/// File 档归档粒度（2026-10-13 裁决：两档都做、默认 copy）。
///
/// 回答"本体放哪"：copy = 复制进 `resources/`（版本冻结、回溯读副本永不失效，草稿箱保留为
/// 工作副本 checkout 语义）；reference = 只存元数据（地址 + 文件名 + 指纹 + 版本记录），
/// 本体留原位、打开校验指纹（不一致提示"源已变更，快照失效"）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PayloadMode {
    /// 本体复制进 `resources/` + 只读；草稿箱那份保留为工作副本。
    Copy,
    /// 本体留原位；只存元数据 + 源指纹；打开时校验指纹。
    Reference,
}

impl PayloadMode {
    /// 默认模式：copy（裁决）。
    pub const DEFAULT: Self = Self::Copy;

    /// 落库值（迁移 023 的 `payload_mode` 列，带 `CHECK`）。
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Copy => "copy",
            Self::Reference => "reference",
        }
    }

    /// 从库值解析：**未知值回退 `Copy`**（与 `ArchiveKind::from_db_str` 同一取舍：
    /// 旧版本读到新值不应整表失败；旧行默认 copy）。
    pub fn from_db_str(value: &str) -> Self {
        match value {
            "reference" => Self::Reference,
            _ => Self::Copy,
        }
    }

    /// 界面文案（归档对话框 / 详情面板标注用）。
    pub fn label(self) -> &'static str {
        match self {
            Self::Copy => "复制",
            Self::Reference => "引用",
        }
    }

    /// 归档对话框单选行的说明小字（回答"选了之后会怎样"）。
    pub fn hint(self) -> &'static str {
        match self {
            Self::Copy => "复制进 resources/ 并变只读；源文件保留为工作副本，回溯永不失效",
            Self::Reference => "只登记元数据与指纹，本体留在源位置；打开时校验源文件指纹",
        }
    }

    /// 选中后表单下方的整段说明（规格 §12.1.1 ①：随模式切换，与单选行小字互补）。
    pub fn note(self) -> &'static str {
        match self {
            Self::Copy => "草稿保留为工作副本，可继续编辑；再次归档将生成新版本",
            Self::Reference => "本体留原位，只存元数据（地址 + 指纹 + 版本）；源变更后快照失效",
        }
    }
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
