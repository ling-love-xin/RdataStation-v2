# -*- coding: utf-8 -*-
"""RdataStation v2 迁移 Round 8：workbench crate（M5 查询工作台）
自 v1 core/services 迁入：connection_service / result_service / persistence_service / driver_service
依赖：engine（services/persistence/driver/connection_manager）+ connection + insight + shared
"""
import shutil
import pathlib

V1 = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\v1\backend\src\core\services")
WB = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src")

REPL = [
    ("crate::core::driver::connection::", "connection::"),
    ("crate::core::services::connection_manager::", "engine::connection_manager::"),
    ("crate::core::services::duckdb_service::", "engine::services::duckdb_service::"),
    ("crate::core::services::execution_service::", "engine::services::execution_service::"),
    ("crate::core::services::sql_service::", "engine::services::sql_service::"),
    ("crate::core::services::insight_engine::", "insight::insight_engine::"),
    ("crate::core::services::quality_scorer::", "insight::quality_scorer::"),
    ("crate::core::services::table_profile_service::", "insight::table_profile_service::"),
    ("crate::core::services::result_service::", "crate::services::result_service::"),
    ("crate::core::services::persistence_service::", "crate::services::persistence_service::"),
    ("crate::core::services::SqlService", "engine::SqlService"),
    ("crate::core::services::", "crate::services::"),
    ("crate::core::driver::", "engine::driver::"),
    ("crate::core::persistence::", "engine::persistence::"),
    ("crate::core::models::", "shared::models::"),
    ("crate::core::error::", "shared::error::"),
    ("crate::core::cache::", "engine::cache::"),
    ("crate::core::duckdb::", "engine::duckdb::"),
    ("crate::core::get_connection_manager", "engine::get_connection_manager"),
    ("crate::core::DuckDBManager", "engine::DuckDBManager"),
    ("crate::core::{", "engine::{"),
]

def rewrite(text):
    for old, new in REPL:
        text = text.replace(old, new)
    return text

# ---------- 1) 复制 4 文件到 services/ ----------
dst = WB / "services"
dst.mkdir(parents=True, exist_ok=True)
for name in ["connection_service.rs", "result_service.rs", "persistence_service.rs", "driver_service.rs"]:
    shutil.copy2(V1 / name, dst / name)
print("workbench: services/ 4 files copied")

# ---------- 2) 改写 ----------
for f in (WB / "services").rglob("*.rs"):
    f.write_text(rewrite(f.read_text(encoding="utf-8")), encoding="utf-8")
print("workbench: references rewritten")

# ---------- 3) services/mod.rs ----------
mod = """//! 工作台服务层（自 v1 `core/services` 迁移）
//!
//! - `connection_service`：用户数据源连接管理（测试/CRUD/路由）
//! - `result_service`：结果集服务 + 洞察计算编排
//! - `persistence_service`：工作台持久化（结果/画像回写）
//! - `driver_service`：驱动管理（注册/文件/数据源类型）

pub mod connection_service;
pub mod driver_service;
pub mod persistence_service;
pub mod result_service;
"""
(WB / "services" / "mod.rs").write_text(mod, encoding="utf-8")
print("workbench: services/mod.rs written")
print("DONE")
