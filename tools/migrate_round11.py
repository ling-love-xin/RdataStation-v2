# -*- coding: utf-8 -*-
"""RdataStation v2 迁移 Round 11：analytics_resource crate（M6 资源分析）
自 v1 core/persistence/analytics_resource_store 全量迁入（models/helpers/folder/recycle/resource/tag/version + tests）。
"""
import shutil
import pathlib

V1 = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\v1\backend\src\core\persistence\analytics_resource_store")
AR = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\analytics_resource\src")

# ---------- 1) 复制 ----------
for name in ["models.rs", "helpers.rs", "folder.rs", "recycle.rs", "resource.rs", "tag.rs", "version.rs", "tests.rs"]:
    shutil.copy2(V1 / name, AR / name)
print("analytics_resource: 8 files copied")

# ---------- 2) 改写 ----------
REPL = [
    ("crate::core::persistence::project_db::", "engine::persistence::project_db::"),
    ("crate::core::persistence::", "engine::persistence::"),
    ("crate::core::error::", "shared::error::"),
    ("crate::core::", "engine::"),
]
for f in AR.rglob("*.rs"):
    if f.name == "lib.rs":
        continue
    t = f.read_text(encoding="utf-8")
    for old, new in REPL:
        t = t.replace(old, new)
    f.write_text(t, encoding="utf-8")

# tests.rs 的 include_str 路径调整（v2 中 SQL 权威来源在 engine/migrations）
p = AR / "tests.rs"
t = p.read_text(encoding="utf-8")
t = t.replace(
    'include_str!("../../../../migrations/project_meta/007_analytics_resources.sql")',
    'include_str!("../../engine/migrations/project_meta/007_analytics_resources.sql")',
)
p.write_text(t, encoding="utf-8")
print("analytics_resource: references rewritten (incl. tests include_str)")

# ---------- 3) lib.rs（v1 mod.rs 代码骨架） ----------
v1_mod = (V1 / "mod.rs").read_text(encoding="utf-8")
lines = v1_mod.split("\n")
start = next(i for i, l in enumerate(lines) if l.startswith("use std::sync::Arc;"))
body = "\n".join(lines[start:])
lib = """//! RdataStation v2 资源分析 crate（analytics_resource，M6）
//!
//! 管理分析资源（数据源连接、DuckDB 表等）：
//! - `models`：数据模型定义
//! - `helpers`：辅助函数（时间解析等）
//! - `resource`：资源 CRUD + 分页列表 + 克隆
//! - `folder`：文件夹 CRUD + 资源关联
//! - `tag`：标签 CRUD + 双向关联查询
//! - `recycle`：回收站操作（软删除/恢复/永久删除）
//! - `version`：版本历史管理
//!
//! 依赖方向：analytics_resource → engine（persistence::project_db）→ shared。

""" + body + "\n"
(AR / "lib.rs").write_text(lib, encoding="utf-8")
print("analytics_resource: lib.rs written")

# ---------- 4) Cargo.toml ----------
toml = """[package]
name = "rds-analytics-resource"
version.workspace = true
edition.workspace = true
description = "M6 资源分析：分析资源（数据源连接/DuckDB 表）管理"

[dependencies]
engine = { path = "../engine", package = "rds-engine" }
shared = { path = "../shared", package = "rds-shared" }

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

[dev-dependencies]
uuid = { version = "1.16.0", features = ["v4", "serde"] }
"""
(AR.parent / "Cargo.toml").write_text(toml, encoding="utf-8")
print("analytics_resource: Cargo.toml written")
print("DONE")
