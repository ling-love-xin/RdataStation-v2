# -*- coding: utf-8 -*-
"""Round 8 修复：残留引用 + 可见性上浮 + workbench 依赖"""
import pathlib

WB = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src")
ENG = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\src")
INS = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\insight\src")

# ---------- 1) workbench 残留引用 ----------
FIXES = [
    ("crate::core::migration::", "engine::migration::"),
    ("crate::core::insight::", "insight::"),
    ("crate::api::dto::QueryResult", "shared::models::QueryResult"),
    ("crate::services::insight_engine", "insight::insight_engine"),
    ("crate::services::result_service::TableQuality", "engine::persistence::insight_types::TableQuality"),
    ("crate::services::result_service::ColumnInsightFull", "engine::persistence::insight_types::ColumnInsightFull"),
    ("crate::services::connection_manager", "engine::connection_manager"),
    ("crate::core::", "engine::"),
]
for f in (WB / "services").rglob("*.rs"):
    t = f.read_text(encoding="utf-8")
    for old, new in FIXES:
        t = t.replace(old, new)
    f.write_text(t, encoding="utf-8")
print("workbench: leftover references fixed")

# ---------- 2) 可见性上浮 ----------
for rel in ["services/execution_service.rs", "services/sql_service.rs"]:
    p = ENG / rel
    t = p.read_text(encoding="utf-8").replace("pub(crate) ", "pub ")
    p.write_text(t, encoding="utf-8")
print("engine: execution/sql service visibility raised")

for rel in ["insight_engine.rs", "quality_scorer.rs", "table_profile_service.rs"]:
    p = INS / rel
    t = p.read_text(encoding="utf-8").replace("pub(crate) ", "pub ")
    p.write_text(t, encoding="utf-8")
print("insight: analysis services visibility raised")

# ---------- 3) result_service unused imports 清理 ----------
p = WB / "services" / "result_service.rs"
t = p.read_text(encoding="utf-8")
t = t.replace("use specta::Type;\n\n", "\n")
t = t.replace(
    """use engine::persistence::insight_types::{
    BooleanStats, ColumnInsightFull, ColumnQualityEntry, ColumnStats, ColumnStatsDetail,
    DateTimeStats, DistributionBin, ExtremeValue, NumericStats, QualityDimension, QualityScore,
    TableColumnMeta, TableProfile, TableQuality, TextFrequency, TextStats,
};
use engine::services::result_types::ResultSet;""",
    """use engine::persistence::insight_types::{
    ColumnInsightFull, ColumnStats, QualityScore, TableProfile, TableQuality,
};
use engine::services::result_types::ResultSet;""",
)
p.write_text(t, encoding="utf-8")
print("workbench: result_service imports cleaned")

print("DONE")
