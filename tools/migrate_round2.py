# -*- coding: utf-8 -*-
"""RdataStation v2 迁移 Round 2：engine 迁入 driver（全量）+ dbi + cache + connection_manager。
自 v1/backend/src/core 复制并改写 crate 路径引用。
"""
import shutil
import pathlib

V1 = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\v1\backend\src\core")
ENG = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\src")

REPL = [
    ("crate::core::driver::", "crate::driver::"),
    ("crate::core::dbi::", "crate::dbi::"),
    ("crate::core::cache::", "crate::cache::"),
    ("crate::core::services::", "crate::connection_manager::"),
    ("crate::core::DuckDBManager", "crate::duckdb::DuckDBManager"),
    ("crate::core::error::", "shared::error::"),
    ("crate::core::models::", "shared::models::"),
    ("crate::core::stream::", "shared::stream::"),
    ("crate::core::macros::", "shared::macros::"),
    ("crate::core::utils::", "shared::utils::"),
    ("crate::core::arrow::", "shared::arrow::"),
    ("crate::core::types::", "shared::types::"),
    ("crate::core::crypto::", "shared::crypto::"),
]

def rewrite(text):
    for old, new in REPL:
        text = text.replace(old, new)
    return text

# ---------- 1) driver 全量 ----------
for sub in ["connection", "jdbc", "native", "registry", "wasm"]:
    shutil.copytree(V1 / "driver" / sub, ENG / "driver" / sub, dirs_exist_ok=True)
for f in (V1 / "driver").glob("*.rs"):
    shutil.copy2(f, ENG / "driver" / f.name)
print("engine: driver/ (top-level + connection/jdbc/native/registry/wasm) copied")

# native/duckdb.rs：删除内部 duckdb_rows_to_arrow 定义（函数已在 duckdb/row_to_arrow.rs）
nd = ENG / "driver" / "native" / "duckdb.rs"
t = nd.read_text(encoding="utf-8")
start = t.index("pub fn duckdb_rows_to_arrow")
brace = t.index("{", start)
depth = 0
i = brace
while i < len(t):
    if t[i] == "{":
        depth += 1
    elif t[i] == "}":
        depth -= 1
        if depth == 0:
            break
    i += 1
# 删除函数定义（含其后的空行），改为引用
t = t[:start] + t[i + 1:]
t = t.replace(
    "/// 将 DuckDB 行转换为 Arrow 批处理\n",
    "",
)
# 在 use 块尾部追加引用
anchor = "use crate::core::models::{ArrowBatch, QueryResult, Value};\n"
t = t.replace(
    anchor,
    anchor + "use crate::duckdb::row_to_arrow::duckdb_rows_to_arrow;\n",
)
nd.write_text(rewrite(t), encoding="utf-8")
print("engine: native/duckdb.rs dedup duckdb_rows_to_arrow")

# ---------- 2) dbi ----------
shutil.copytree(V1 / "dbi", ENG / "dbi", dirs_exist_ok=True)
print("engine: dbi/ copied")

# ---------- 3) cache ----------
shutil.copytree(V1 / "cache", ENG / "cache", dirs_exist_ok=True)
print("engine: cache/ copied")

# ---------- 4) connection_manager ----------
shutil.copy2(V1 / "services" / "connection_manager.rs", ENG / "connection_manager.rs")
print("engine: connection_manager.rs copied")

# ---------- 5) 全部改写 ----------
for f in (ENG / "driver").rglob("*.rs"):
    f.write_text(rewrite(f.read_text(encoding="utf-8")), encoding="utf-8")
for f in (ENG / "dbi").rglob("*.rs"):
    f.write_text(rewrite(f.read_text(encoding="utf-8")), encoding="utf-8")
for f in (ENG / "cache").rglob("*.rs"):
    f.write_text(rewrite(f.read_text(encoding="utf-8")), encoding="utf-8")
f = ENG / "connection_manager.rs"
f.write_text(rewrite(f.read_text(encoding="utf-8")), encoding="utf-8")
print("engine: path references rewritten")

print("DONE")
