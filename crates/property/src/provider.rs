//! 属性源接口（对齐 DBeaver `DBPPropertySource`）——驱动无关的动态渲染核心。
//!
//! DBeaver 的动态渲染机制（源码实证）：`DBPPropertySource` 是接口，
//! 各数据库插件（`ext.postgresql` 等）用 `@Property` 注解 getter 实现它；
//! 属性面板 UI **只依赖接口**，不按数据库类型写分支——新增数据库 = 新插件实现接口，UI 零改动。
//!
//! RDS 对齐：本模块定义 [`PropertyProvider`] trait + [`ProviderRegistry`]。
//! - **驱动特异部分**（属性行集合 / 洞察总览 / 子项类别树 / 编辑 SQL）由各驱动 provider 实现；
//! - **通用骨架**（`database::property_panel::load_properties` 的列/索引/约束/DDL/数据取数）
//!   驱动无关，provider 只补充"定义信息"；
//! - **渲染层**（workbench）只依赖 `ProviderRegistry::provider(driver, kind)`。
//!
//! 新增数据库（如 Oracle）= `database` crate 注册一个 `OraclePropertyProvider`，
//! **property crate 模型 / 通用骨架 / 渲染层零改动**。

use crate::descriptor::PageId;
use crate::model::{PropertyKind, PropertyRow, PropertyTable};

/// 属性目标（从 `database::model::PropertyRef` 提取的轻量引用；不依赖 database crate）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropertyTargetRef {
    pub kind: PropertyKind,
    pub name: String,
    pub catalog: Option<String>,
    pub schema: Option<String>,
    /// 父对象（列的所属表 / 触发器的关联表）
    pub parent: Option<String>,
    pub conn_id: String,
    /// 归属域文案（项目 (P) / 全局 (G) / 共享 (GP)）
    pub source: String,
}

/// 通用骨架统计（provider 可在属性行 / 洞察中引用）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProviderStats {
    pub column_count: usize,
    pub index_count: usize,
    pub constraint_count: usize,
}

/// 属性源上下文（provider 组装属性行 / 洞察 / 类别的输入）。
pub struct ProviderInput<'a> {
    pub target: &'a PropertyTargetRef,
    /// 连接显示名（连接对象标题）
    pub conn_label: &'a str,
    /// 数据库类型（`drivers.type_id`，可为空）
    pub db_type: Option<&'a str>,
    pub stats: ProviderStats,
    /// 调用方统计（洞察指标：schemas / tables / views / cache_hits / row_estimate / size …；
    /// 未提供则 provider 省略对应指标卡）
    pub counts: &'a [(&'a str, String)],
    /// 骨架传入的附加对象（列明细等；provider 自行 downcast，`None` 则跳过特异行）
    pub detail: Option<&'a dyn std::any::Any>,
}

impl<'a> ProviderInput<'a> {
    /// 读取调用方统计（`counts` 中按 key 取值）。
    pub fn count(&self, key: &str) -> Option<&'a str> {
        self.counts
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.as_str())
    }
}

/// 洞察总览状态（对象徽标右侧状态点）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverviewStatus {
    /// 在线（连接已建立 / 对象已加载）
    Connected,
    /// 已断开 / 失败
    Disconnected,
    /// 未连接（占位）
    Idle,
}

/// 洞察指标卡。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverviewCard {
    pub label: String,
    pub value: String,
    pub unit: String,
    /// 补充行（口径说明，如 `reltuples`）
    pub delta: Option<String>,
}

/// 洞察总览（RDS 新增部分——DBeaver 只有 General + 子项，我们加"一眼看统计"）。
///
/// 去重原则：洞察只放**运行时 / 统计事实**（版本 / 主机 / 对象计数 / 缓存 / 行数）；
/// 对象**定义属性**（可编辑配置）归 General。
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectOverview {
    pub title: String,
    pub subtitle: String,
    pub status: OverviewStatus,
    pub status_text: String,
    pub cards: Vec<OverviewCard>,
    /// 关键信息（键值行，如 版本 / 主机 / 延迟）
    pub kv: Vec<(String, String)>,
    /// 数据口径说明（markdown 风格文本，渲染为注脚）
    pub note: String,
    /// 项目信息芯片（驱动 / 作用域 / 分组 / 标签——RDS 连接模型独有）
    pub meta: Vec<(String, String)>,
}

/// 子项类别（DBeaver 下部左侧类别树的一类）。
///
/// `table` 由通用骨架按 [`PageId`] 合并（列/索引/约束/DDL 已通用取数）；
/// provider 只声明类别集合、顺序与可用性——**新增类别 / 灰置类别都在这里**。
#[derive(Debug, Clone, PartialEq)]
pub struct PropertyCategory {
    pub id: PageId,
    pub label: String,
    /// `false` = 先绘制、置灰标注（能力未实现，DBeaver 同样有灰置页）
    pub enabled: bool,
    pub table: PropertyTable,
}

impl PropertyCategory {
    /// 置灰类别（内容区显示「暂不可用」占位）。
    pub fn disabled(id: PageId, label: &str) -> Self {
        Self {
            id,
            label: label.to_string(),
            enabled: false,
            table: PropertyTable {
                headers: Vec::new(),
                rows: Vec::new(),
            },
        }
    }
}

/// 属性源（对齐 DBeaver `DBPPropertySource`）——**驱动实现，面板零改动**。
pub trait PropertyProvider: Send + Sync {
    /// 驱动标识（`drivers.type_id`：`postgres` / `sqlite` / `duckdb` / `mysql` / `oracle` …）。
    fn driver(&self) -> &'static str;
    /// 服务的对象类型（每驱动 × 类型一个 provider）。
    fn kind(&self) -> PropertyKind;

    /// 属性行集合（对齐 `getProperties()`；值由 provider 组装，可引用 [`ProviderInput`] 统计）。
    fn property_defs(&self, input: &ProviderInput) -> Vec<PropertyRow>;

    /// 洞察总览（RDS 新增；运行时 / 统计事实，与 General 去重）。
    fn overview(&self, input: &ProviderInput) -> ObjectOverview;

    /// 子项类别树（驱动能力掩码；灰置类别 = `enabled: false`）。
    fn categories(&self, input: &ProviderInput) -> Vec<PropertyCategory>;

    /// 编辑 SQL 生成（对齐 `setPropertyValue`——生成语句，执行由调用方）。
    /// 返回实际 SQL（渲染方展示）；不支持返回 `Err`（明确报错，不静默）。
    fn apply_edit_sql(
        &self,
        input: &ProviderInput,
        row_id: &str,
        value: &str,
    ) -> Result<String, String>;
}

/// 属性源注册表（驱动 × 对象类型）——渲染 / 骨架只依赖它。
///
/// 全局单例由 `database::property_providers::global_registry()` 提供（首次访问时注册全部驱动）。
#[derive(Default)]
pub struct ProviderRegistry {
    providers: Vec<Box<dyn PropertyProvider>>,
}

impl ProviderRegistry {
    /// 注册 provider（应用启动 / 首次访问时调用）。
    pub fn register(&mut self, provider: Box<dyn PropertyProvider>) {
        self.providers.push(provider);
    }

    /// 按 驱动 × 对象类型 查找 provider；未注册返回 `None`（渲染回退通用展示）。
    /// 驱动比较大小写不敏感（`postgres` / `postgresql` / `pg` 等价）。
    pub fn provider(&self, driver: &str, kind: PropertyKind) -> Option<&dyn PropertyProvider> {
        self.providers
            .iter()
            .find(|p| p.driver().eq_ignore_ascii_case(driver) && p.kind() == kind)
            .map(|p| p.as_ref())
    }

    /// 全部 provider（调试 / 测试用）。
    pub fn providers(&self) -> &[Box<dyn PropertyProvider>] {
        &self.providers
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 类别置灰辅助：enabled=false + 空表。
    #[test]
    fn disabled_category_has_empty_table() {
        let c = PropertyCategory::disabled(PageId::Privileges, "权限");
        assert!(!c.enabled);
        assert!(c.table.headers.is_empty());
        assert!(c.table.rows.is_empty());
    }

    /// 注册表按 驱动 × 类型 命中；未注册返回 None。
    #[test]
    fn registry_resolves_provider_by_driver_and_kind() {
        struct PgConn;
        impl PropertyProvider for PgConn {
            fn driver(&self) -> &'static str { "postgres" }
            fn kind(&self) -> PropertyKind { PropertyKind::Connection }
            fn property_defs(&self, _i: &ProviderInput) -> Vec<PropertyRow> { vec![] }
            fn overview(&self, _i: &ProviderInput) -> ObjectOverview {
                ObjectOverview {
                    title: String::new(), subtitle: String::new(),
                    status: OverviewStatus::Connected, status_text: String::new(),
                    cards: vec![], kv: vec![], note: String::new(), meta: vec![],
                }
            }
            fn categories(&self, _i: &ProviderInput) -> Vec<PropertyCategory> { vec![] }
            fn apply_edit_sql(&self, _i: &ProviderInput, _id: &str, _v: &str) -> Result<String, String> {
                Err("unsupported".to_string())
            }
        }

        let mut reg = ProviderRegistry::default();
        reg.register(Box::new(PgConn));
        assert!(reg.provider("postgres", PropertyKind::Connection).is_some());
        assert!(reg.provider("postgres", PropertyKind::Table).is_none(), "类型不匹配");
        assert!(reg.provider("sqlite", PropertyKind::Connection).is_none(), "驱动不匹配");
    }
}
