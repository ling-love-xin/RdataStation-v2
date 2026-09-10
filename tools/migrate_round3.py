# -*- coding: utf-8 -*-
"""RdataStation v2 迁移 Round 3：connection crate。
从 engine/src/driver/connection 抽取连接层（config/connector/factory/known_hosts/stream），
并更新 engine 内引用点。
"""
import shutil
import pathlib

ENG = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\src")
CON = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\connection\src")

# ---------- 1) 复制连接层文件 ----------
for name in ["config.rs", "connector.rs", "factory.rs", "known_hosts.rs", "stream.rs", "mod.rs"]:
    shutil.copy2(ENG / "driver" / "connection" / name, CON / name)
print("connection: 6 files copied from engine/driver/connection")

# ---------- 2) 重写 connection/src/lib.rs ----------
lib = """//! RdataStation v2 connection crate（M3 数据源连接）
//!
//! 自 v1 `core/driver/connection` 迁移（配置/连接器/SSH/SSL/Proxy/流）。
//! 后续待迁移：`core/services/connection_service.rs`（连接生命周期服务）、
//! DuckDB Secret 本地加速通道（`secret.rs`，M3 核心差异化能力）。
//!
//! 依赖方向：connection → rds-shared；Feature crates → connection。

pub mod config;
pub mod connector;
pub mod factory;
pub mod known_hosts;
pub mod stream;

// 占位模块（TODO(migration): 按 GPUI-kit 规范实现命令与视图）
pub mod model;
pub mod commands;
pub mod connection_view;
pub mod connection_dialog;
pub mod secret;

pub use config::ConnectionConfig;
pub use connector::Connection;
pub use factory::ConnectionFactory;
pub use stream::ConnectionStream;
"""
(CON / "lib.rs").write_text(lib, encoding="utf-8")
print("connection: lib.rs rewritten")

# ---------- 3) engine 引用点更新 ----------
# driver/mod.rs 删除 connection 模块声明
m = ENG / "driver" / "mod.rs"
t = m.read_text(encoding="utf-8")
t = t.replace("pub mod connection;\n", "")
m.write_text(t, encoding="utf-8")

# driver/factory.rs 与 driver/registry/config.rs 改用 connection crate
for rel in ["driver/factory.rs", "driver/registry/config.rs"]:
    f = ENG / rel
    t = f.read_text(encoding="utf-8")
    t = t.replace("super::connection::config::ConnectionMethod", "connection::config::ConnectionMethod")
    t = t.replace("super::connection::{ConnectionConfig, ConnectionFactory}", "connection::{ConnectionConfig, ConnectionFactory}")
    t = t.replace("crate::driver::connection::config::ConnectionMethod", "connection::config::ConnectionMethod")
    f.write_text(t, encoding="utf-8")
print("engine: driver/mod.rs + factory.rs + registry/config.rs updated")

# ---------- 4) engine 删除已抽取的 connection 子目录 ----------
shutil.rmtree(ENG / "driver" / "connection")
print("engine: driver/connection removed")

print("DONE")
