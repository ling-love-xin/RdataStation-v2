# -*- coding: utf-8 -*-
import pathlib
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\connection\Cargo.toml")
t = p.read_text(encoding="utf-8")
if "thiserror" not in t:
    t = t.replace('tracing = "0.1.41"', 'tracing = "0.1.41"\nthiserror = "1.0.69"')
    p.write_text(t, encoding="utf-8")
    print("thiserror added")
else:
    print("exists")
