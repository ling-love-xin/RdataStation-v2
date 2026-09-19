//! 数据库类型 → 图标（微标）：给 `data_source_types` 的每一行一个可渲染的图标。
//!
//! ## 通用图标够用，品牌图标是另一回事
//!
//! 组件库自带 **Lucide 全量集**（`gpui_kit::assets::IconName`，1830 个 SVG，由
//! `gpui-kit-assets` 嵌进 `icons/` 命名空间）：`database` / `table` / `table-properties` /
//! `eye`（视图）/ `hash`（索引）/ `key-round`（主键）/ `link-2`（外键）/ `list-ordered`（序列）/
//! `square-function`（例程）/ `zap`（触发器）/ `braces`（JSON）/ `binary`（BLOB）/
//! `file-spreadsheet`（CSV）… **对象与文件类型这一层是完整的**，不需要自备。
//!
//! **品牌标（海豚 / 大象 / 鸭子 / 红标）不在里面**，那是厂商注册商标：收不收录属产品与法务
//! 拍板，不是工程缺口（JetBrains 系之所以「有那么多」，是逐个收录 + 法务背书，口径与落地
//! 配方见 `docs/architecture/ui/db-icons.md`）。本模块因此只做两件事：
//!
//! 1. 通用图标**按类型 → 按分类**给到位（[`db_icon_of`]）；
//! 2. 给品牌标留一个**稳定落点**（[`DbIcon::Brand`]，路径由 [`DbIcon::brand_path`] 推导）。
//!
//! ## 用法
//!
//! `DbIcon` 实现了 `IconNamed`，所以能直接交给组件：
//!
//! ```ignore
//! let icon = gpui_kit::component::Icon::new(db_icon_of("mysql", Some("relational")));
//! ```
//!
//! ## 现状与边界
//!
//! - `data_source_types.icon` 里的 **emoji**（MySQL=🐬…）是**数据**，本模块不碰它；
//!   渲染优先级（品牌标 > 通用标 > emoji > 形状 + 2 字母）由调用点决定，这里只提供 SVG 侧。
//! - **尚未接线**：导航与对话框的类型徽标今天仍走 emoji（`helpers::type_badge`）与
//!   「形状 + 2 字母」（能力矩阵 §7 #10 记为有意设计）；接线属各自视图轮次，
//!   `crates/database/src/nav_view.rs` 当时正被并发会话修改，故本模块只入表不接。

use gpui_kit::SharedString;
use gpui_kit::assets::{IconName, IconNamed};

/// 一个数据库类型的图标来源。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum DbIcon {
    /// 组件库自带（Lucide 全量集）：AssetSource 由应用统一注册（`gpui-kit-assets`），开箱可用。
    Catalog(IconName),
    /// 自备品牌标：路径按 [`DbIcon::brand_path`] 约定推导，**文件要落在 `icons/db/` 命名空间**，
    /// 且必须在应用侧 AssetSource 里登记（`crates/app/src/main.rs` 的 `.with_assets(...)`）。
    ///
    /// 本仓当前**没有任何一处构造它**——还没有获得授权的品牌 SVG；拿到之后按文档三步走：
    /// 放文件 → 登记资产源 → 把 [`db_icon_of`] 对应行从 `Catalog(..)` 换成 `Brand { .. }`。
    /// 按类型 id 而不是路径取参数，就是为了让「一张图 = 一行改动」成立。
    Brand {
        /// `data_source_types.id`（同时是文件名：`icons/db/<type_id>.svg`）。
        type_id: &'static str,
    },
}

impl DbIcon {
    /// 品牌标的路径约定：独立命名空间 `icons/db/`。
    ///
    /// 必须独立成目录：Lucide 占的是 `icons/<名字>.svg` **平铺**命名空间（1830 个），
    /// 自备图若直接叫 `mysql.svg`，将来 Lucide 收录同名图标就会串。
    pub fn brand_path(type_id: &str) -> String {
        format!("icons/db/{type_id}.svg")
    }
}

impl IconNamed for DbIcon {
    fn path(self) -> SharedString {
        match self {
            DbIcon::Catalog(name) => name.path(),
            DbIcon::Brand { type_id } => Self::brand_path(type_id).into(),
        }
    }
}

/// 类型 id + 分类 → 图标。
///
/// 优先级：**按类型**（`data_source_types.id`）→ **按分类**（`data_source_types.category`）→
/// 通用 `Database`。判定用不着查库：两个入参都由调用点从类型目录带过来（`driver_catalog`
/// 已给出 `type_name` / `type_category`）。
///
/// 新增数据库类型时不改这里也不会崩（落到分类档），但同族会长得一样——要区分就得加一行。
/// 「种子里的每个类型都有明确一行」由本模块的测试盯着（种子里加类型而这里没加 → 红）。
pub fn db_icon_of(type_id: &str, category: Option<&str>) -> DbIcon {
    let icon = match type_id.trim().to_ascii_lowercase().as_str() {
        // 关系型：通用「库」图标。品牌标落地前同族同标——区分靠名字列或品牌 SVG，不靠这套通用图标。
        "mysql" | "mariadb" | "postgresql" | "oracle" | "mssql" | "sqlserver" => IconName::Database,
        // 单文件库：盘上的一个文件（SQLite 的形态特征就是「一个文件」）。
        "sqlite" => IconName::HardDrive,
        // 分析 / 列式：与 DuckDB 加速通道同一个语义（快），不复用关系型的「库」。
        "duckdb" | "clickhouse" => IconName::DatabaseZap,
        // 文档库：存的是 JSON 形态的文档。
        "mongodb" => IconName::Braces,
        // 内存 KV：键值对常驻内存。
        "redis" => IconName::MemoryStick,
        _ => match category.unwrap_or_default().trim().to_ascii_lowercase().as_str() {
            "relational" => IconName::Database,
            "file-based" => IconName::HardDrive,
            "analytics" => IconName::DatabaseZap,
            "nosql" => IconName::Boxes,
            // 分类也没给 / 没收录：给最中性的「库」，不猜。
            _ => IconName::Database,
        },
    };
    DbIcon::Catalog(icon)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 期望表：**种子里每个类型一行**。比断言「不等于兜底」更有意义——它连「同族用了哪个图标」
    /// 也一起钉住了，改映射必须来这里同步（有意的：图标是对外可见的行为）。
    const EXPECTED: &[(&str, IconName)] = &[
        ("mysql", IconName::Database),
        ("mariadb", IconName::Database),
        ("postgresql", IconName::Database),
        ("oracle", IconName::Database),
        ("mssql", IconName::Database),
        ("sqlite", IconName::HardDrive),
        ("duckdb", IconName::DatabaseZap),
        ("clickhouse", IconName::DatabaseZap),
        ("mongodb", IconName::Braces),
        ("redis", IconName::MemoryStick),
    ];

    /// 从迁移种子读出类型 id。
    ///
    /// 跨 crate 读 `engine` 的迁移文件是**测试专用**的技巧：`include_str!` 只把文本编进来，
    /// 不产生依赖边（`workbench-shell` 不得依赖任何特性 crate，见 crate 文档）。好处是
    /// 「种子加了类型、图标表没跟上」这种漂移**会红**，而不是靠人自觉——同款先例见
    /// `crates/workbench/tests/ui_contract.rs` 对 `database/src/nav_view.rs` 的扫描。
    fn seed_type_ids() -> Vec<String> {
        const SEED: &str =
            include_str!("../../engine/migrations/global/008_add_data_source_module.sql");
        let start = SEED
            .find("INSERT OR IGNORE INTO data_source_types")
            .expect("种子语句不见了：迁移改了写法就同步这里");
        let statement = &SEED[start..];
        let statement = &statement[..statement.find(';').expect("种子语句没有结束分号")];
        statement
            .split("('")
            .skip(1)
            .filter_map(|chunk| chunk.split('\'').next())
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn every_seed_type_has_an_explicit_icon_row() {
        let mut seed = seed_type_ids();
        seed.sort();
        let mut expected: Vec<String> = EXPECTED.iter().map(|(id, _)| (*id).to_string()).collect();
        expected.sort();
        assert_eq!(
            seed, expected,
            "类型目录（008 种子）与本表的类型集合不一致：种子里加/删了类型，就要在 \
             `db_icons::db_icon_of` 与 `EXPECTED` 各加/删一行"
        );
    }

    #[test]
    fn each_seed_type_maps_to_its_row() {
        for (type_id, icon) in EXPECTED {
            assert_eq!(
                db_icon_of(type_id, None),
                DbIcon::Catalog(*icon),
                "{type_id} 的图标变了（改映射要连 EXPECTED 一起改）"
            );
        }
        // 大小写与空白不该改变结果（类型 id 来自库，来源不保证规范）。
        assert_eq!(db_icon_of(" MySQL ", None), DbIcon::Catalog(IconName::Database));
        // 别名：mssql 的另一种常见写法。
        assert_eq!(db_icon_of("sqlserver", None), DbIcon::Catalog(IconName::Database));
    }

    #[test]
    fn unknown_types_fall_back_by_category_then_to_the_generic_database() {
        // 目录里新出现、本表还没收录的类型：先用分类兜住。
        assert_eq!(
            db_icon_of("snowflake", Some("analytics")),
            DbIcon::Catalog(IconName::DatabaseZap)
        );
        assert_eq!(
            db_icon_of("firebird", Some("relational")),
            DbIcon::Catalog(IconName::Database)
        );
        assert_eq!(
            db_icon_of("some-file-db", Some("file-based")),
            DbIcon::Catalog(IconName::HardDrive)
        );
        assert_eq!(
            db_icon_of("cassandra", Some("nosql")),
            DbIcon::Catalog(IconName::Boxes)
        );
        // 分类也缺 / 未收录 → 最中性的「库」，不猜。
        assert_eq!(db_icon_of("unknown", None), DbIcon::Catalog(IconName::Database));
        assert_eq!(
            db_icon_of("unknown", Some("weird-category")),
            DbIcon::Catalog(IconName::Database)
        );
    }

    /// 映射指向的图标必须真的在资产里能取到。
    ///
    /// 这条挡的是「名字看着对、资产其实没有」：`IconName` 的变体名换成路径后要能在
    /// `AllAssets` 里命中（漏了就是渲染成空白，界面上什么都看不出来）。
    #[test]
    fn mapped_catalog_icons_resolve_in_the_asset_source() {
        let mut paths: Vec<SharedString> = EXPECTED
            .iter()
            .map(|(_, icon)| IconNamed::path(DbIcon::Catalog(*icon)))
            .collect();
        paths.push(IconNamed::path(DbIcon::Catalog(IconName::Boxes)));
        for path in paths {
            assert!(
                gpui_kit::assets::AllAssets::get(path.as_ref()).is_some(),
                "资产里没有 {path}"
            );
        }
    }

    #[test]
    fn brand_paths_live_in_their_own_namespace() {
        assert_eq!(DbIcon::brand_path("mysql"), "icons/db/mysql.svg");
        assert_eq!(
            IconNamed::path(DbIcon::Brand { type_id: "mysql" }),
            SharedString::from("icons/db/mysql.svg"),
            "Brand 的路径必须由约定推导（换一行就能接上品牌标，不用手写路径）"
        );
        // 品牌标与 Lucide 的平铺命名空间不重叠。
        assert!(!DbIcon::brand_path("table").starts_with("icons/table"));
    }
}
