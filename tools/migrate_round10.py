# -*- coding: utf-8 -*-
"""RdataStation v2 迁移 Round 10：scratchpad crate（M5 草稿箱）
自 v1 core/scratchpad 全量迁入（models/state/store）。
依赖极简：shared（error）+ tokio + serde + chrono。
"""
import shutil
import pathlib

V1 = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\v1\backend\src\core\scratchpad")
SC = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\scratchpad\src")

# ---------- 1) 复制 ----------
for name in ["models.rs", "state.rs", "store.rs"]:
    shutil.copy2(V1 / name, SC / name)
print("scratchpad: models/state/store copied")

# ---------- 2) 改写 ----------
REPL = [
    ("crate::core::scratchpad::", "crate::"),
    ("crate::core::error::", "shared::error::"),
    ("crate::core::", "shared::"),
]
for f in SC.rglob("*.rs"):
    if f.name == "lib.rs":
        continue
    t = f.read_text(encoding="utf-8")
    for old, new in REPL:
        t = t.replace(old, new)
    f.write_text(t, encoding="utf-8")
print("scratchpad: references rewritten")

# ---------- 3) lib.rs（v1 mod.rs 骨架） ----------
v1_mod = (V1 / "mod.rs").read_text(encoding="utf-8")
lib = """//! RdataStation v2 草稿箱 crate（scratchpad，M5）
//!
//! 类似 VS Code 文件管理的草稿工作区：
//! - `models`：草稿条目/搜索/差异/替换/外部引用模型
//! - `state`：草稿箱状态机
//! - `store`：草稿箱存储（文件系统 + 超时锁）
//!
//! 依赖方向：scratchpad → shared。

""" + v1_mod + "\n"
(SC / "lib.rs").write_text(lib, encoding="utf-8")
print("scratchpad: lib.rs written")

# ---------- 4) Cargo.toml ----------
toml = """[package]
name = "rds-scratchpad"
version.workspace = true
edition.workspace = true
description = "M5 草稿箱：类 VS Code 文件管理的草稿工作区"

[dependencies]
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
"""
(SC.parent / "Cargo.toml").write_text(toml, encoding="utf-8")
print("scratchpad: Cargo.toml written")
print("DONE")
