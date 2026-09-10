# -*- coding: utf-8 -*-
import pathlib, re

def show(path, pat, before, after):
    t = pathlib.Path(path).read_text(encoding="utf-8", errors="ignore")
    i = t.find(pat)
    if i < 0:
        print(f"!! {pat} NOT FOUND in {path}")
        return
    print(f"===== {path} @{pat}")
    print(t[i - before:i + after])

base = r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\src"
t = pathlib.Path(base + r"\duckdb\snapshot.rs").read_text(encoding="utf-8")
i = t.find("获取快照元数据失败")
print("=== error text ctx")
print(t[i - 500:i + 200])
show(base + r"\duckdb\snapshot.rs", "fn list_snapshots", 100, 700)
show(base + r"\duckdb\snapshot.rs", "fn setup_test_db", 400, 900)
