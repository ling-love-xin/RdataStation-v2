# -*- coding: utf-8 -*-
import pathlib
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\mock\Cargo.toml")
t = p.read_text(encoding="utf-8")
anchor = '# ===== Async 运行时 =====\ntokio = { version = "1.44.1", features = ["full"] }\n'
add = '# ===== DuckDB（分析引擎，mock 落临时表） =====\nduckdb = { version = "1.10502.0", features = ["bundled"] }\n'
assert anchor in t
t = t.replace(anchor, anchor + "\n" + add)
p.write_text(t, encoding="utf-8")
print("duckdb added")
