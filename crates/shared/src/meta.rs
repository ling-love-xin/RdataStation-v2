//! 数据库对象元数据 DTO（shared 层）
//!
//! 自 engine `driver::traits` 下沉（2026-10-29，engine 瘦身第二刀前置小手术）：
//! `driver`（内省）与 `persistence::metadata_cache`（缓存回填）两侧共用这些纯数据型，
//! 放 shared 避免 rds-store → rds-driver 的依赖环。driver 侧 `pub use` 重导出，
//! `engine::driver::*` 等路径保持零改动。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use specta::Type;

/// Schema 对象类型
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub enum SchemaObjectKind {
    Catalog,
    Schema,
    Table,
    View,
    Column,
    Index,
    PrimaryKey,
    ForeignKey,
    Procedure,
    Function,
    Sequence,
    Trigger,
}

/// 列所引用的外键目标。
///
/// 为什么单独一个类型而不是拼成 `"表.列"` 字符串：表名与列名都可能含点（带引号的标识符），
/// 拼串之后就再也分不回来了；而且引用的两个端点各自都是独立的展示位。
/// 取法照 dbui 的 `Column.references`（MIT）——它也是把「引到哪」放在**列上**，
/// 而不是只能从约束里反推。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ForeignKeyRef {
    /// 被引用的表名（同 schema 内不带 schema 前缀，与驱动内省口径一致）。
    pub table: String,
    /// 被引用的列名。
    pub column: String,
}

/// 列详情（完整元数据）
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ColumnDetail {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub is_primary_key: bool,
    /// 这一列是否参与外键。
    ///
    /// 与 [`ColumnDetail::references`] 的分工：本字段是「**有没有**」，`references` 是
    /// 「**引到哪**」。保留本字段是因为桥接驱动（JDBC 那类）可能只知道有没有、拿不到目标；
    /// 能拿到目标时，两者应当一致（见 `references` 的文档）。
    pub is_foreign_key: bool,
    /// 引用的目标；`None` = 不引用任何表。
    ///
    /// **与 `is_foreign_key` 的关系**：能拿到目标时 `references.is_some()` 必须等于
    /// `is_foreign_key`；拿不到目标（驱动不支持）时 `references` 为 `None` 而
    /// `is_foreign_key` 可以仍为 `true`。不要在 UI 里把 `references.is_none()` 读成
    /// 「不是外键」——请读 `is_foreign_key`。
    #[serde(default)]
    pub references: Option<ForeignKeyRef>,
    /// 列在表中的序号（**1 基**，与 `information_schema.columns.ordinal_position` 同口径）。
    ///
    /// 0 = 驱动没给。有了它，UI 不必再拿数组下标当序号（筛选 / 分页 / 重排后下标会错位）。
    #[serde(default)]
    pub ordinal: u32,
    pub default_value: Option<String>,
    pub comment: Option<String>,
    /// 这一列是否为标识 / 自增列（PG `attidentity` 非空；MySQL `EXTRA` 含
    /// `auto_increment`；SQLite / DuckDB 无原生标识概念 → false）。
    ///
    /// 与 [`ColumnDetail::identity_generation`] 的分工：本字段是「**是不是**标识列」，
    /// `identity_generation` 是「**怎么生成**」。
    #[serde(default)]
    pub is_identity: bool,
    /// 标识列的生成方式（`is_identity == true` 时有值，否则 `None`）：
    /// PG `BY DEFAULT` / `ALWAYS`；MySQL `AUTO_INCREMENT`。
    #[serde(default)]
    pub identity_generation: Option<String>,
    /// 列级排序规则（PG `attcollation` → `pg_collation.collname`；MySQL
    /// `COLLATION_NAME`；SQLite / DuckDB 无原生概念 → `None`）。
    #[serde(default)]
    pub collation: Option<String>,
    #[serde(default)]
    pub extra: HashMap<String, String>,
}

/// 索引详情
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct IndexDetail {
    pub name: String,
    pub table_name: String,
    pub column_names: Vec<String>,
    pub is_unique: bool,
    pub is_primary: bool,
    #[serde(default)]
    pub index_type: Option<String>,
    #[serde(default)]
    pub comment: Option<String>,
}

/// 约束详情
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ConstraintDetail {
    pub name: String,
    pub table_name: String,
    pub constraint_type: String,
    pub column_names: Vec<String>,
    #[serde(default)]
    pub referenced_table: Option<String>,
    #[serde(default)]
    pub referenced_columns: Vec<String>,
    #[serde(default)]
    pub update_rule: Option<String>,
    #[serde(default)]
    pub delete_rule: Option<String>,
}

/// 对象列表项（对象树 / 元数据缓存 / 属性面板共用的**唯一**结构对象表示）
///
/// 取代了 v1 的 `SchemaObject`。那个类型多带三个字段，全都是死字段：
/// `children`（懒加载整棵树）无人读取、`table_name` / `event`（触发器专有）无人填充，
/// 而 5 个原生驱动的 `list_tables` 都在做「`NodeInfo` 降级成 `SchemaObject`」的空转。
/// 一个对象只留一份表示：名字 + 类别 + 注释。
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct NodeInfo {
    pub name: String,
    pub kind: SchemaObjectKind,
    pub comment: Option<String>,
    /// 关联父对象（触发器的所属表；其他类别为空——列的父对象由路径决定，不在这里）。
    ///
    /// 不演成任意 KV 背包：只放驱动内省时**已经查到**、界面真会显示的那一项。
    pub parent_name: Option<String>,
    /// 子表是否为**分区**子表（PG `relispartition`；`parent_name` 非空时才有意义）。
    ///
    /// S3-② 分区/继承折叠：分区子表与继承子表在父表徽标上分开计数
    /// （`ChildTables { count, are_partitions }` 同形）。非 PG 驱动恒为 `false`。
    pub is_partition: bool,
}

impl NodeInfo {
    /// 只带身份的对象项（绝大多数内省点只需要名字与类别）。
    pub fn new(name: impl Into<String>, kind: SchemaObjectKind) -> Self {
        Self {
            name: name.into(),
            kind,
            comment: None,
            parent_name: None,
            is_partition: false,
        }
    }
    /// 带注释。
    pub fn with_comment(mut self, comment: Option<String>) -> Self {
        self.comment = comment;
        self
    }

    /// 带关联父对象（目前只有触发器用：所属表）。
    pub fn with_parent(mut self, parent: impl Into<String>) -> Self {
        let parent = parent.into();
        self.parent_name = (!parent.is_empty()).then_some(parent);
        self
    }

    /// 标记为分区子表（`parent_name` 非空时配合使用）。
    pub fn with_partition(mut self) -> Self {
        self.is_partition = true;
        self
    }
}

/// 一次建连批量内省的 schema 全部对象类别（`Database::introspect_schema_objects`）。
///
/// 表/视图在同一批（`tables` 内按 `kind` 区分 Table / View，等价 `list_tables` 语义）；
/// 过程 / 函数分开（PG `prokind`、MySQL 类型各自区分）。
#[derive(Debug, Clone, Default)]
pub struct SchemaObjects {
    pub tables: Vec<NodeInfo>,
    pub procedures: Vec<NodeInfo>,
    pub functions: Vec<NodeInfo>,
    pub sequences: Vec<NodeInfo>,
    pub triggers: Vec<NodeInfo>,
}

/// 对象详情（完整元数据，按需加载）
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct NodeDetail {
    pub node: NodeInfo,
    pub columns: Vec<ColumnDetail>,
    pub index_count: Option<u32>,
    pub row_count_estimate: Option<u32>,
}
