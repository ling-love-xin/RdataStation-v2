# -*- coding: utf-8 -*-
"""RdataStation v2 迁移 Round 12：plugin crate（M9 插件系统）
- v1 core/plugin 全量（dependency/events/installer/loader/manager/manifest/permission + storage）
- v1 core/services 的 plugin_bridge / plugin_service
- v1 adapters/sidecar（Go Sidecar 进程管理 + JSON-RPC 驱动适配）
- v1 adapters/wasm（Extism WASM 运行时插件适配）；tauri 适配器退役
"""
import shutil
import pathlib

V1 = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\v1\backend\src")
PL = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\plugin\src")

REPL = [
    ("crate::core::plugin::", "crate::"),
    ("crate::core::services::", "crate::"),
    ("crate::core::get_connection_manager", "engine::get_connection_manager"),
    ("crate::core::models::", "shared::models::"),
    ("crate::core::driver::", "engine::driver::"),
    ("crate::core::persistence::", "engine::persistence::"),
    ("crate::core::error::", "shared::error::"),
    ("crate::core::CoreError", "engine::CoreError"),
    ("crate::core::", "engine::"),
]

def rewrite(text):
    for old, new in REPL:
        text = text.replace(old, new)
    return text

# ---------- 1) core/plugin ----------
for name in ["dependency.rs", "events.rs", "installer.rs", "loader.rs", "manager.rs",
             "manifest.rs", "permission.rs", "storage.rs"]:
    shutil.copy2(V1 / "core" / "plugin" / name, PL / name)
print("plugin: core/plugin 8 files copied")

# ---------- 2) plugin_bridge / plugin_service ----------
for name in ["plugin_bridge.rs", "plugin_service.rs"]:
    shutil.copy2(V1 / "core" / "services" / name, PL / name)
print("plugin: plugin_bridge/plugin_service copied")

# ---------- 3) sidecar / wasm 适配器 ----------
shutil.copytree(V1 / "adapters" / "sidecar", PL / "sidecar", dirs_exist_ok=True)
shutil.copytree(V1 / "adapters" / "wasm", PL / "wasm", dirs_exist_ok=True)
print("plugin: sidecar/ + wasm/ adapters copied")

# ---------- 4) 改写 ----------
for f in PL.rglob("*.rs"):
    if f.name == "lib.rs":
        continue
    f.write_text(rewrite(f.read_text(encoding="utf-8")), encoding="utf-8")
print("plugin: references rewritten")

# ---------- 5) lib.rs（v1 plugin/mod.rs 骨架 + 适配器/服务声明） ----------
v1_mod = (V1 / "core" / "plugin" / "mod.rs").read_text(encoding="utf-8")
body = "\n".join(v1_mod.split("\n")[4:])
lib = """//! RdataStation v2 插件 crate（plugin，M9）
//!
//! 插件系统核心 + 运行时适配器：
//! - 核心：dependency（依赖解析）/ events（事件总线）/ installer / loader（热加载）
//!   / manager（生命周期）/ manifest（清单）/ permission（权限）/ storage
//! - 服务：plugin_bridge（桥接）/ plugin_service（插件管理服务）
//! - Sidecar 适配器：Go Sidecar 进程管理（JSON-RPC 通信 + 驱动适配）
//! - WASM 适配器：Extism 运行时（分析/驱动/工具插件）
//!
//! 依赖方向：plugin → engine → shared。

""" + body + "\n\n" + """pub mod plugin_bridge;
pub mod plugin_service;
pub mod sidecar;
pub mod wasm;

pub use plugin_service::PluginService;
"""
(PL / "lib.rs").write_text(lib, encoding="utf-8")
print("plugin: lib.rs written")

# ---------- 6) Cargo.toml ----------
toml = """[package]
name = "rds-plugin"
version.workspace = true
edition.workspace = true
description = "M9 插件系统：Sidecar / WASM 运行时的数据库与分析插件支持"

[dependencies]
engine = { path = "../engine", package = "rds-engine" }
shared = { path = "../shared", package = "rds-shared" }

# ===== Async 运行时 =====
tokio = { version = "1.44.1", features = ["full"] }
async-trait = "0.1.88"

# ===== 序列化 =====
serde = { version = "1.0.219", features = ["derive"] }
serde_json = "1.0.135"
specta = { version = "=2.0.0-rc.25", features = ["derive", "serde_json", "chrono"] }

# ===== WASM 运行时 =====
extism = "1.21.0"

# ===== HTTP =====
reqwest = { version = "0.12.12", features = ["json", "rustls-tls"] }

# ===== 其他 =====
tracing = "0.1.41"
thiserror = "1.0.69"
uuid = { version = "1.16.0", features = ["v4", "serde"] }
chrono = { version = "0.4.39", features = ["serde"] }
"""
(PL.parent / "Cargo.toml").write_text(toml, encoding="utf-8")
print("plugin: Cargo.toml written")
print("DONE")
