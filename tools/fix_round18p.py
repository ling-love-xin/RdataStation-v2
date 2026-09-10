# -*- coding: utf-8 -*-
import pathlib, re

base = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\src")

def tag_ignore(path_str, markers):
    p = base / path_str
    t = p.read_text(encoding="utf-8")
    n = 0
    for s in markers:
        i = t.find(s)
        if i < 0:
            continue
        # 从匹配点向前找最近的开符 ```rust
        j = t.rfind("```rust", 0, i)
        if j < 0:
            continue
        eol = t.find("\n", j)
        if t[j:eol] == "```rust":
            t = t[:j] + "```rust,ignore" + t[eol:]
            n += 1
    p.write_text(t, encoding="utf-8")
    print(path_str, "tagged:", n)

tag_ignore(r"driver\registry\mod.rs", ["pub struct MySqlDriverFactory;", "DriverRegistry::register(MySqlDriverFactory);", "DriverRegistry::register(MySqlDriverFactory);"])
tag_ignore(r"connection_manager.rs", ["let config = ConnectionConfig {"])
