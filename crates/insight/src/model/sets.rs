//! 洞察模型 · 集合与面板标签（自 model.rs 拆分，纯位移）

use crate::model::InsightTarget;

// ==================== 面板骨架 ====================

/// 已洞察表的来源种类（右栏「已洞察表」集合行的徽标）。
///
/// 对象统一是 DuckDB 临时表，但**入口来源**分三路（用户口径：数据库表 /
/// 草稿箱 CSV 文件 / select 查询结果集），集合行据此给出第一眼辨识。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsightSetKind {
    /// 源库连接上的表（`InsightTarget::SourceTable` 且带连接）
    SourceTable,
    /// 查询结果集 / 已入内存库的表（`InsightTarget::Table`）
    ResultSet,
    /// 草稿箱文件（CSV / Parquet / Excel：`InsightTarget::SourceTable` 但跑在 DuckDB 内存库）
    File,
}

impl InsightSetKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::SourceTable => "源表",
            Self::ResultSet => "结果集",
            Self::File => "文件",
        }
    }
}

/// 已洞察表集合条目（右栏管理列表的一行）。
///
/// 数据源是中间工作页的 `opened`——只有**进入过洞察**的表才入列
/// （用户口径「只有进入洞察后的表才能选择对比」）；这里只装展示与操作
/// 所需的最小事实，画像载荷仍在中间页（对比聚合不重复存储）。
#[derive(Debug, Clone, PartialEq)]
pub struct InsightSetEntry {
    /// 稳定业务键（与对比勾选共用 `target_key`）
    pub key: String,
    /// 展示名（表名）
    pub label: String,
    /// 对象来源种类
    pub kind: InsightSetKind,
    /// 来源描述（`source.label`；内存库对象给临时表名）
    pub source: String,
    /// 洞察临时表名（已完成取样的对象才有；`None` = 还在取样中）。
    ///
    /// 关系排查 / 归档的 SQL 前提：源表目标取样完成后中间页会把 target 更新为
    /// `InsightTarget::Table`，`to_set_entry` 据此回填——未取样时按钮置灰，
    /// 不拿 label 去猜表名。
    pub temp_table: Option<String>,
    /// 最近一次表画像的行数 / 列数（未完成画像为 `None`，不编假值）
    pub row_count: Option<u64>,
    pub column_count: Option<usize>,
    /// 检查器命中计数（评估全表完成后才有；无评级语义）
    pub checker_hits: Option<usize>,
    pub checker_applicable: Option<usize>,
    /// 还原目标：集合行点击「切换」时宿主据此重新打开该目标（2026-10-30 多标签
    /// 与原型 it-row 点击语义的还原依据——entry 只存展示最小事实，目标本体在中间页）
    pub target: InsightTarget,
    /// 是否中间页当前指向的目标（高亮用）
    pub is_current: bool,
}

/// 跨洞察表列值重叠结果的一行（关系排查引擎的产出）。
///
/// 语义是「值重叠」而非「外键关系」：两表同名列各取去重值域，算交集占比——
/// 值域高度重叠说明两表可能在描述同一批实体（可进一步做键级对齐）。
#[derive(Debug, Clone, PartialEq)]
pub struct OverlapRow {
    /// 参与比较的列名（两表同名列）
    pub column: String,
    /// 交集唯一值数
    pub overlap: i64,
    /// 交集占 A 表（发起方）唯一值域的比例
    pub rate_a: f64,
    /// 交集占 B 表（被对比方）唯一值域的比例
    pub rate_b: f64,
}

/// 一次跨洞察表关系排查的完整结果（右栏集合卡「关系排查」区渲染）。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OverlapView {
    /// 发起方表名（临时表名，展示用 label 由视图取集合条目）
    pub table_a: String,
    /// 被对比方表名
    pub table_b: String,
    /// 同名列值重叠明细（按交集数降序）
    pub rows: Vec<OverlapRow>,
}

impl OverlapView {
    /// 有没有可对比的同名列（空 = 两表没有类型兼容的同名列，UI 给说明）
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// 面板的六个 Tab（顺序固定；标签 2~3 字，280px 右栏内六项不溢出）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelTab {
    /// 摘要（D66 对齐原型右栏六 Tab：首置，汇总当前目标的画像摘要）
    Overview,
    /// 列画像（Phase 1 落地）
    Column,
    /// 表探查（Phase 3）
    Table,
    /// 多列分析（Phase 3）
    MultiColumn,
    /// 结构洞察（Phase 4）
    Schema,
    /// 快照历史与版本对比（Phase 5）
    History,
}

impl PanelTab {
    /// 展示顺序
    pub const ALL: [PanelTab; 6] = [
        PanelTab::Overview,
        PanelTab::Column,
        PanelTab::Table,
        PanelTab::MultiColumn,
        PanelTab::Schema,
        PanelTab::History,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PanelTab::Overview => "摘要",
            PanelTab::Column => "列",
            PanelTab::Table => "表",
            PanelTab::MultiColumn => "多列",
            PanelTab::Schema => "结构",
            PanelTab::History => "历史",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }

    pub fn from_index(ix: usize) -> Option<Self> {
        Self::ALL.get(ix).copied()
    }
}