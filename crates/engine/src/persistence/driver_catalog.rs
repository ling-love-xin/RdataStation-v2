//! 驱动目录读取（`driver id → 数据库族 / 驱动显示名`）。
//!
//! 定位：`drivers` 表是导航连接行（徽标形状 + 短码）、hover 卡与对象属性面板的**共用元数据来源**，
//! 也是「驱动实现 id → 数据库族 id」的**唯一映射处**（[`type_id_of`]）。
//! 查询实现原先在 `workbench::services::nav_runtime`，但它是引擎侧知识（驱动注册表落库后的投影），
//! 且 `database`（导航视图下沉后）与 `workbench`（编辑器属性面板）都要读，故上收到本层：
//! 消费方各取所需，不再出现「视图层持有引擎查询」这种反向依赖。
//!
//! **驱动声明的权威在代码，本表是读模型**：`DriverDescriptor`（`driver/registry/descriptors.rs`）
//! 声明 `config_schema` / `capabilities` / `supported_auth_types` / `is_file` / `default_port` /
//! `url_template` / `driver_properties`，启动时由 `driver/declaration.rs::sync_driver_declarations`
//! 幂等 upsert 进 `drivers` 表；界面与连接链路照旧读表（读模型比每次现算便宜，也给
//! 外部驱动留了落库位置）。迁移 008/013/014/016/017 里的种子降为**首装兜底**（表结构仍由迁移建）；
//! 列归属（哪些列是声明拥有的、哪些归库 / 用户）见 `driver/declaration.rs` 头注。

use std::collections::HashMap;
use std::time::Duration;

use crate::persistence::driver_store;

/// 驱动目录条目。
///
/// 只带视图需要的字段（`type_id` 决定徽标形状与短码，`name` 用于 hover 卡），
/// 其余列（端口 / 认证方式 / 下载地址…）由连接对话框单独读取，避免目录胖化。
///
/// `type_name` / `type_category` 来自 `data_source_types`（**类型目录**）：
/// 导航侧展示类型名要用目录值，而不是在视图里硬编码一张类型表——
/// 否则新增一个库族就要改 UI，且同一个库在对话框（读目录）与导航（硬编码）会显示不一致。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverMeta {
    /// 数据库类型 id（`drivers.type_id`，如 `postgresql`）。
    pub type_id: String,
    /// 驱动显示名（`drivers.name`，如 `PostgreSQL (Official)`）。
    pub name: String,
    /// 类型显示名（`data_source_types.name`，如 `PostgreSQL`）；
    /// 目录里没有该类型（旧数据 / 未种子化）时为 `None` —— 调用方回退。
    pub type_name: Option<String>,
    /// 类型分类（`data_source_types.category`：`relational` / `file-based` /
    /// `analytics` / `nosql`）；分类 → 中文文案的映射属视图层，本层只给键。
    pub type_category: Option<String>,
}

/// 读取全局库里的驱动目录。
///
/// **渲染期不做 I/O**：调用方须在后台任务或 `cx.defer_in` 中一次性加载并缓存。
/// 读取失败（全局库未建 / `drivers` 表缺失）返回空表，消费方回退通用形状，不影响导航可用性。
pub fn load() -> HashMap<String, DriverMeta> {
    load_from_path().unwrap_or_default()
}

/// 只读打开全局库并取全部驱动（顺带带出**类型目录**里对应的显示名 / 分类）。
///
/// 用**只读**标志打开：全局库未初始化时宁可报错回退空表，也不要在用户目录里凭空建出空库。
/// 类型目录不按 `enabled` 过滤：已保存的连接可能引用已禁用的类型，展示不能凭空消失。
fn load_from_path() -> Result<HashMap<String, DriverMeta>, Box<dyn std::error::Error>> {
    let conn = open_global_ro()?;
    let types = load_type_directory(&conn);
    let drivers = driver_store::get_all_drivers(&conn)?;
    Ok(drivers
        .into_iter()
        .map(|d| {
            let meta = types.get(&d.type_id);
            (
                d.id,
                DriverMeta {
                    type_id: d.type_id,
                    name: d.name,
                    type_name: meta.map(|(name, _)| name.clone()),
                    type_category: meta.map(|(_, cat)| cat.clone()),
                },
            )
        })
        .collect())
}

/// 类型目录（`data_source_types` → `类型 id → (显示名, 分类)`）。
///
/// 表不存在（旧库 / 库未迁移）时返回空表，调用方回退——**不建表、不迁移**（只读入口）。
fn load_type_directory(conn: &rusqlite::Connection) -> HashMap<String, (String, String)> {
    let mut out = HashMap::new();
    let Ok(mut stmt) = conn.prepare("SELECT id, name, category FROM data_source_types") else {
        return out;
    };
    if let Ok(rows) = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
        ))
    }) {
        for (id, name, category) in rows.flatten() {
            out.insert(id, (name, category.unwrap_or_default()));
        }
    }
    out
}

/// 驱动 id → 数据库族 id（`drivers.type_id`）。
///
/// **两个概念不要混用**：连接记录里的 `db_type` 存的是驱动实现 id（`mysql_native`），
/// 而“数据库族”是 `data_source_types.id`（`mysql`）——驱动目录是唯一的映射处。
/// 需要族的调用方（DuckDB Secret 类型、元数据缓存身份指纹、导航 facet）应走这里，
/// 不要按字符串前缀猜（插件驱动的 id 与库族无关，如 JDBC 驱动目标库由 `type_id` 声明）。
///
/// 目录里没有该驱动（未初始化 / 已删 / 插件驱动未落库）→ `None`；调用方可回退原名。
pub fn type_id_of(driver_id: &str) -> Option<String> {
    let conn = open_global_ro().ok()?;
    driver_store::get_type_id(&conn, driver_id).ok().flatten()
}

/// 只读打开全局库（不建库、不建目录）。
fn open_global_ro() -> Result<rusqlite::Connection, Box<dyn std::error::Error>> {
    let path = crate::migration::get_global_db_path()?;
    let conn = rusqlite::Connection::open_with_flags(
        &path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    let _ = conn.busy_timeout(Duration::from_secs(3));
    Ok(conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_never_panics_without_global_db() {
        // 未初始化全局系统的进程（单测）只应拿到空表，不应 panic。
        let _ = load();
        // 族解析同理：目录不在位 → None（调用方回退原名，不编造族）
        let _ = type_id_of("mysql_native");
    }
}
