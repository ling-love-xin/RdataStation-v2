# -*- coding: utf-8 -*-
import pathlib

MK = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\mock")

# 1) 修正 duckdb_rows_to_arrow 路径
for f in (MK / "src").rglob("*.rs"):
    t = f.read_text(encoding="utf-8")
    if "engine::driver::native::duckdb::duckdb_rows_to_arrow" in t:
        t = t.replace("engine::driver::native::duckdb::duckdb_rows_to_arrow",
                      "engine::duckdb::row_to_arrow::duckdb_rows_to_arrow")
        f.write_text(t, encoding="utf-8")
        print("fixed path:", f.name)

# 2) 补 thiserror
p = MK / "Cargo.toml"
t = p.read_text(encoding="utf-8")
if "thiserror" not in t:
    t = t.replace("tracing = \"0.1.41\"", "tracing = \"0.1.41\"\nthiserror = \"1.0.69\"")
    p.write_text(t, encoding="utf-8")
    print("thiserror added")
print("DONE")
