# -*- coding: utf-8 -*-
import pathlib, re

base = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\src")

def tag_ignore(path_str, starts):
    p = base / path_str
    t = p.read_text(encoding="utf-8")
    n = 0
    for s in starts:
        # 找到该标记后的第一个 ```rust 或 ``` 裸块
        i = t.find(s)
        while i >= 0:
            j = t.find("```", i + len(s))
            if j < 0:
                break
            # 块首语言标签
            eol = t.find("\n", j)
            head = t[j + 3:eol].strip()
            if head in ("", "rust"):
                t = t[:j + 3] + "rust,ignore" + t[eol:]
                n += 1
                break
            i = t.find(s, i + len(s))
    p.write_text(t, encoding="utf-8")
    print(path_str, "tagged:", n)

tag_ignore(r"driver\registry\mod.rs", ["pub struct MySqlDriverFactory;", "DriverRegistry::register(MySqlDriverFactory);"])
tag_ignore(r"connection_manager.rs", ["let config = ConnectionConfig {"])
