//! 驱动目录读取（`driver id → 数据库类型 / 驱动显示名`）。
//!
//! 定位：`drivers` 表是导航连接行（徽标形状 + 短码）、hover 卡与对象属性面板的**共用元数据来源**。
//! 查询实现原先在 `workbench::services::nav_runtime`，但它是引擎侧知识（驱动注册表落库后的投影），
//! 且 `database`（导航视图下沉后）与 `workbench`（编辑器属性面板）都要读，故上收到本层：
//! 消费方各取所需，不再出现「视图层持有引擎查询」这种反向依赖。
//!
//! 与 [`crate::driver::metadata::DriverMetadata`] 的区别：那个是**内置驱动的静态描述**
//! （代码里写死的 id / 版本 / 图标），本模块读的是**库里已注册的驱动行**（含导出的外部驱动）。

use std::collections::HashMap;
use std::time::Duration;

use crate::persistence::driver_store;

/// 驱动目录条目。
///
/// 只带视图需要的两项（`type_id` 决定徽标形状与短码，`name` 用于 hover 卡），
/// 其余列（端口 / 认证方式 / 下载地址…）由连接对话框单独读取，避免目录胖化。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverMeta {
    /// 数据库类型 id（`drivers.type_id`，如 `postgresql`）。
    pub type_id: String,
    /// 驱动显示名（`drivers.name`，如 `PostgreSQL (Official)`）。
    pub name: String,
}

/// 读取全局库里的驱动目录。
///
/// **渲染期不做 I/O**：调用方须在后台任务或 `cx.defer_in` 中一次性加载并缓存。
/// 读取失败（全局库未建 / `drivers` 表缺失）返回空表，消费方回退通用形状，不影响导航可用性。
pub fn load() -> HashMap<String, DriverMeta> {
    load_from_path().unwrap_or_default()
}

/// 只读打开全局库并取全部驱动。
///
/// 用**只读**标志打开：全局库未初始化时宁可报错回退空表，也不要在用户目录里凭空建出空库。
fn load_from_path() -> Result<HashMap<String, DriverMeta>, Box<dyn std::error::Error>> {
    let path = crate::migration::get_global_db_path()?;
    let conn = rusqlite::Connection::open_with_flags(
        &path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    let _ = conn.busy_timeout(Duration::from_secs(3));
    let drivers = driver_store::get_all_drivers(&conn)?;
    Ok(drivers
        .into_iter()
        .map(|d| {
            (
                d.id,
                DriverMeta {
                    type_id: d.type_id,
                    name: d.name,
                },
            )
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_never_panics_without_global_db() {
        // 未初始化全局系统的进程（单测）只应拿到空表，不应 panic。
        let _ = load();
    }
}
