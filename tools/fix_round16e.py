# -*- coding: utf-8 -*-
import pathlib

# 1) secret.rs 测试改 POSTGRES
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\connection\src\secret.rs")
t = p.read_text(encoding="utf-8")
t = t.replace('secret_type: SECRET_TYPE_DUCKDB.to_string(),', 'secret_type: SECRET_TYPE_POSTGRES.to_string(),')
p.write_text(t, encoding="utf-8")
print("secret tests -> POSTGRES")

# 2) 移除 probe
probe = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\connection\src\probe.rs")
if probe.exists():
    probe.unlink()
    print("probe.rs removed")
lib = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\connection\src\lib.rs")
t = lib.read_text(encoding="utf-8")
t = t.replace("pub mod probe;\n", "")
lib.write_text(t, encoding="utf-8")
print("probe unmounted")
