//! 连接绑定（B1）：文档绑定哪个连接、以及“有哪些连接可选”的端口
//!
//! ## 为什么要这一层
//!
//! 1a 的执行走的是引擎的 `DEFAULT_CONN_KEY = "active"`（**当前活动连接**）：用户在导航里
//! 点了哪个库，SQL 就发到哪个库——界面上**没有任何地方看得出这件事**（架构 §12 #26）。
//! B1 把“这条 SQL 发到哪”变成**文档属性**：绑定存在文档上（与 `mode` / `read_only` 同类），
//! 执行时随目标一起交给执行通道。
//!
//! ## 依赖方向
//!
//! 连接列表与建连都是**宿主的事**（workbench 侧才有 M3/M4 的加载器与连接管理器），
//! 因此这里只定义**端口**（[`ConnectionsPort`]）与展示所需的纯数据（[`ConnectionOption`]）：
//! 编辑器不依赖 `connection` / `database`，也不碰 I/O。
//!
//! ## 短码从哪来
//!
//! `P` / `G` / `GP`（项目 / 全局 / 共享快照）的口径**由宿主填**（`ConnectionOption.short`），
//! 避免在编辑器里再抄一份 id 前缀解析（那已经是 `engine::persistence::id_prefix` 与
//! `database::model::ConnectionScope` 的地盘）。

use std::rc::Rc;

/// 一个可绑定的连接（渲染路径只读的快照数据）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionOption {
    /// 稳定 id——绑定的就是它
    pub id: String,
    /// 作用域短码（`P` / `G` / `GP`；口径由宿主决定）
    pub short: String,
    /// 展示名（连接名）
    pub name: String,
    /// 运行态：是否已建连（宿主维护的真实状态，不猜）
    pub connected: bool,
    /// 【B10】驱动类型（`mysql_native` / `postgres_native` / `sqlite` / `duckdb` …）
    ///
    /// 编辑器用它决定**格式化 / 执行计划 / 转译用哪套方言**（同一个连接、两种引擎时，
    /// 方言不能猜）。宿主填（它才知道连接是什么驱动）。
    pub db_type: String,
}

/// 连接端口（宿主注入；未注入 = 没有可选项，且建连请求明确失败）
pub trait ConnectionsPort {
    /// 当前作用域下可选的连接（**必须是内存快照**：工具栏每帧都会读它）
    fn options(&self) -> Vec<ConnectionOption>;

    /// 确保某个连接已建连（未连则建连）；失败给**可读原因**（自动建连同 M4 口径）
    fn ensure_connected(&self, conn_id: &str) -> Result<(), String>;

    /// **实时**在线连接 id 集合（事件路径专用：连接菜单打开时调用）。
    ///
    /// 默认实现从 [`options`](Self::options) 快照推导（渲染安全）；宿主实现可覆盖为
    /// 运行时真查——导航建连成功只回写导航快照、不一定回写 [`options`] 的 `connected`，
    /// 菜单要“只列在线”就必须在这里拿真状态。**禁止在 render 路径调用**（宿主实现
    /// 可能同步查连接管理器）。
    fn online_ids(&self) -> std::collections::HashSet<String> {
        self.options()
            .into_iter()
            .filter(|option| option.connected)
            .map(|option| option.id)
            .collect()
    }
}

/// 端口句柄（与 `QueryRunner` / `SessionStore` 同一模式：宿主注入、可缺省）
pub type ConnectionsHandle = Rc<dyn ConnectionsPort>;

/// 一个库 / schema 项（状态栏面包屑的数据源；来自宿主导航缓存，渲染路径只读快照）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaInfo {
    /// 库（catalog）名；无 Catalog 层的驱动（MySQL 侧库即 schema）为 `None`
    pub catalog: Option<String>,
    /// schema 名
    pub schema: String,
}

/// schema 列表端口（T2c-1，宿主注入；未注入 = 没有 schema 可切）
///
/// 与 [`ConnectionsPort`] 同口径：**必须是内存快照**（状态栏每帧都读它）；
/// 实现侧未命中时安排后台预载、本次给空（渲染路径不做 I/O）。
pub trait SchemasPort {
    /// 某连接的库 / schema 列表（按导航缓存返回；连接不认识 = 空列表）
    fn schemas(&self, conn_id: &str) -> Vec<SchemaInfo>;
}

/// 端口句柄
pub type SchemasHandle = Rc<dyn SchemasPort>;

/// 状态栏面包屑（T2c-1）：`连接 ▸ 库 ▸ Schema`
///
/// 分段由纯函数算出来（可穷举断言）；缺哪段就不占哪段（零 UI 造数据）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Breadcrumb {
    /// 连接段（`●P·orders` / [`UNBOUND_LABEL`]）
    pub connection: String,
    /// 库段（当前 schema 所在库；无 Catalog 层或未选 schema 时 `None`）
    pub catalog: Option<String>,
    /// schema 段（文档当前绑定；`None` = 未选 → 控件层显示可点的「schema ▾」占位）
    pub schema: Option<String>,
}

/// 面包屑分段（纯函数）：连接 → 库 → schema
///
/// 库 / schema 段都从「schema 列表 + 当前绑定」推导：有绑定 → 带它所在库；
/// 没绑定 → 两段都不占（不拿列表第一个假装是当前）。
pub fn breadcrumb(
    conn_id: Option<&str>,
    options: &[ConnectionOption],
    schemas: &[SchemaInfo],
    current_schema: Option<&str>,
) -> Breadcrumb {
    let connection = status_text(conn_id, options);
    let (catalog, schema) = match current_schema {
        Some(current) => {
            let catalog = schemas
                .iter()
                .find(|item| item.schema == current)
                .and_then(|item| item.catalog.clone());
            (catalog, Some(current.to_string()))
        }
        None => (None, None),
    };
    Breadcrumb {
        connection,
        catalog,
        schema,
    }
}

/// 连接选择器的展示数据（纯函数算出来的，便于穷举断言）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionChip {
    /// 短码（`P` / `G` / `GP`）
    pub short: String,
    /// 展示名（连接名；认不出来的 id 就用 id 本身，不做美化）
    pub name: String,
    /// 运行态点：已建连为实心
    pub connected: bool,
}

impl ConnectionChip {
    /// 状态栏 / 工具栏文案：`●P·orders` / `○P·orders`
    ///
    /// 运行态用 `●` / `○` 而不是图标：这一处需要**文本可断言**（状态栏本来就是一行字），
    /// 图标留给工具栏按钮。
    pub fn text(&self) -> String {
        format!(
            "{}{}·{}",
            if self.connected { "●" } else { "○" },
            self.short,
            self.name
        )
    }
}

/// 未绑定连接时的状态栏文案（说明“会跟随当前连接”，而不是含糊的“无”）
pub const UNBOUND_LABEL: &str = "○ 未绑定连接";

/// 【T2c-2】驱动的用户可读方言名（状态栏「方言 X」）
///
/// 已知驱动映射成产品名；未知驱动**不猜**——原样显示（如实原则：猜错方言比不显示更糟）。
pub fn dialect_label(db_type: &str) -> String {
    match db_type {
        "mysql_native" | "mysql" => "MySQL".to_string(),
        "postgres_native" | "postgresql" | "postgres" => "PostgreSQL".to_string(),
        "sqlite" => "SQLite".to_string(),
        "duckdb" => "DuckDB".to_string(),
        other => other.to_string(),
    }
}

/// 驱动 → 语义引擎方言（#54 hover / 未来诊断的解析依据）
///
/// 与 `workbench::editor_completion::driver_to_dialect` 同表（editor 不依赖
/// workbench，这里登记同一映射）；未知驱动返回 `None`——不猜（Ansi 兜底由调用方定）。
pub fn sql_dialect(db_type: &str) -> Option<engine::sql::SqlDialect> {
    match db_type {
        "mysql_native" | "mysql" => Some(engine::sql::SqlDialect::Mysql),
        "postgres_native" | "postgresql" | "postgres" => Some(engine::sql::SqlDialect::Postgres),
        "sqlite" => Some(engine::sql::SqlDialect::Sqlite),
        "duckdb" => Some(engine::sql::SqlDialect::Duckdb),
        _ => None,
    }
}

/// 把绑定的 id 解析成展示数据；`None` = 解析不出（宿主没给这个连接 / 没接端口）
pub fn chip_for(
    conn_id: Option<&str>,
    options: &[ConnectionOption],
) -> Option<ConnectionChip> {
    let id = conn_id?;
    Some(match options.iter().find(|option| option.id == id) {
        Some(option) => ConnectionChip {
            short: option.short.clone(),
            name: option.name.clone(),
            connected: option.connected,
        },
        // 绑定还在、列表里没有（连接被删 / 换了项目）：显示 id，**不假装它还在**
        None => ConnectionChip {
            short: "?".to_string(),
            name: id.to_string(),
            connected: false,
        },
    })
}

/// 状态栏那一段文案（`None` 绑定 → [`UNBOUND_LABEL`]）
pub fn status_text(conn_id: Option<&str>, options: &[ConnectionOption]) -> String {
    match chip_for(conn_id, options) {
        Some(chip) => chip.text(),
        None => UNBOUND_LABEL.to_string(),
    }
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{ConnectionChip, ConnectionOption, UNBOUND_LABEL, chip_for, status_text};

    fn option(id: &str, name: &str, connected: bool) -> ConnectionOption {
        ConnectionOption {
            id: id.to_string(),
            short: if id.starts_with('G') { "G" } else { "P" }.to_string(),
            name: name.to_string(),
            connected,
            db_type: "mysql_native".to_string(),
        }
    }

    #[test]
    fn chip_text_marks_runtime_state() {
        let options = vec![
            option("P_orders", "orders", true),
            option("G_lab", "lab", false),
        ];
        assert_eq!(
            chip_for(Some("P_orders"), &options).expect("能解析").text(),
            "●P·orders"
        );
        assert_eq!(
            chip_for(Some("G_lab"), &options).expect("能解析").text(),
            "○G·lab"
        );
    }

    #[test]
    fn unbound_says_what_it_means() {
        // 未绑定不是“无连接”：执行仍会走当前活动连接（1a 口径），要如实说明
        let text = status_text(None, &[]);
        assert_eq!(text, UNBOUND_LABEL);
        assert!(text.contains("未绑定"));
        assert!(chip_for(None, &[]).is_none());
    }

    #[test]
    fn a_vanished_binding_shows_the_id_instead_of_a_pretend_name() {
        let text = status_text(Some("P_gone"), &[option("G_lab", "lab", true)]);
        assert!(text.contains("P_gone"), "认不出来就显示 id：{text}");
        assert!(text.contains('?'), "短码未知要看得出来：{text}");
        assert!(!text.contains('●'), "列表里没有就不能说它已连接：{text}");
    }

    #[test]
    fn chip_matches_the_bound_id_only() {
        let options = vec![option("P_a", "a", true), option("P_b", "b", true)];
        assert_eq!(chip_for(Some("P_b"), &options).expect("解析").name, "b");
        assert_ne!(chip_for(Some("P_b"), &options), chip_for(Some("P_a"), &options));
        // 大小写不同就是不同的 id（宿主给的 id 是稳定的，不做模糊匹配）
        assert!(chip_for(Some("p_b"), &options).is_some_and(|chip| chip.short == "?"));
    }

    #[test]
    fn chip_equality_is_by_value() {
        let chip = ConnectionChip {
            short: "P".to_string(),
            name: "orders".to_string(),
            connected: true,
        };
        assert_eq!(chip.clone().text(), chip.text());
    }

    /// T2c-1：面包屑分段——连接段恒在；库/schema 段只从「列表 + 绑定」推导
    #[test]
    fn breadcrumb_segments_follow_truth() {
        use super::{SchemaInfo, breadcrumb};
        let options = vec![option("P_orders", "orders", true)];
        let schemas = vec![
            SchemaInfo {
                catalog: Some("rdb".to_string()),
                schema: "public".to_string(),
            },
            SchemaInfo {
                catalog: Some("rdb".to_string()),
                schema: "analytics".to_string(),
            },
        ];

        // 没绑定 schema：库 / schema 段都不占（不拿第一个假装是当前）
        let crumb = breadcrumb(Some("P_orders"), &options, &schemas, None);
        assert_eq!(crumb.connection, "●P·orders");
        assert_eq!(crumb.catalog, None, "未选 schema 不该有库段");
        assert_eq!(crumb.schema, None, "未选 schema 不该有 schema 段");

        // 绑定了：带它所在库
        let crumb = breadcrumb(Some("P_orders"), &options, &schemas, Some("analytics"));
        assert_eq!(crumb.catalog.as_deref(), Some("rdb"));
        assert_eq!(crumb.schema.as_deref(), Some("analytics"));

        // 绑定在列表里找不到（列表没载好 / 连接不认识）：schema 段仍显示绑定，库段不猜
        let crumb = breadcrumb(Some("P_orders"), &options, &schemas, Some("ghost"));
        assert_eq!(crumb.schema.as_deref(), Some("ghost"));
        assert_eq!(crumb.catalog, None, "列表里没有就不能编一个库出来");

        // 未绑定连接：连接段如实说「未绑定」
        let crumb = breadcrumb(None, &options, &schemas, None);
        assert_eq!(crumb.connection, UNBOUND_LABEL);
    }

    /// T2c-2：方言名如实映射——已知驱动给产品名，未知驱动原样（不猜）
    #[test]
    fn dialect_labels_known_drivers_and_keeps_unknown() {
        use super::dialect_label;
        assert_eq!(dialect_label("mysql_native"), "MySQL");
        assert_eq!(dialect_label("mysql"), "MySQL");
        assert_eq!(dialect_label("postgres_native"), "PostgreSQL");
        assert_eq!(dialect_label("sqlite"), "SQLite");
        assert_eq!(dialect_label("duckdb"), "DuckDB");
        // 未知驱动：原样显示（猜错比不显示更糟）
        assert_eq!(dialect_label("oracle"), "oracle");
        assert_eq!(dialect_label(""), "");
    }
}
