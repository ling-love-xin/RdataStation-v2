//! 结构对象的**统一引用**（跨模块寻址的唯一类型）。
//!
//! ## 为什么需要它
//!
//! 同一张表在本仓有多份表示——驱动内省的 `NodeInfo` / `ColumnDetail`、L2 的 `tables`
//! 行、`metadata_index` 行、导航 `NavNode`、洞察 `TableColumnMeta`。跨模块「引用一个对象」时
//! 各处自己拼名字，代价是两条：
//!
//! 1. **同形类型重复**：`database::model::{TableRef, SchemaRef}` 都是「连接 + 名字段」，
//!    搜索/命令等新消费者每来一个就要再抄一份；
//! 2. **无法判断两个表示是不是同一个对象**：属性面板拿到的是名字，导航树用拼串 key，
//!    索引行给的是 `schema/table` 路径——三者没有共同的比较基准。
//!
//! （2026-09-19 之前驱动内省那层还多一份 `SchemaObject`（与 `NodeInfo` 重叠），现已删除：
//! 「一个对象一份表示」在驱动接口面已经成立，剩下的差异是**关注点不同**——L2 行带存储
//! 元数据（id / `last_sync`）、`NavNode` 带 UI 状态（展开态 / 错误位）、`TableColumnMeta`
//! 来自 DuckDB `DESCRIBE` 而非元数据内省，不是重复类型。）
//!
//! [`ObjectRef`] 只回答「是哪一个对象」：**连接 + 类别 + catalog / schema / 父对象 / 名字**。
//! 它刻意**不携带**驱动名、显示名、行数估算这类会随时间变化的字段——那些由宿主按
//! `conn_id` 解析（缓存只存「上次读到的事实」，引用也不该存会过期的展示信息）。
//!
//! ## 与相邻类型的边界
//!
//! | 类型 | 是什么 | 为什么不合并 |
//! | --- | --- | --- |
//! | `driver::traits::SchemaObjectKind` | **驱动内省结果**的类别（含 `Index` / `PrimaryKey` / `ForeignKey` 这类不可寻址项） | 它描述"读到了什么"，不是"能引用什么" |
//! | `database::model::PropertyKind` | **属性面板**的展示类别（含 `Connection`，且带 UI 文案） | 合并要等属性面板重做；本批只在视图侧做一次映射 |
//! | [`ObjectKind`] | **可寻址结构对象**的闭集 | 三者的转换都在使用方一侧，都是纯函数 |
//!
//! ## 与索引的对应
//!
//! [`ObjectKind::as_index_str`] 的取值与 `metadata_index.object_type` 对齐
//! （`table` / `view` / `column` / `schema` / `routine` …），因此
//! [`ObjectRef::from_index_hit`] 能把一次索引搜索命中直接还原成引用——
//! 这是「搜索命中」与「导航树节点」指向同一对象时的对接口。

use serde::{Deserialize, Serialize};

/// 可寻址的结构对象类别。
///
/// 「可寻址」= 能被一个 [`ObjectRef`] 指到，且导航树 / 属性面板 / 索引里都有对应物。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ObjectKind {
    /// 目录 / 库（无 Catalog 层的驱动不会用到）
    Catalog,
    /// 模式（MySQL 侧库即 schema）
    Schema,
    /// 表
    Table,
    /// 视图
    View,
    /// 列
    Column,
    /// 存储过程 / 函数
    Routine,
    /// 序列
    Sequence,
    /// 触发器
    Trigger,
}

impl ObjectKind {
    /// 与 `metadata_index.object_type` 对齐的稳定小写串（落库 / 跨模块通信都用它）。
    pub fn as_index_str(self) -> &'static str {
        match self {
            Self::Catalog => "catalog",
            Self::Schema => "schema",
            Self::Table => "table",
            Self::View => "view",
            Self::Column => "column",
            Self::Routine => "routine",
            Self::Sequence => "sequence",
            Self::Trigger => "trigger",
        }
    }

    /// 从索引串还原类别；未知取值返回 `None`（**不猜**：索引里出现新类别时调用方自行决定）。
    pub fn parse_index_str(value: &str) -> Option<Self> {
        match value {
            "catalog" => Some(Self::Catalog),
            "schema" => Some(Self::Schema),
            "table" => Some(Self::Table),
            "view" => Some(Self::View),
            "column" => Some(Self::Column),
            "routine" | "procedure" | "function" => Some(Self::Routine),
            "sequence" => Some(Self::Sequence),
            "trigger" => Some(Self::Trigger),
            _ => None,
        }
    }

    /// 界面短标签（导航树 / 搜索结果行直接显示）。
    pub fn label(self) -> &'static str {
        match self {
            Self::Catalog => "Catalog",
            Self::Schema => "模式",
            Self::Table => "表",
            Self::View => "视图",
            Self::Column => "列",
            Self::Routine => "例程",
            Self::Sequence => "序列",
            Self::Trigger => "触发器",
        }
    }

    /// 是否容器（能装子对象：库 / 模式）。导航树里它们有可展开的下一层。
    pub fn is_container(self) -> bool {
        matches!(self, Self::Catalog | Self::Schema)
    }
}

/// 结构对象的引用：**跨模块唯一的寻址类型**。
///
/// 字段一律用普通字符串（空串 = 该层不存在），而不是 `Option`：导航侧与服务侧早就把层级
/// 规范化成非空串（无 Catalog 层用 `main`、无 Schema 层用库名），保持同一口径可以少一层
/// `Option` 噪音；真正可空的场景（索引行的 NULL）在 [`ObjectRef::from_index_hit`] 里收敛一次。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ObjectRef {
    /// 归属连接（缓存、连接池、驱动能力位都按它定位）
    pub conn_id: String,
    /// 对象类别
    pub kind: ObjectKind,
    /// Catalog / 库名（空串 = 该驱动没有这一层）
    pub catalog: String,
    /// Schema（空串 = 该驱动没有独立 schema 层）
    pub schema: String,
    /// 父对象名（列 → 所属表；其他类别为空串）
    pub parent: String,
    /// 对象名
    pub name: String,
}

impl ObjectRef {
    /// 直接给出全部段（其余构造器都走它）。
    pub fn new(
        conn_id: impl Into<String>,
        kind: ObjectKind,
        catalog: impl Into<String>,
        schema: impl Into<String>,
        parent: impl Into<String>,
        name: impl Into<String>,
    ) -> Self {
        Self {
            conn_id: conn_id.into(),
            kind,
            catalog: catalog.into(),
            schema: schema.into(),
            parent: parent.into(),
            name: name.into(),
        }
    }

    /// 表引用。
    pub fn table(
        conn_id: impl Into<String>,
        catalog: impl Into<String>,
        schema: impl Into<String>,
        table: impl Into<String>,
    ) -> Self {
        Self::new(conn_id, ObjectKind::Table, catalog, schema, "", table)
    }

    /// 视图引用。
    pub fn view(
        conn_id: impl Into<String>,
        catalog: impl Into<String>,
        schema: impl Into<String>,
        view: impl Into<String>,
    ) -> Self {
        Self::new(conn_id, ObjectKind::View, catalog, schema, "", view)
    }

    /// Catalog（库）引用。
    ///
    /// 容器引用的 `name` 与末段同名（库 → `catalog`、模式 → `schema`），
    /// 这样 [`Self::key`] / [`Self::dotted`] 与导航树节点 key 同构。
    pub fn catalog(conn_id: impl Into<String>, catalog: impl Into<String>) -> Self {
        let catalog = catalog.into();
        Self::new(
            conn_id,
            ObjectKind::Catalog,
            catalog.clone(),
            "",
            "",
            catalog,
        )
    }

    /// Schema 引用（结构洞察的入口参数）。
    pub fn schema(
        conn_id: impl Into<String>,
        catalog: impl Into<String>,
        schema: impl Into<String>,
    ) -> Self {
        // schema 这一层的名字就是 schema 名（与 `NavNode::child_key(conn, [catalog, schema])` 同形）。
        let schema = schema.into();
        Self::new(
            conn_id,
            ObjectKind::Schema,
            catalog,
            schema.clone(),
            "",
            schema,
        )
    }

    /// 列引用（`parent` = 所属表 / 视图名）。
    pub fn column(
        conn_id: impl Into<String>,
        catalog: impl Into<String>,
        schema: impl Into<String>,
        table: impl Into<String>,
        column: impl Into<String>,
    ) -> Self {
        Self::new(conn_id, ObjectKind::Column, catalog, schema, table, column)
    }

    /// 例程引用（存储过程 / 函数）。
    pub fn routine(
        conn_id: impl Into<String>,
        catalog: impl Into<String>,
        schema: impl Into<String>,
        routine: impl Into<String>,
    ) -> Self {
        Self::new(conn_id, ObjectKind::Routine, catalog, schema, "", routine)
    }

    /// 序列引用。
    pub fn sequence(
        conn_id: impl Into<String>,
        catalog: impl Into<String>,
        schema: impl Into<String>,
        sequence: impl Into<String>,
    ) -> Self {
        Self::new(conn_id, ObjectKind::Sequence, catalog, schema, "", sequence)
    }

    /// 触发器引用。
    ///
    /// `table` = 触发器挂在的那张表（部分驱动需要它才能定位；无归属时传空串）。
    pub fn trigger(
        conn_id: impl Into<String>,
        catalog: impl Into<String>,
        schema: impl Into<String>,
        table: impl Into<String>,
        trigger: impl Into<String>,
    ) -> Self {
        Self::new(conn_id, ObjectKind::Trigger, catalog, schema, table, trigger)
    }

    /// 从一次索引搜索命中还原引用（未知类别返回 `None`）。
    ///
    /// 这是「搜索」与「导航树 / 属性面板」之间的唯一对接口：命中的类别串、
    /// 名字、父对象、catalog / schema 全部来自 `IndexSearchHit`（已联结 `schemata`）。
    ///
    /// 容器命中（库 / 模式）在联结不到行、`catalog` / `schema` 为 NULL 时，
    /// 用 `name` 补齐自指段——否则会得到只剩连接 id 的空引用。
    pub fn from_index_hit(
        conn_id: &str,
        object_type: &str,
        name: &str,
        parent: Option<&str>,
        catalog: Option<&str>,
        schema: Option<&str>,
    ) -> Option<Self> {
        let kind = ObjectKind::parse_index_str(object_type)?;
        let mut catalog = catalog.unwrap_or_default().to_string();
        let mut schema = schema.unwrap_or_default().to_string();
        match kind {
            ObjectKind::Catalog if catalog.is_empty() => catalog = name.to_string(),
            ObjectKind::Schema if schema.is_empty() => schema = name.to_string(),
            _ => {}
        }
        Some(Self::new(
            conn_id,
            kind,
            catalog,
            schema,
            parent.unwrap_or_default(),
            name,
        ))
    }

    /// 归一化的段位序列：`[catalog, schema, parent, name]`，空段略去。
    ///
    /// **容器去重**：库 / 模式的 `name` 与末段重复（见 [`Self::catalog`] / [`Self::schema`]），
    /// 此处只输出一次——导航树的容器节点 key 也是这么拼的（`child_key(conn, [catalog, 模式名])`），
    /// 所以「库 / 模式引用」与树节点不会因重复段而对不上。
    fn segments(&self) -> Vec<&str> {
        let mut parts: Vec<&str> = Vec::with_capacity(4);
        for segment in [&self.catalog, &self.schema, &self.parent] {
            if !segment.is_empty() {
                parts.push(segment);
            }
        }
        let name_is_container = matches!(self.kind, ObjectKind::Catalog | ObjectKind::Schema);
        if !name_is_container && !self.name.is_empty() {
            parts.push(&self.name);
        }
        parts
    }

    /// 规范化 key：`{conn}/{catalog}/{schema}/{parent}/{name}`，空段跳过。
    ///
    /// 与导航树的 `NavNode::child_key(conn_id, &[catalog, schema, parent, name])` 同构
    /// （该同构由 `database` 侧单测钉住）——所以「搜索结果行」与「树上那一行」可以拿它
    /// 判断是不是同一个对象。跳过空段是为了让无 Catalog / 无 Schema 层的驱动也得到干净
    /// 的 key，而不是出现 `//`。
    pub fn key(&self) -> String {
        let segments = self.segments();
        if segments.is_empty() {
            return self.conn_id.clone();
        }
        let mut out = String::with_capacity(
            self.conn_id.len() + segments.iter().map(|s| s.len() + 1).sum::<usize>(),
        );
        out.push_str(&self.conn_id);
        for segment in segments {
            out.push('/');
            out.push_str(segment);
        }
        out
    }

    /// 点分限定名（空段略去）：`catalog.schema.table` / `schema.table` / `table`。
    ///
    /// **不带引号**：各方言的引号规则由宿主按驱动决定（见 `workbench::Shared::insight_sample_sql`），
    /// 这里只用于展示文案与快捷判断。
    pub fn dotted(&self) -> String {
        self.segments().join(".")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 索引串 ↔ 类别的往返；未知串不猜。
    #[test]
    fn kind_index_str_roundtrip() {
        for kind in [
            ObjectKind::Catalog,
            ObjectKind::Schema,
            ObjectKind::Table,
            ObjectKind::View,
            ObjectKind::Column,
            ObjectKind::Routine,
            ObjectKind::Sequence,
            ObjectKind::Trigger,
        ] {
            assert_eq!(
                ObjectKind::parse_index_str(kind.as_index_str()),
                Some(kind),
                "{kind:?} 往返失败"
            );
        }
        // 驱动 / 旧账本里可能出现的同义串
        assert_eq!(
            ObjectKind::parse_index_str("procedure"),
            Some(ObjectKind::Routine)
        );
        assert_eq!(
            ObjectKind::parse_index_str("function"),
            Some(ObjectKind::Routine)
        );
        // 索引里出现但不可寻址的类别（索引 / 键）：不猜，交由调用方兜底
        assert_eq!(ObjectKind::parse_index_str("index"), None);
        assert_eq!(ObjectKind::parse_index_str("primary_key"), None);
        assert_eq!(ObjectKind::parse_index_str(""), None);
    }

    /// 界面标签：每类都有非空中文标签，且容器只有库 / 模式。
    #[test]
    fn kind_labels_and_containers() {
        for kind in [
            ObjectKind::Catalog,
            ObjectKind::Schema,
            ObjectKind::Table,
            ObjectKind::View,
            ObjectKind::Column,
            ObjectKind::Routine,
            ObjectKind::Sequence,
            ObjectKind::Trigger,
        ] {
            assert!(!kind.label().is_empty(), "{kind:?} 缺标签");
        }
        assert!(ObjectKind::Catalog.is_container());
        assert!(ObjectKind::Schema.is_container());
        assert!(!ObjectKind::Table.is_container());
        assert!(!ObjectKind::Column.is_container());
    }

    /// 构造器填的段位正确（列带父对象，表 / 视图 / 例程不带）。
    #[test]
    fn constructors_shape_the_segments() {
        let t = ObjectRef::table("P_1", "shop", "public", "orders");
        assert_eq!(t.kind, ObjectKind::Table);
        assert_eq!((t.catalog.as_str(), t.schema.as_str()), ("shop", "public"));
        assert_eq!(t.parent, "", "表没有父对象");
        assert_eq!(t.name, "orders");

        let c = ObjectRef::column("P_1", "shop", "public", "orders", "order_id");
        assert_eq!(c.kind, ObjectKind::Column);
        assert_eq!(c.parent, "orders");

        let s = ObjectRef::schema("G_1", "main", "public");
        assert_eq!(s.kind, ObjectKind::Schema);
        assert_eq!(s.name, "public", "schema 引用的名字就是 schema 名");

        // 视图与表的区别只在 `kind`（同一个限定名下两者可能共存，键也各归各的）
        let v = ObjectRef::view("P_1", "shop", "public", "orders");
        assert_eq!(v.kind, ObjectKind::View);
        assert_eq!(v.key(), t.key(), "限定名相同的表与视图 key 相同——区分靠 kind");

        // 其余可寻址对象：例程 / 序列 / 触发器
        let r = ObjectRef::routine("P_1", "shop", "public", "fn_total");
        assert_eq!(r.kind, ObjectKind::Routine);
        assert_eq!(r.key(), "P_1/shop/public/fn_total");
        let seq = ObjectRef::sequence("P_1", "shop", "public", "seq_order");
        assert_eq!(seq.kind, ObjectKind::Sequence);
        let trg = ObjectRef::trigger("P_1", "shop", "public", "orders", "trg_audit");
        assert_eq!(trg.kind, ObjectKind::Trigger);
        assert_eq!(
            trg.parent, "orders",
            "触发器带所属表（部分驱动定位需要）"
        );
        assert_eq!(trg.key(), "P_1/shop/public/orders/trg_audit");
    }

    /// key：空段跳过，段序固定为 conn / catalog / schema / parent / name；容器不重复末段。
    #[test]
    fn key_skips_empty_segments() {
        assert_eq!(
            ObjectRef::table("P_1", "shop", "public", "orders").key(),
            "P_1/shop/public/orders"
        );
        assert_eq!(
            ObjectRef::column("P_1", "shop", "public", "orders", "order_id").key(),
            "P_1/shop/public/orders/order_id"
        );
        // 无 Catalog 层（MySQL 侧库即 schema：schema 段仍非空，catalog 空则跳过）
        assert_eq!(
            ObjectRef::table("G_1", "", "mall", "order").key(),
            "G_1/mall/order"
        );
        // 容器：名字就是末段，不重复输出
        assert_eq!(
            ObjectRef::schema("G_1", "shop", "public").key(),
            "G_1/shop/public",
            "模式引用的 key 要与树上那个模式节点一致"
        );
        assert_eq!(ObjectRef::catalog("G_1", "main").key(), "G_1/main");
        // 全部空段只剩连接 id
        assert_eq!(ObjectRef::schema("G_1", "", "").key(), "G_1");
    }

    /// 索引命中 → 引用：类别、父对象、catalog / schema 全部带过来；未知类别不产出引用。
    #[test]
    fn refs_from_index_hits() {
        let table =
            ObjectRef::from_index_hit("P_1", "table", "orders", None, Some("shop"), Some("public"))
                .expect("表命中应可还原");
        assert_eq!(table.kind, ObjectKind::Table);
        assert_eq!(table.key(), "P_1/shop/public/orders");

        let column = ObjectRef::from_index_hit(
            "P_1",
            "column",
            "order_id",
            Some("orders"),
            Some("shop"),
            Some("public"),
        )
        .expect("列命中应可还原");
        assert_eq!(column.kind, ObjectKind::Column);
        assert_eq!(column.parent, "orders");

        // NULL 段收敛成空串，并用 name 补齐容器自指段
        // （索引的 schema 命中：`object_name` 与 `schema_name` 本就是同一个名字）
        let bare = ObjectRef::from_index_hit("G_1", "schema", "main", None, None, None)
            .expect("schema 命中应可还原");
        assert_eq!(bare.key(), "G_1/main");

        assert!(ObjectRef::from_index_hit("P_1", "index", "idx_a", None, None, None).is_none());
    }

    /// 点分限定名：空段略去，容器不重复末段，用于展示文案。
    #[test]
    fn dotted_name_skips_empty_segments() {
        assert_eq!(
            ObjectRef::table("P_1", "shop", "public", "orders").dotted(),
            "shop.public.orders"
        );
        assert_eq!(
            ObjectRef::table("P_1", "", "public", "orders").dotted(),
            "public.orders"
        );
        assert_eq!(
            ObjectRef::column("P_1", "", "", "orders", "order_id").dotted(),
            "orders.order_id"
        );
        assert_eq!(
            ObjectRef::schema("G_1", "shop", "public").dotted(),
            "shop.public"
        );
    }
}
