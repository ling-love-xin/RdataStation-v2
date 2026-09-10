# -*- coding: utf-8 -*-
"""RdataStation v2 迁移 Round 13：mock crate（M7 测试数据生成）
自 v1 backend/src/mock 全量迁入（engine/error/generators/models/persistence/schema_map/templates）。
生成测试数据仅落 DuckDB 分析引擎，不回传源数据库（M7 约束由 schema_map 列映射 + engine 落临时表保证）。
"""
import shutil
import pathlib

V1 = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\v1\backend\src\mock")
MK = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\mock\src")

REPL = [
    ("crate::core::driver::native::duckdb::", "engine::driver::native::duckdb::"),
    ("crate::core::driver::", "engine::driver::"),
    ("crate::core::duckdb::", "engine::duckdb::"),
    ("crate::core::persistence::", "engine::persistence::"),
    ("crate::core::models::", "shared::models::"),
    ("crate::core::sql::", "engine::sql::"),
    ("crate::core::error::", "shared::error::"),
    ("crate::core::CoreError", "engine::CoreError"),
    ("crate::core::", "engine::"),
    ("crate::mock::", "crate::"),
]

def rewrite(text):
    for old, new in REPL:
        text = text.replace(old, new)
    return text

# ---------- 1) 复制 ----------
for name in ["engine.rs", "error.rs", "generators.rs", "models.rs",
             "persistence.rs", "schema_map.rs", "templates.rs"]:
    shutil.copy2(V1 / name, MK / name)
print("mock: 7 files copied")

# ---------- 2) 改写 ----------
for f in MK.rglob("*.rs"):
    if f.name == "lib.rs":
        continue
    f.write_text(rewrite(f.read_text(encoding="utf-8")), encoding="utf-8")
print("mock: references rewritten")

# ---------- 3) lib.rs ----------
v1_mod = (V1 / "mod.rs").read_text(encoding="utf-8")
lib = """//! RdataStation v2 测试数据生成 crate（mock，M7）
//!
//! 基于数据源元数据（schema_map 列映射）生成测试数据，仅落 DuckDB 分析引擎临时表，
//! 不回传各源数据库（M7 约束）。
//! - `engine`：MockEngine 执行管线
//! - `generators`：fake crate 驱动的各类数据生成器
//! - `models`：列定义/依赖/配置/导出模型
//! - `persistence`：生成任务与模板存储
//! - `schema_map`：源库列类型 → mock 列类型映射
//! - `templates`：场景模板
//!
//! 依赖方向：mock → engine → shared。

""" + v1_mod + "\n"
(MK / "lib.rs").write_text(lib, encoding="utf-8")
print("mock: lib.rs written")

# ---------- 4) Cargo.toml ----------
toml = """[package]
name = "rds-mock"
version.workspace = true
edition.workspace = true
description = "M7 测试数据生成：基于元数据的 mock 数据（仅落 DuckDB 分析引擎）"

[dependencies]
engine = { path = "../engine", package = "rds-engine" }
shared = { path = "../shared", package = "rds-shared" }

# ===== 数据生成 =====
fake = { version = "5", features = [
    "derive", "chrono", "uuid", "http", "ferroid", "ulid", "semver",
    "random_color", "geo", "url", "serde_json", "bigdecimal", "rust_decimal", "time",
] }
rand = "0.8"

# ===== 时间 =====
chrono = { version = "0.4.39", features = ["serde"] }

# ===== 序列化 =====
serde = { version = "1.0.219", features = ["derive"] }
serde_json = "1.0.135"
specta = { version = "=2.0.0-rc.25", features = ["derive", "serde_json", "chrono"] }

# ===== Async 运行时 =====
tokio = { version = "1.44.1", features = ["full"] }

# ===== 其他 =====
tracing = "0.1.41"
rusqlite = { version = "0.32.1", features = ["bundled", "chrono", "serde_json"] }
uuid = { version = "1.16.0", features = ["v4", "serde"] }
"""
(MK.parent / "Cargo.toml").write_text(toml, encoding="utf-8")
print("mock: Cargo.toml written")
print("DONE")
