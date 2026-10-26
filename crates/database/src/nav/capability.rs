//! L2 操作能力矩阵（方案稿 §4.3 / D2，B2）：动作全集 + 位集 + 族 × 类型静态规则。
//!
//! 分层边界（2026-10-25 拍板 Q1-A / Q2-A / Q3-A）：
//! - 本模块只承载**静态规则与纯函数**，不持连接状态、不发 I/O、不向 engine 新增 trait；
//! - 连接态（`connected` 前置）由视图层门控纯函数传入；运行时实测位（只读 / 版本差异）
//!   后续批次经 `MetadataService` 桥接既有 `supports_*` 通道，本批不接；
//! - 「动作是否出现在菜单」的类型适用性归 B4 动作注册表 `applies`；本模块只回答
//!   「已出现的动作在当前族 × 类型下是否被声明支持」。
//!
//! 新增数据库族：静态规则自动随 L1 档案类型全集派生，视图层零改动。

use engine::ObjectKind;

use crate::model::NavNodeKind;
use crate::nav::family_profile::registry;

/// 导航动作全集。
///
/// 分组仅用于文档归类；门控按位集逐项判定，不拆子枚举（拍板 Q2-A）。
/// **顺序即位索引**：`OpSet` 按变体序号取位，新增变体只能追加到末尾，禁止插入中间。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NavOp {
    // ---- 本地：剪贴板 / 属性 / 面板导航 / 编辑器空查询壳，断连可用 ----
    ShowProperties,
    CopyName,
    CopyQualified,
    OpenSql,
    InsightPanel,
    // ---- 回源库：读取 / 分析，需已连接 ----
    ViewData,
    GenDml,
    RefreshProps,
    MockData,
    Stats,
    InsightSchema,
    // ---- B6 破坏性（预留位；本批不注册动作、不挂菜单，D22 / D23） ----
    Rename,
    Drop,
    Truncate,
    // ---- B5 / 后续预留（暂不消费） ----
    CreateObject,
    SetActive,
    SearchChildren,
    Description,
    Columns,
    ObjectSource,
    RoutineSource,
}

impl NavOp {
    /// 是否本地动作（不回源库，断连可用；与现状 `nav_obj_menu_enabled` 本地清单等价）。
    pub fn is_local(self) -> bool {
        matches!(
            self,
            NavOp::ShowProperties
                | NavOp::CopyName
                | NavOp::CopyQualified
                | NavOp::OpenSql
                | NavOp::InsightPanel
        )
    }

    /// 位索引（变体序号 → u128 位；新增变体只能追加末尾）。
    fn bit(self) -> u128 {
        1 << (self as u8)
    }
}

/// 动作位集（族 × 类型静态能力集；可整集传参、纯函数可比较）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OpSet(u128);

impl OpSet {
    /// 空能力集。
    pub const EMPTY: Self = Self(0);

    /// 纳入一个动作位。
    pub fn add(mut self, op: NavOp) -> Self {
        self.0 |= op.bit();
        self
    }

    /// 是否声明支持指定动作。
    pub fn has(self, op: NavOp) -> bool {
        self.0 & op.bit() != 0
    }

    /// 子集判定。
    pub fn contains(self, other: OpSet) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// 展示节点 → 可寻址身份类别（`NavNodeKind` → `ObjectKind`）。
///
/// 连接根 / 类别文件夹 / 列溢出提示节点不属于可寻址对象，返回 `None`；
/// 与 `primitives.rs` 图标分支内局部 match 保持等价，此处独立成纯函数供能力矩阵复用。
pub fn nav_node_object_kind(kind: &NavNodeKind) -> Option<ObjectKind> {
    match kind {
        NavNodeKind::Table { .. } => Some(ObjectKind::Table),
        NavNodeKind::View => Some(ObjectKind::View),
        NavNodeKind::Routine { .. } => Some(ObjectKind::Routine),
        NavNodeKind::Sequence => Some(ObjectKind::Sequence),
        NavNodeKind::Trigger => Some(ObjectKind::Trigger),
        NavNodeKind::Column { .. } => Some(ObjectKind::Column),
        NavNodeKind::Catalog => Some(ObjectKind::Catalog),
        NavNodeKind::Schema => Some(ObjectKind::Schema),
        NavNodeKind::Connection { .. }
        | NavNodeKind::Folder(_)
        | NavNodeKind::ColumnOverflow => None,
    }
}

/// 族 × 类型静态操作集（随 L1 档案类型全集派生；档案未声明类型返回空集）。
///
/// 类型适用性规则与现状 `rows.rs` 菜单条件对齐（B4 迁注册表 `applies` 时同源搬移）：
///
/// | 动作 | 适用类型 |
/// | --- | --- |
/// | CopyName / OpenSql / InsightPanel | 档案声明的全部可寻址类型 |
/// | ShowProperties | 档案声明的全部可寻址类型 |
/// | CopyQualified | Table / View / Routine / Sequence / Trigger |
/// | ViewData / MockData / Stats | Table / View |
/// | GenDml | Table |
/// | RefreshProps | Table / View / Schema（B2 门控暂不消费，B4 接入容器刷新判定） |
/// | InsightSchema | Table / View / Schema |
///
/// 回源库动作的 `connected` 前置由门控纯函数判定，不在本静态集内表达。
pub fn family_ops(family_id: &str, kind: Option<ObjectKind>) -> OpSet {
    let Some(kind) = kind else {
        return OpSet::EMPTY;
    };
    // 档案未声明该类型 → 族不认识，不给动作位（安全网；正常路径该类型根本进不了树）。
    if registry().profile_of(family_id, kind).is_none() {
        return OpSet::EMPTY;
    }

    // 本地动作：档案声明类型即开放（出现条件由 B4 `applies` 复核）。
    let mut ops = OpSet::EMPTY
        .add(NavOp::ShowProperties)
        .add(NavOp::CopyName)
        .add(NavOp::OpenSql)
        .add(NavOp::InsightPanel);

    match kind {
        ObjectKind::Table => {
            ops = ops
                .add(NavOp::CopyQualified)
                .add(NavOp::ViewData)
                .add(NavOp::MockData)
                .add(NavOp::Stats)
                .add(NavOp::InsightSchema)
                // B6 破坏性：表可删除、可清空（族档案未声明表的族在此之前已被安全网拦截）。
                .add(NavOp::Drop)
                .add(NavOp::Truncate);
        }
        ObjectKind::View => {
            ops = ops
                .add(NavOp::CopyQualified)
                .add(NavOp::ViewData)
                .add(NavOp::MockData)
                .add(NavOp::Stats)
                .add(NavOp::InsightSchema)
                // 视图可删除；清空视图无意义，不挂 Truncate（§4.3 类型适用矩阵据此钉死）。
                .add(NavOp::Drop);
        }
        ObjectKind::Routine | ObjectKind::Sequence | ObjectKind::Trigger => {
            ops = ops.add(NavOp::CopyQualified);
            // 仅序列可删除；例程 / 触发器保持仅复制限定名（两位置均不出现破坏性位，§7.1 挂账依据）。
            if kind == ObjectKind::Sequence {
                ops = ops.add(NavOp::Drop);
            }
        }
        _ => {}
    }

    if kind == ObjectKind::Table {
        ops = ops.add(NavOp::GenDml);
    }

    if matches!(kind, ObjectKind::Table | ObjectKind::View | ObjectKind::Schema) {
        ops = ops.add(NavOp::RefreshProps).add(NavOp::InsightSchema);
    }

    ops
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::NavFolder;
    use crate::model::NavNodeKind;

    // ---- 位集基础 ----

    #[test]
    fn opset_add_has_and_empty() {
        let set = OpSet::EMPTY.add(NavOp::ViewData).add(NavOp::Stats);
        assert!(set.has(NavOp::ViewData));
        assert!(set.has(NavOp::Stats));
        assert!(!set.has(NavOp::GenDml));
        assert!(!OpSet::EMPTY.has(NavOp::CopyName));
        assert!(OpSet::EMPTY.is_empty());
        assert!(!set.is_empty());
    }

    #[test]
    fn opset_subset() {
        let parent = OpSet::EMPTY
            .add(NavOp::CopyName)
            .add(NavOp::CopyQualified);
        let child = OpSet::EMPTY.add(NavOp::CopyName);
        assert!(parent.contains(child));
        assert!(!child.contains(parent));
    }

    // ---- 本地动作分类 ----

    #[test]
    fn local_ops_match_legacy_local_list() {
        for op in [
            NavOp::ShowProperties,
            NavOp::CopyName,
            NavOp::CopyQualified,
            NavOp::OpenSql,
            NavOp::InsightPanel,
        ] {
            assert!(op.is_local(), "{op:?} 应为本地动作");
        }
        for op in [
            NavOp::ViewData,
            NavOp::GenDml,
            NavOp::RefreshProps,
            NavOp::MockData,
            NavOp::Stats,
            NavOp::InsightSchema,
        ] {
            assert!(!op.is_local(), "{op:?} 应为回源库动作");
        }
    }

    // ---- 展示节点 → 身份类别 ----

    #[test]
    fn node_kind_maps_to_addressable_objects_only() {
        let cases = [
            (NavNodeKind::Table { row_estimate: None }, Some(ObjectKind::Table)),
            (NavNodeKind::View, Some(ObjectKind::View)),
            (
                NavNodeKind::Routine {
                    routine_type: "PROCEDURE".to_string(),
                },
                Some(ObjectKind::Routine),
            ),
            (NavNodeKind::Sequence, Some(ObjectKind::Sequence)),
            (NavNodeKind::Trigger, Some(ObjectKind::Trigger)),
            (
                NavNodeKind::Column {
                    data_type: "int".to_string(),
                    nullable: true,
                    primary: false,
                    foreign: false,
                },
                Some(ObjectKind::Column),
            ),
            (NavNodeKind::Catalog, Some(ObjectKind::Catalog)),
            (NavNodeKind::Schema, Some(ObjectKind::Schema)),
        ];
        for (kind, expected) in cases {
            assert_eq!(nav_node_object_kind(&kind), expected);
        }
        for kind in [
            NavNodeKind::Folder(NavFolder::Tables),
            NavNodeKind::ColumnOverflow,
        ] {
            assert_eq!(nav_node_object_kind(&kind), None);
        }
    }

    // ---- 族 × 类型静态规则（与现状菜单条件等价） ----

    #[test]
    fn table_opset_covers_data_and_dml_and_analysis() {
        let ops = family_ops("postgresql", Some(ObjectKind::Table));
        for op in [
            NavOp::ViewData,
            NavOp::GenDml,
            NavOp::MockData,
            NavOp::Stats,
            NavOp::InsightSchema,
            NavOp::RefreshProps,
            NavOp::CopyName,
            NavOp::CopyQualified,
            NavOp::ShowProperties,
            NavOp::OpenSql,
            NavOp::InsightPanel,
        ] {
            assert!(ops.has(op), "表应声明 {op:?}");
        }
    }

    #[test]
    fn view_opset_covers_data_and_analysis_but_not_dml() {
        let ops = family_ops("postgresql", Some(ObjectKind::View));
        for op in [NavOp::ViewData, NavOp::MockData, NavOp::Stats, NavOp::InsightSchema] {
            assert!(ops.has(op), "视图应声明 {op:?}");
        }
        assert!(!ops.has(NavOp::GenDml), "视图不生成 DML 模板");
    }

    #[test]
    fn routine_sequence_trigger_keep_copy_only_on_source() {
        for kind in [ObjectKind::Routine, ObjectKind::Sequence, ObjectKind::Trigger] {
            let ops = family_ops("postgresql", Some(kind));
            assert!(ops.has(NavOp::CopyQualified), "{kind:?} 应可复制限定名");
            assert!(ops.has(NavOp::CopyName));
            for op in [NavOp::ViewData, NavOp::GenDml, NavOp::MockData, NavOp::Stats] {
                assert!(!ops.has(op), "{kind:?} 不应声明 {op:?}");
            }
        }
    }

    /// B6 破坏性能力位类型矩阵（§4.3）：表 Drop+Truncate；视图仅 Drop；序列仅 Drop；
    /// 例程 / 触发器不挂破坏性位；族档案未声明的类型由安全网返回空集。
    #[test]
    fn destructive_ops_follow_type_matrix() {
        // PG 表：删除 + 清空都声明。
        let table = family_ops("postgresql", Some(ObjectKind::Table));
        assert!(table.has(NavOp::Drop), "表应声明 Drop");
        assert!(table.has(NavOp::Truncate), "表应声明 Truncate");
        // PG 视图：仅 Drop，不挂 Truncate。
        let view = family_ops("postgresql", Some(ObjectKind::View));
        assert!(view.has(NavOp::Drop), "视图应声明 Drop");
        assert!(!view.has(NavOp::Truncate), "视图不挂清空");
        // PG 序列：仅 Drop，不挂 Truncate。
        let sequence = family_ops("postgresql", Some(ObjectKind::Sequence));
        assert!(sequence.has(NavOp::Drop), "序列应声明 Drop");
        assert!(!sequence.has(NavOp::Truncate), "序列不挂清空");
        // PG 例程 / 触发器：两个破坏性位都不出现。
        for kind in [ObjectKind::Routine, ObjectKind::Trigger] {
            let ops = family_ops("postgresql", Some(kind));
            assert!(!ops.has(NavOp::Drop), "{kind:?} 不应声明 Drop");
            assert!(!ops.has(NavOp::Truncate), "{kind:?} 不应声明 Truncate");
        }
        // MySQL 档案不声明序列：安全网空集，破坏性位自然不存在。
        let mysql_sequence = family_ops("mysql", Some(ObjectKind::Sequence));
        assert!(mysql_sequence.is_empty());
        assert!(!mysql_sequence.has(NavOp::Drop));
        assert!(!mysql_sequence.has(NavOp::Truncate));
    }

    #[test]
    fn schema_opset_covers_insight_schema_only() {
        let ops = family_ops("postgresql", Some(ObjectKind::Schema));
        assert!(ops.has(NavOp::InsightSchema));
        assert!(ops.has(NavOp::RefreshProps));
        for op in [NavOp::ViewData, NavOp::GenDml, NavOp::MockData, NavOp::Stats] {
            assert!(!ops.has(op));
        }
    }

    #[test]
    fn undeclared_family_type_gets_empty_ops() {
        // MySQL 档案不声明序列；未知族回退档案声明全量。
        assert!(family_ops("mysql", Some(ObjectKind::Sequence)).is_empty());
        assert!(family_ops("mysql", Some(ObjectKind::Table)).has(NavOp::ViewData));
        assert!(!family_ops("oracle", Some(ObjectKind::Table)).is_empty());
    }

    #[test]
    fn non_addressable_node_yields_empty_set() {
        assert!(family_ops("postgresql", None).is_empty());
    }
}
