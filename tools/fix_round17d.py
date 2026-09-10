# -*- coding: utf-8 -*-
import pathlib
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\services\secret_integration.rs")
t = p.read_text(encoding="utf-8")
old = 'assert_eq!(sanitize_secret_name("连接 01"), "____01");'
new = 'assert_eq!(sanitize_secret_name("连接 01"), "___01");'
assert old in t, "pattern not found"
t = t.replace(old, new)
p.write_text(t, encoding="utf-8")
print("assert fixed")
