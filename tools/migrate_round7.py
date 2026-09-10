# -*- coding: utf-8 -*-
"""RdataStation v2 迁移 Round 7：insight crate（M8）
- 规则引擎：v1 core/insight 全量（rule_executor/rule_registry/rule_types/schema_analyzer/mod.rs 逻辑）
- 分析服务：v1 core/services 的 insight_engine / quality_scorer / table_profile_service
- 资产：v1 backend/insight-rules（内置规则 .rule.toml，include_dir 编译时嵌入）
"""
import shutil
import pathlib

V1 = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\v1\backend\src\core")
V1B = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\v1\backend")
INS = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\insight")

REPL = [
    ("crate::core::insight::", "crate::"),
    ("crate::core::services::duckdb_service::", "engine::services::duckdb_service::"),
    ("crate::core::services::sql_service::", "engine::services::sql_service::"),
    ("crate::core::services::result_service::", "engine::persistence::insight_types::"),
    ("crate::core::services::SqlService", "engine::SqlService"),
    ("crate::core::services::insight_engine::", "crate::insight_engine::"),
    ("crate::core::services::", "engine::services::"),
    ("crate::core::persistence::", "engine::persistence::"),
    ("crate::core::models::", "shared::models::"),
    ("crate::core::error::", "shared::error::"),
    ("crate::core::driver::", "engine::driver::"),
    ("crate::core::duckdb::", "engine::duckdb::"),
    ("crate::core::get_connection_manager", "engine::get_connection_manager"),
    ("crate::core::DuckDBManager", "engine::DuckDBManager"),
]

def rewrite(text):
    for old, new in REPL:
        text = text.replace(old, new)
    return text

# ---------- 1) 规则引擎 4 文件 ----------
for name in ["rule_executor.rs", "rule_registry.rs", "rule_types.rs", "schema_analyzer.rs"]:
    shutil.copy2(V1 / "insight" / name, INS / "src" / name)
print("insight: rule engine 4 files copied")

# ---------- 2) 分析服务 3 文件 ----------
for name in ["insight_engine.rs", "quality_scorer.rs", "table_profile_service.rs"]:
    shutil.copy2(V1 / "services" / name, INS / "src" / name)
print("insight: analysis services 3 files copied")

# ---------- 3) insight-rules 资产 ----------
shutil.copytree(V1B / "insight-rules", INS / "insight-rules", dirs_exist_ok=True)
print("insight: insight-rules/ copied")

# ---------- 4) 改写 ----------
for f in (INS / "src").rglob("*.rs"):
    f.write_text(rewrite(f.read_text(encoding="utf-8")), encoding="utf-8")
print("insight: references rewritten")

# ---------- 5) lib.rs（以 v1 mod.rs 逻辑为骨架） ----------
lib = (INS / "src" / "lib.rs").read_text(encoding="utf-8")
body = """//! RdataStation v2 洞察与规则引擎 crate（insight）
//!
//! 承载 M8 洞察模块：
//! - 规则引擎（`rule_*`，自 v1 `core/insight`）：内置/用户规则执行、注册表、热加载
//! - 分析服务（`insight_engine` / `quality_scorer` / `table_profile_service`，自 v1 `core/services`）
//! - 内置规则资产 `insight-rules/`（include_dir 编译时嵌入）
//!
//! 依赖方向：insight → engine → shared（不依赖任何业务 Feature crate）。

""" + lib
(INS / "src" / "lib.rs").write_text(body, encoding="utf-8")
print("insight: lib.rs header prepended")
print("DONE")
