# -*- coding: utf-8 -*-
import pathlib
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\project\Cargo.toml")
t = p.read_text(encoding="utf-8")
if "[dev-dependencies]" not in t:
    t += '\n[dev-dependencies]\n# M1 双层架构装配验证（冒烟测试）\nduckdb = { version = "1.10502.0", features = ["bundled"] }\n'
    p.write_text(t, encoding="utf-8")
    print("dev-deps added")
else:
    print("exists")
