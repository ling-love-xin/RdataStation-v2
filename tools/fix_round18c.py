# -*- coding: utf-8 -*-
import pathlib
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\src\duckdb\manager.rs")
t = p.read_text(encoding="utf-8")
old = "    maintenance_conn: Connection,"
new = "    maintenance_conn: Option<Connection>,"
assert old in t
t = t.replace(old, new)
p.write_text(t, encoding="utf-8")
print("struct maintenance_conn -> Option")
