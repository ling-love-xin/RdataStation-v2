# -*- coding: utf-8 -*-
"""RdataStation v2 迁移 Round 6：engine 第四轮
迁入 SQL 执行基础设施：
- services: sql_service / sql_parser_service / duckdb_service / execution_service / snapshot_service
- core/sql（builder/engine/formatter/parser/transpiler，基于 sqlglot_rust）
抽取 ResultSet（自 result_service.rs 14-22 行）到 engine/src/services/result_types.rs，
供 duckdb_service / execution_service 引用，解除对 result_service 的依赖。
"""
import shutil
import pathlib

V1 = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\v1\backend\src\core")
ENG = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\src")

REPL = [
    ("crate::core::persistence::", "crate::persistence::"),
    ("crate::core::services::connection_manager::", "crate::connection_manager::"),
    ("crate::core::services::duckdb_service::", "crate::services::duckdb_service::"),
    ("crate::core::services::sql_service::", "crate::services::sql_service::"),
    ("crate::core::services::result_service::", "crate::services::result_types::"),
    ("crate::core::services::", "crate::services::"),
    ("crate::core::models::", "shared::models::"),
    ("crate::core::error::", "shared::error::"),
    ("crate::core::cache::", "crate::cache::"),
    ("crate::core::dbi::", "crate::dbi::"),
    ("crate::core::driver::", "crate::driver::"),
    ("crate::core::sql::", "crate::sql::"),
    ("crate::core::get_connection_manager", "crate::get_connection_manager"),
    ("crate::core::DuckDBManager", "crate::DuckDBManager"),
    ("crate::core::{", "crate::{"),
]

def rewrite(text):
    for old, new in REPL:
        text = text.replace(old, new)
    return text

# ---------- 1) services 5 文件 ----------
dst_s = ENG / "services"
dst_s.mkdir(parents=True, exist_ok=True)
for name in ["sql_service.rs", "sql_parser_service.rs", "duckdb_service.rs",
             "execution_service.rs", "snapshot_service.rs"]:
    shutil.copy2(V1 / "services" / name, dst_s / name)
print("engine: services/ 5 files copied")

# ---------- 2) core/sql 全量 ----------
shutil.copytree(V1 / "sql", ENG / "sql", dirs_exist_ok=True)
print("engine: sql/ copied")

# ---------- 3) 抽取 ResultSet 到 services/result_types.rs ----------
rs = (V1 / "services" / "result_service.rs").read_text(encoding="utf-8")
lines = rs.split("\n")
start = next(i for i, l in enumerate(lines) if l.startswith("pub struct ResultSet"))
depth, i = 0, start
while i < len(lines):
    depth += lines[i].count("{") - lines[i].count("}")
    if depth == 0:
        break
    i += 1
body = "\n".join(lines[start:i + 1])
header = """//! 结果集类型（占位于此）
//! TODO(migration): 自 v1 `core/services/result_service.rs` 抽取，
//! 供 engine 内部 sql/duckdb/execution 服务引用；随 workbench 轮次归属确认。

use specta::Type;

"""
(dst_s / "result_types.rs").write_text(header + body + "\n", encoding="utf-8")
print("engine: services/result_types.rs extracted", len(body.splitlines()), "lines")

# ---------- 4) 改写引用 ----------
for f in (ENG / "services").rglob("*.rs"):
    f.write_text(rewrite(f.read_text(encoding="utf-8")), encoding="utf-8")
for f in (ENG / "sql").rglob("*.rs"):
    f.write_text(rewrite(f.read_text(encoding="utf-8")), encoding="utf-8")
print("engine: services/sql path references rewritten")

print("DONE")
