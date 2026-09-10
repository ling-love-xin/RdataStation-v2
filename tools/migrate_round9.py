# -*- coding: utf-8 -*-
"""RdataStation v2 迁移 Round 9：project crate（M1 项目 + 双层数据架构）
- 自 v1 core/project 迁入 models.rs / store.rs（ProjectManager 装配 SQLite 元数据 + DuckDB 分析引擎 = 双层数据架构）
- MissingDriver（驱动缺失类型）自 workbench driver_service 上移至 engine driver 层（project/workbench 双使用方）
"""
import re
import shutil
import pathlib

V1 = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\v1\backend\src\core")
ENG = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\src")
WB = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src")
PRJ = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\project\src")

# ---------- 1) MissingDriver 上移 engine ----------
ds = (WB / "services" / "driver_service.rs").read_text(encoding="utf-8")
lines = ds.split("\n")
start = next(i for i, l in enumerate(lines) if "项目打开时检测到的缺失驱动信息" in l)
end = next(i for i, l in enumerate(lines) if l.startswith("pub struct MissingDriver"))
depth, i = 0, end
while i < len(lines):
    depth += lines[i].count("{") - lines[i].count("}")
    if depth == 0:
        break
    i += 1
end_idx = i
block = "\n".join(lines[start:end_idx + 1])
header = """//! 缺失驱动信息（自 workbench driver_service 上移，project/workbench 共用）

use specta::Type;

"""
(ENG / "driver" / "missing_driver.rs").write_text(header + block + "\n", encoding="utf-8")
print("engine: driver/missing_driver.rs extracted")

# driver/mod.rs 声明
dm = (ENG / "driver" / "mod.rs").read_text(encoding="utf-8")
if "pub mod missing_driver;" not in dm:
    dm = dm.replace("pub mod traits;", "pub mod missing_driver;\npub mod traits;")
    if "pub mod missing_driver;" not in dm:
        dm = dm.replace("pub mod ", "pub mod missing_driver;\npub mod ", 1)
(ENG / "driver" / "mod.rs").write_text(dm, encoding="utf-8")
print("engine: driver/mod.rs declares missing_driver")

# workbench driver_service 删除定义 + 引用
t = ds.replace("\n".join(lines[start:end_idx + 1]), "")
t = t.replace("use shared::error::CoreError;", "use engine::driver::MissingDriver;\nuse shared::error::CoreError;")
(WB / "services" / "driver_service.rs").write_text(t, encoding="utf-8")
print("workbench: driver_service uses engine::driver::MissingDriver")

# ---------- 2) project crate 全量 ----------
for name in ["models.rs", "store.rs"]:
    shutil.copy2(V1 / "project" / name, PRJ / name)
print("project: models.rs/store.rs copied")

# ---------- 3) 改写 project 引用 ----------
REPL = [
    ("crate::core::project::models::", "crate::models::"),
    ("crate::core::project::", "crate::"),
    ("crate::core::services::driver_service::MissingDriver", "engine::driver::MissingDriver"),
    ("crate::core::migration::", "engine::migration::"),
    ("crate::core::persistence", "engine::persistence"),
    ("crate::core::error::", "shared::error::"),
    ("crate::core::", "engine::"),
]
for f in (PRJ).rglob("*.rs"):
    if f.name == "lib.rs":
        continue
    t = f.read_text(encoding="utf-8")
    for old, new in REPL:
        t = t.replace(old, new)
    f.write_text(t, encoding="utf-8")
print("project: references rewritten")

# ---------- 4) lib.rs（v1 mod.rs 骨架） ----------
v1_mod = (V1 / "project" / "mod.rs").read_text(encoding="utf-8")
body = "\n".join(v1_mod.split("\n")[6:])  # 跳过 GBK 乱码 doc 头（前 6 行）
lib = """//! RdataStation v2 项目 crate（project，M1）
//!
//! 承载双层数据架构（项目级物理隔离）：
//! - SQLite（meta/project.db）：元数据索引、事务信息
//! - DuckDB（analytics/data.duckdb）：分析数据、版本载入
//! - Config（config/*.json）：连接配置、SQL 文件
//! - 版本化支持（Versioned<T>，为 DuckLake 多人协同预留）

""" + body + "\n"
(PRJ / "lib.rs").write_text(lib, encoding="utf-8")
print("project: lib.rs written")
print("DONE")
