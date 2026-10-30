//! L4 动作注册表实现：命令 id 枚举 + 静态描述符登记 + 类型过滤 / 门控 / 分组排序纯函数。

use std::sync::OnceLock;

use engine::ObjectKind;

use crate::nav::capability::{NavOp, OpSet};

/// 动作命令 id（底座层分派键；决策 A：注册表不挂回调，视图层按本枚举 match 到现有处理器）。
///
/// 变体即注册表登记项；新增动作先在此追加变体，再补一条 [`NavActionDescriptor`]。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NavActionId {
    /// 查看属性（L2 缓存可渲染，断连可用）。
    ShowProperties,
    /// 查看数据（LIMIT 200）——回源库执行查询。
    ViewData,
    /// 在 SQL 编辑器中打开（空查询壳，断连可用）。
    OpenSql,
    /// 查看洞察——**统一洞察入口**（2026-10-30 用户拍板去重）：
    /// 表 / 视图 → 取样画像；schema → 结构洞察；其余节点 → 纯面板导航。
    InsightPanel,
    /// 生成 SQL ▸——DML 模板子菜单（INSERT / UPDATE / DELETE）。
    GenDml,
    /// 生成 Mock 数据——读源库结构。
    MockData,
    /// 复制名称——本地剪贴板。
    CopyName,
    /// 复制限定名——本地剪贴板。
    CopyQualified,
    /// 刷新元数据——容器刷新，连接前置但不按能力集收窄。
    RefreshProps,
    /// 删除对象（B6 staged 缓冲：表 / 视图 / 序列）。
    DeleteObject,
    /// 清空表（B6 staged 缓冲：仅表，不删对象本身）。
    EmptyTable,
}

/// 动作产出的靶类型（文档性声明；靶构造函数维持 `nav_view/primitives.rs` 现状，B4 不迁）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetKind {
    /// 纯本地动作，无对象靶（剪贴板 / 面板导航 / 空查询壳 / 刷新）。
    None,
    /// 属性面板请求（`PropertyRequest`，取自节点 `property`）。
    PropertyRef,
    /// 表 / 视图可寻址靶（查看数据 / DML / Mock / 洞察取样）。
    TableOnly,
    /// 限定名靶（复制限定名）。
    QualifiedName,
    /// schema 级引用靶（洞察分派到结构洞察）。
    SchemaTarget,
}

/// 菜单分组（渲染分段 + 分隔线；枚举序即分组渲染顺序）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ActionGroup {
    /// 查看 / 工作区入口：属性、查看数据、打开 SQL、查看洞察。
    Primary,
    /// 生成 / 分析：生成 SQL、Mock 数据。
    Generate,
    /// 对象本地操作：复制名称、复制限定名、刷新。
    Object,
    /// 删除 / 清空（B6 staged 缓冲；rename 仍预留，不入本批）。
    Manage,
}

/// 动作描述符（纯数据；不含回调、不含视图句柄）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NavActionDescriptor {
    /// 分派命令 id（视图层 match 键）。
    pub id: NavActionId,
    /// 稳定命令 id 串（测试断言 / 将来命令清单与快捷键同源用）。
    pub slug: &'static str,
    /// 菜单文案。
    pub label: &'static str,
    /// 所属分组。
    pub group: ActionGroup,
    /// 组内排序（小在前）。
    pub order: u16,
    /// 适用类型白名单；空切片 = 全部可寻址类型适用。
    pub applies: &'static [ObjectKind],
    /// 依赖的 L2 能力位；`None` = 不查能力集（`RefreshProps` 特例）。
    pub op: Option<NavOp>,
    /// 靶类型声明。
    pub target: TargetKind,
    /// 快捷键 GPUI Action id（可选；仅元数据声明，绑定注册仍集中 `app/main.rs`）。
    pub shortcut: Option<&'static str>,
    /// 是否破坏性动作（B6 入 staged 缓冲；仅 DeleteObject / EmptyTable 为 true，其余 false）。
    pub destructive: bool,
}

/// 装配结果：描述符引用 + 门控判定（视图层渲染只消费本结构，不另算门控）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedEntry {
    /// 描述符静态引用。
    pub descriptor: &'static NavActionDescriptor,
    /// 当前上下文是否可用（`false` = 菜单置灰，不隐藏）。
    pub enabled: bool,
}

/// 动作注册表：静态描述符集合 + 过滤 / 门控 / 分组排序纯函数。
pub struct NavActionRegistry {
    entries: &'static [NavActionDescriptor],
}

impl NavActionRegistry {
    /// 按节点身份、连接态、能力集装配当前上下文的动作序列（分组分段、组内 order 排序）。
    ///
    /// 过滤两层职责互不混淆：
    /// - `applies`：类型适用性（注册表静态白名单）；
    /// - 靶可用性（`property` / `qualified` / 路径靶是否 `Some`）属视图层运行时判定，
    ///   不在本函数内表达。
    pub fn entries_for(
        &self,
        kind: Option<ObjectKind>,
        connected: bool,
        ops: &OpSet,
    ) -> Vec<ResolvedEntry> {
        let mut resolved: Vec<ResolvedEntry> = self
            .entries
            .iter()
            .filter(|d| applies_to(d, kind))
            .map(|d| ResolvedEntry {
                descriptor: d,
                enabled: gate(d.op, connected, ops),
            })
            .collect();
        // 分组序 → 组内 order 稳定排序；同组同序按注册表书写顺序兜底（描述符静态数组顺序）。
        resolved.sort_by(|a, b| {
            a.descriptor
                .group
                .cmp(&b.descriptor.group)
                .then(a.descriptor.order.cmp(&b.descriptor.order))
        });
        resolved
    }

    /// 注册表全部描述符（测试 / 命令清单同源导出用）。
    pub fn all(&self) -> &'static [NavActionDescriptor] {
        self.entries
    }
}

/// 类型适用性：`applies` 空切片 = 全部可寻址类型；`None`（连接根 / 类别文件夹）只放行
/// 通用动作，类型敏感动作不出现。
fn applies_to(d: &NavActionDescriptor, kind: Option<ObjectKind>) -> bool {
    if d.applies.is_empty() {
        return true;
    }
    matches!(kind, Some(k) if d.applies.contains(&k))
}

/// 门控判定（与现状 `nav_obj_menu_enabled` 等价）：
/// - `op = None`：连接前置，不查能力集（刷新特例）；
/// - 本地动作（`op.is_local()`）：断连可用，不查连接、不查能力集；
/// - 回源动作：连接前置 + 当前族 × 类型能力集声明支持。
fn gate(op: Option<NavOp>, connected: bool, ops: &OpSet) -> bool {
    match op {
        None => connected,
        Some(op) if op.is_local() => true,
        Some(op) => connected && ops.has(op),
    }
}

/// 静态登记表（B4 对象菜单十一条目；B6 追加 DeleteObject / EmptyTable 两条 Manage 破坏性动作，共十三条）。
static ENTRIES: &[NavActionDescriptor] = &[
    NavActionDescriptor {
        id: NavActionId::OpenSql,
        slug: "nav.open-sql",
        label: "在 SQL 编辑器中打开",
        group: ActionGroup::Primary,
        order: 0,
        applies: &[],
        op: Some(NavOp::OpenSql),
        target: TargetKind::None,
        shortcut: None,
        destructive: false,
    },
    NavActionDescriptor {
        id: NavActionId::ShowProperties,
        slug: "nav.show-properties",
        label: "查看属性",
        group: ActionGroup::Primary,
        order: 1,
        applies: &[],
        op: Some(NavOp::ShowProperties),
        target: TargetKind::PropertyRef,
        shortcut: Some("NavOpenProperties"),
        destructive: false,
    },
    NavActionDescriptor {
        id: NavActionId::ViewData,
        slug: "nav.view-data",
        label: "查看数据（LIMIT 200）",
        group: ActionGroup::Primary,
        order: 2,
        applies: &[ObjectKind::Table, ObjectKind::View],
        op: Some(NavOp::ViewData),
        target: TargetKind::TableOnly,
        shortcut: None,
        destructive: false,
    },
    NavActionDescriptor {
        id: NavActionId::InsightPanel,
        slug: "nav.insight-panel",
        label: "查看洞察",
        group: ActionGroup::Primary,
        order: 3,
        applies: &[],
        // 门控保持「连接级面板可用」；按节点分派取样 / 结构洞察在视图层按靶可用性执行
        // （2026-10-30 去重：原「查看统计 / 结构洞察」两条已并入本入口）。
        op: Some(NavOp::InsightPanel),
        target: TargetKind::None,
        shortcut: None,
        destructive: false,
    },
    NavActionDescriptor {
        id: NavActionId::GenDml,
        slug: "nav.gen-sql",
        label: "生成 SQL",
        group: ActionGroup::Generate,
        order: 0,
        applies: &[ObjectKind::Table],
        op: Some(NavOp::GenDml),
        target: TargetKind::TableOnly,
        shortcut: None,
        destructive: false,
    },
    NavActionDescriptor {
        id: NavActionId::MockData,
        slug: "nav.mock-data",
        label: "生成 Mock 数据",
        group: ActionGroup::Generate,
        order: 1,
        applies: &[ObjectKind::Table, ObjectKind::View],
        op: Some(NavOp::MockData),
        target: TargetKind::TableOnly,
        shortcut: None,
        destructive: false,
    },
    NavActionDescriptor {
        id: NavActionId::CopyName,
        slug: "nav.copy-name",
        label: "复制名称",
        group: ActionGroup::Object,
        order: 0,
        applies: &[],
        op: Some(NavOp::CopyName),
        target: TargetKind::None,
        shortcut: None,
        destructive: false,
    },
    NavActionDescriptor {
        id: NavActionId::CopyQualified,
        slug: "nav.copy-qualified",
        label: "复制限定名",
        group: ActionGroup::Object,
        order: 1,
        applies: &[
            ObjectKind::Table,
            ObjectKind::View,
            ObjectKind::Routine,
            ObjectKind::Sequence,
            ObjectKind::Trigger,
        ],
        op: Some(NavOp::CopyQualified),
        target: TargetKind::QualifiedName,
        shortcut: None,
        destructive: false,
    },
    NavActionDescriptor {
        id: NavActionId::RefreshProps,
        slug: "nav.refresh",
        label: "刷新元数据",
        group: ActionGroup::Object,
        order: 2,
        applies: &[ObjectKind::Table, ObjectKind::View, ObjectKind::Schema],
        op: None,
        target: TargetKind::None,
        shortcut: None,
        destructive: false,
    },
    NavActionDescriptor {
        id: NavActionId::DeleteObject,
        slug: "nav.delete-object",
        label: "删除…",
        group: ActionGroup::Manage,
        order: 0,
        applies: &[ObjectKind::Table, ObjectKind::View, ObjectKind::Sequence],
        op: Some(NavOp::Drop),
        target: TargetKind::PropertyRef,
        shortcut: None,
        destructive: true,
    },
    NavActionDescriptor {
        id: NavActionId::EmptyTable,
        slug: "nav.empty-table",
        label: "清空…",
        group: ActionGroup::Manage,
        order: 1,
        applies: &[ObjectKind::Table],
        op: Some(NavOp::Truncate),
        target: TargetKind::PropertyRef,
        shortcut: None,
        destructive: true,
    },
];

/// 全局动作注册表（静态登记表，无需运行时注册；预留 `register` 供 B6 追加破坏性动作）。
pub fn registry() -> &'static NavActionRegistry {
    static REG: OnceLock<NavActionRegistry> = OnceLock::new();
    REG.get_or_init(|| NavActionRegistry { entries: ENTRIES })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nav::capability::family_ops;

    fn ids(entries: &[ResolvedEntry]) -> Vec<NavActionId> {
        entries.iter().map(|e| e.descriptor.id).collect()
    }

    #[test]
    fn registry_covers_all_action_ids() {
        // 登记表覆盖 NavActionId 全部变体（新增变体漏登记会在编译期穷尽性之外被本测试钉住）。
        let all = registry().all();
        assert_eq!(all.len(), 11);
        for id in [
            NavActionId::ShowProperties,
            NavActionId::ViewData,
            NavActionId::OpenSql,
            NavActionId::InsightPanel,
            NavActionId::GenDml,
            NavActionId::MockData,
            NavActionId::CopyName,
            NavActionId::CopyQualified,
            NavActionId::RefreshProps,
            NavActionId::DeleteObject,
            NavActionId::EmptyTable,
        ] {
            assert!(all.iter().any(|d| d.id == id), "缺动作 {id:?}");
        }
    }

    #[test]
    fn table_applies_all_data_and_analysis_actions() {
        let entries = registry().entries_for(Some(ObjectKind::Table), true, &OpSet::EMPTY);
        let got = ids(&entries);
        for id in [
            NavActionId::ShowProperties,
            NavActionId::ViewData,
            NavActionId::GenDml,
            NavActionId::OpenSql,
            NavActionId::InsightPanel,
            NavActionId::MockData,
            NavActionId::CopyName,
            NavActionId::CopyQualified,
            NavActionId::RefreshProps,
        ] {
            assert!(got.contains(&id), "表应出现 {id:?}");
        }
    }

    #[test]
    fn view_does_not_apply_dml_template() {
        let entries = registry().entries_for(Some(ObjectKind::View), true, &OpSet::EMPTY);
        let got = ids(&entries);
        assert!(!got.contains(&NavActionId::GenDml), "视图不生成 DML 模板");
        for id in [
            NavActionId::ViewData,
            NavActionId::MockData,
        ] {
            assert!(got.contains(&id), "视图应出现 {id:?}");
        }
    }

    /// B6 破坏性动作类型白名单（§4.3）：表挂删除 + 清空；视图 / 序列仅删除；其余类型两条都不出现。
    #[test]
    fn destructive_actions_follow_applies_matrix() {
        // 表：两条都出现。
        let table = ids(&registry().entries_for(Some(ObjectKind::Table), true, &OpSet::EMPTY));
        assert!(table.contains(&NavActionId::DeleteObject), "表应出现删除");
        assert!(table.contains(&NavActionId::EmptyTable), "表应出现清空");
        // 视图：仅删除，无清空。
        let view = ids(&registry().entries_for(Some(ObjectKind::View), true, &OpSet::EMPTY));
        assert!(view.contains(&NavActionId::DeleteObject), "视图应出现删除");
        assert!(!view.contains(&NavActionId::EmptyTable), "视图不出现清空");
        // 序列：仅删除，无清空。
        let sequence = ids(&registry().entries_for(Some(ObjectKind::Sequence), true, &OpSet::EMPTY));
        assert!(sequence.contains(&NavActionId::DeleteObject), "序列应出现删除");
        assert!(!sequence.contains(&NavActionId::EmptyTable), "序列不出现清空");
        // 例程 / 触发器 / 模式 / 列：两条都不出现。
        for kind in [
            ObjectKind::Routine,
            ObjectKind::Trigger,
            ObjectKind::Schema,
            ObjectKind::Column,
        ] {
            let got = ids(&registry().entries_for(Some(kind), true, &OpSet::EMPTY));
            assert!(!got.contains(&NavActionId::DeleteObject), "{kind:?} 不应出现删除");
            assert!(!got.contains(&NavActionId::EmptyTable), "{kind:?} 不应出现清空");
        }
    }

    #[test]
    fn schema_applies_only_insight_and_refresh_and_local() {
        let entries = registry().entries_for(Some(ObjectKind::Schema), true, &OpSet::EMPTY);
        let got = ids(&entries);
        for id in [
            NavActionId::ShowProperties,
            NavActionId::OpenSql,
            NavActionId::InsightPanel,
            NavActionId::RefreshProps,
            NavActionId::CopyName,
        ] {
            assert!(got.contains(&id), "schema 应出现 {id:?}");
        }
        for id in [
            NavActionId::ViewData,
            NavActionId::GenDml,
            NavActionId::MockData,
            NavActionId::CopyQualified,
        ] {
            assert!(!got.contains(&id), "schema 不应出现 {id:?}");
        }
    }

    #[test]
    fn routine_keeps_copy_qualified_only_on_source() {
        for kind in [ObjectKind::Routine, ObjectKind::Sequence, ObjectKind::Trigger] {
            let entries = registry().entries_for(Some(kind), true, &OpSet::EMPTY);
            let got = ids(&entries);
            assert!(got.contains(&NavActionId::CopyQualified), "{kind:?} 可复制限定名");
            assert!(got.contains(&NavActionId::CopyName));
            for id in [
                NavActionId::ViewData,
                NavActionId::GenDml,
                NavActionId::MockData,
            ] {
                assert!(!got.contains(&id), "{kind:?} 不应出现 {id:?}");
            }
        }
    }

    #[test]
    fn non_addressable_node_only_keeps_generic_local_actions() {
        // None（连接根 / 类别文件夹）：仅 applies 空切片的通用动作放行。
        let entries = registry().entries_for(None, true, &OpSet::EMPTY);
        let got = ids(&entries);
        for id in [
            NavActionId::ShowProperties,
            NavActionId::OpenSql,
            NavActionId::InsightPanel,
            NavActionId::CopyName,
        ] {
            assert!(got.contains(&id), "非可寻址节点应保留通用动作 {id:?}");
        }
        for id in [
            NavActionId::ViewData,
            NavActionId::GenDml,
            NavActionId::MockData,
            NavActionId::CopyQualified,
            NavActionId::RefreshProps,
        ] {
            assert!(!got.contains(&id), "非可寻址节点不应出现 {id:?}");
        }
    }

    #[test]
    fn disconnect_gates_source_actions_but_keeps_local() {
        let table_ops = family_ops("postgresql", Some(ObjectKind::Table));
        let entries = registry().entries_for(Some(ObjectKind::Table), false, &table_ops);
        for entry in &entries {
            match entry.descriptor.id {
                NavActionId::ShowProperties
                | NavActionId::OpenSql
                | NavActionId::InsightPanel
                | NavActionId::CopyName
                | NavActionId::CopyQualified => {
                    assert!(entry.enabled, "本地动作断连应可用 {:?}", entry.descriptor.id);
                }
                _ => assert!(!entry.enabled, "回源动作断连应置灰 {:?}", entry.descriptor.id),
            }
        }
    }

    #[test]
    fn disconnect_gates_destructive_actions_explicitly() {
        // B6：删除 / 清空属回源破坏性动作，断连一律置灰（不依赖 `_` 兜底，显式钉死）。
        let table_ops = family_ops("postgresql", Some(ObjectKind::Table));
        let entries = registry().entries_for(Some(ObjectKind::Table), false, &table_ops);
        let by_id: std::collections::HashMap<_, _> =
            entries.iter().map(|e| (e.descriptor.id, e.enabled)).collect();
        assert!(!by_id[&NavActionId::DeleteObject], "断连删除应置灰");
        assert!(!by_id[&NavActionId::EmptyTable], "断连清空应置灰");
    }

    #[test]
    fn connected_source_actions_need_family_ops() {
        // 族不认识该类型 → 空能力集；即便已连接，回源动作仍置灰；本地动作恒可用；
        // 刷新只查连接态，不查能力集。
        let empty = family_ops("mysql", Some(ObjectKind::Sequence));
        let entries = registry().entries_for(Some(ObjectKind::Sequence), true, &empty);
        for entry in &entries {
            match entry.descriptor.id {
                NavActionId::ShowProperties
                | NavActionId::OpenSql
                | NavActionId::InsightPanel
                | NavActionId::CopyName
                | NavActionId::CopyQualified => {
                    assert!(entry.enabled, "本地动作应可用 {:?}", entry.descriptor.id);
                }
                NavActionId::RefreshProps => {
                    assert!(entry.enabled, "刷新连接即可用，不查能力集");
                }
                _ => assert!(!entry.enabled, "族不支持应置灰 {:?}", entry.descriptor.id),
            }
        }
    }

    #[test]
    fn connected_table_ops_enable_source_actions() {
        let ops = family_ops("postgresql", Some(ObjectKind::Table));
        let entries = registry().entries_for(Some(ObjectKind::Table), true, &ops);
        for entry in &entries {
            assert!(entry.enabled, "连接 + 能力集覆盖应可用 {:?}", entry.descriptor.id);
        }
    }

    #[test]
    fn column_keeps_only_generic_local_actions() {
        // 列节点只放行本地通用动作；复制限定名 / 刷新 / 回源数据动作不进入条目序列
        // （靶 Some 判定由视图层在渲染时再过滤）。
        let entries = registry().entries_for(Some(ObjectKind::Column), true, &OpSet::EMPTY);
        let got = ids(&entries);
        for id in [
            NavActionId::ShowProperties,
            NavActionId::CopyName,
            NavActionId::OpenSql,
            NavActionId::InsightPanel,
        ] {
            assert!(got.contains(&id), "列节点应保留通用动作 {id:?}");
        }
        for id in [
            NavActionId::CopyQualified,
            NavActionId::RefreshProps,
            NavActionId::ViewData,
        ] {
            assert!(!got.contains(&id), "列节点不应出现 {id:?}");
        }
    }

    #[test]
    fn view_ops_without_dml_template_gates_gensql() {
        // 表节点挂视图能力集：GenDml 位缺失 → 置灰；ViewData 位存在 → 可用。
        // （视图条目本身不含 GenDml，由 `view_does_not_apply_dml_template` 钉类型白名单。）
        let ops = family_ops("postgresql", Some(ObjectKind::View));
        let entries = registry().entries_for(Some(ObjectKind::Table), true, &ops);
        let by_id: std::collections::HashMap<_, _> =
            entries.iter().map(|e| (e.descriptor.id, e.enabled)).collect();
        assert!(by_id[&NavActionId::ViewData]);
        assert!(!by_id[&NavActionId::GenDml]);
    }

    #[test]
    fn entries_are_grouped_and_ordered() {
        let entries = registry().entries_for(Some(ObjectKind::Table), true, &OpSet::EMPTY);
        let groups: Vec<ActionGroup> = entries.iter().map(|e| e.descriptor.group).collect();
        // 分组非递减：Primary → Generate → Object → Manage。
        for pair in groups.windows(2) {
            assert!(pair[0] <= pair[1], "分组顺序错乱：{:?} → {:?}", pair[0], pair[1]);
        }
        // 组内 order 递增。
        for group in [
            ActionGroup::Primary,
            ActionGroup::Generate,
            ActionGroup::Object,
            ActionGroup::Manage,
        ] {
            let orders: Vec<u16> = entries
                .iter()
                .filter(|e| e.descriptor.group == group)
                .map(|e| e.descriptor.order)
                .collect();
            for pair in orders.windows(2) {
                assert!(pair[0] < pair[1], "{group:?} 组内顺序错乱");
            }
        }
    }

    #[test]
    fn only_properties_carries_shortcut_metadata() {
        let all = registry().all();
        let props = all.iter().find(|d| d.id == NavActionId::ShowProperties).unwrap();
        assert_eq!(props.shortcut, Some("NavOpenProperties"));
        for d in all.iter().filter(|d| d.id != NavActionId::ShowProperties) {
            assert!(d.shortcut.is_none(), "{:?} 不应登记快捷键", d.id);
        }
    }

    #[test]
    fn destructive_flag_true_only_for_delete_and_empty() {
        // B6：仅删除对象 / 清空表两条为破坏性；其余九条均为 false（回源读 / 本地动作不进 staged 族）。
        let all = registry().all();
        for d in all {
            match d.id {
                NavActionId::DeleteObject | NavActionId::EmptyTable => {
                    assert!(d.destructive, "{:?} 应为破坏性", d.id);
                }
                _ => assert!(!d.destructive, "{:?} 不应为破坏性", d.id),
            }
        }
    }
}
