# -*- coding: utf-8 -*-
"""RdataStation v2 迁移 Round 5：engine 迁入 persistence（SQLite/DuckDB 元数据仓储）
+ logging + migration + migrations/ SQL 资源目录。
"""
import shutil
import pathlib

V1 = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\v1\backend\src\core")
V1B = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\v1\backend")
ENG = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine")

REPL = [
    ("crate::core::persistence::", "crate::persistence::"),
    ("crate::core::logging::", "crate::logging::"),
    ("crate::core::migration::", "crate::migration::"),
    ("crate::core::driver::", "crate::driver::"),
    ("crate::core::duckdb::", "crate::duckdb::"),
    ("crate::core::error::", "shared::error::"),
    ("crate::core::models::", "shared::models::"),
    ("crate::core::stream::", "shared::stream::"),
    ("crate::core::macros::", "shared::macros::"),
    ("crate::core::utils::", "shared::utils::"),
    ("crate::core::arrow::", "shared::arrow::"),
    ("crate::core::types::", "shared::types::"),
    ("crate::core::crypto::", "shared::crypto::"),
    ("crate::core::{", "crate::{"),
]

def rewrite(text):
    for old, new in REPL:
        text = text.replace(old, new)
    return text

# ---------- 1) persistence（排除 analytics_resource_store） ----------
src_p = V1 / "persistence"
dst_p = ENG / "src" / "persistence"
dst_p.mkdir(parents=True, exist_ok=True)
for f in src_p.glob("*.rs"):
    shutil.copy2(f, dst_p / f.name)
print("engine: persistence/", len(list(src_p.glob('*.rs'))), "files copied")

# ---------- 2) logging / migration ----------
for sub in ["logging", "migration"]:
    shutil.copytree(V1 / sub, ENG / "src" / sub, dirs_exist_ok=True)
    print("engine:", sub, "copied")

# ---------- 3) migrations/ SQL 资源（include_dir! 指向 $CARGO_MANIFEST_DIR/migrations） ----------
shutil.copytree(V1B / "migrations", ENG / "migrations", dirs_exist_ok=True)
print("engine: migrations/ copied")

# ---------- 4) persistence/mod.rs 移除 analytics_resource_store（M6 归 analytics_resource crate） ----------
m = dst_p / "mod.rs"
t = m.read_text(encoding="utf-8")
t = t.replace("pub mod analytics_resource_store;\n", "")
t = t.replace("pub use analytics_resource_store::{\n    AnalyticsFolder, AnalyticsRecycleItem, AnalyticsResource, AnalyticsResourceStore, AnalyticsTag,\n    CreateFolderRequest, CreateResourceRequest, CreateTagRequest, ListResourcesOutput,\n    ResourceVersion,\n};\n", "")
m.write_text(rewrite(t), encoding="utf-8")
print("engine: persistence/mod.rs stripped analytics_resource_store")

# ---------- 5) 全部改写 ----------
for f in (ENG / "src" / "persistence").rglob("*.rs"):
    f.write_text(rewrite(f.read_text(encoding="utf-8")), encoding="utf-8")
for f in (ENG / "src" / "logging").rglob("*.rs"):
    f.write_text(rewrite(f.read_text(encoding="utf-8")), encoding="utf-8")
for f in (ENG / "src" / "migration").rglob("*.rs"):
    f.write_text(rewrite(f.read_text(encoding="utf-8")), encoding="utf-8")
print("engine: persistence/logging/migration path references rewritten")

print("DONE")
