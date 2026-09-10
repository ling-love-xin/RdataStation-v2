# -*- coding: utf-8 -*-
import pathlib
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\connection\Cargo.toml")
t = p.read_text(encoding="utf-8")
if "duckdb" not in t:
    t += '\n\n# ===== DuckDB（Secret 本地加速通道） =====\nduckdb = { version = "1.10502.0", features = ["bundled"] }\n'
    p.write_text(t, encoding="utf-8")
    print("duckdb dep added")
else:
    print("exists")
