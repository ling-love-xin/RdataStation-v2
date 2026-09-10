# -*- coding: utf-8 -*-
"""RdataStation v2 首轮迁移：shared（基础层）+ engine（DuckDB 分析引擎）。
自 v1/backend/src/core 复制并改写 crate 路径引用。
"""
import shutil
import pathlib

V1 = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\v1\backend\src\core")
V2 = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2")
SHARED = V2 / "crates" / "shared" / "src"
ENGINE = V2 / "crates" / "engine" / "src"

# ---------- 1) shared: 9 个顶层文件 + utils/ ----------
SHARED.mkdir(parents=True, exist_ok=True)
for name in [
    "api_version.rs", "arrow.rs", "crypto.rs", "error.rs", "macros.rs",
    "models.rs", "port_negotiation.rs", "stream.rs", "types.rs",
]:
    shutil.copy2(V1 / name, SHARED / name)
shutil.copytree(V1 / "utils", SHARED / "utils", dirs_exist_ok=True)
# 顶层 utils 引用改写：crate::core:: -> crate::（同 crate 内）
for f in SHARED.rglob("*.rs"):
    t = f.read_text(encoding="utf-8")
    t = t.replace("crate::core::", "crate::")
    f.write_text(t, encoding="utf-8")
print("shared: 9 files + utils/ copied, crate::core:: -> crate:: rewritten")

# ---------- 2) engine: duckdb 模块 ----------
(ENGINE / "duckdb").mkdir(parents=True, exist_ok=True)
for f in (V1 / "duckdb").glob("*.rs"):
    shutil.copy2(f, ENGINE / "duckdb" / f.name)
print("engine: duckdb/", len(list((V1 / 'duckdb').glob('*.rs'))), "files copied")

# 抽取 duckdb_rows_to_arrow（native/duckdb.rs 742-903）
native = (V1 / "driver" / "native" / "duckdb.rs").read_text(encoding="utf-8")
start = native.index("pub fn duckdb_rows_to_arrow")
brace = native.index("{", start)
depth = 0
i = brace
while i < len(native):
    if native[i] == "{":
        depth += 1
    elif native[i] == "}":
        depth -= 1
        if depth == 0:
            break
    i += 1
fn_body = native[start:i + 1]
header = (
    "//! 将 DuckDB 行转换为 Arrow 批处理\n"
    "//! TODO(migration): 自 v1 `core/driver/native/duckdb.rs` 抽取，保持原实现；\n"
    "//! 后续统一由 `shared::arrow` 承载转换能力。\n"
    "use std::sync::Arc;\n\n"
    "use arrow::array::{ArrayRef, BinaryArray, BooleanArray, Float64Array, Int64Array, StringArray};\n"
    "use arrow::datatypes::{DataType, Field, Schema};\n"
    "use arrow::record_batch::RecordBatch;\n"
    "use shared::error::{CoreError, DatabaseError};\n"
    "use shared::models::ArrowBatch;\n\n"
)
(ENGINE / "duckdb" / "row_to_arrow.rs").write_text(header + fn_body + "\n", encoding="utf-8")
print("engine: duckdb/row_to_arrow.rs extracted (", len(fn_body.splitlines()), "lines )")

# duckdb 模块内引用改写
for f in (ENGINE / "duckdb").glob("*.rs"):
    t = f.read_text(encoding="utf-8")
    t = t.replace(
        "crate::core::driver::native::duckdb::duckdb_rows_to_arrow",
        "crate::duckdb::row_to_arrow::duckdb_rows_to_arrow",
    )
    t = t.replace("crate::core::error::", "shared::error::")
    f.write_text(t, encoding="utf-8")
print("engine: duckdb/ path references rewritten")

print("DONE")
