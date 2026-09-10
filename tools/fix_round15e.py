# -*- coding: utf-8 -*-
import pathlib
s = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\project\src\store.rs")
t = s.read_text(encoding="utf-8")
old = "INSERT INTO connections(id, name, driver) VALUES ('conn-global-001', '供应商主数据', 'duckdb')"
new = "INSERT OR REPLACE INTO connections(id, name, driver) VALUES ('conn-global-001', '供应商主数据', 'duckdb')"
assert old in t
t = t.replace(old, new)
s.write_text(t, encoding="utf-8")
print("idempotent insert fixed")
