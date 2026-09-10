# -*- coding: utf-8 -*-
import pathlib

def show(path, pattern, before, after):
    t = pathlib.Path(path).read_text(encoding="utf-8", errors="ignore")
    i = t.find(pattern)
    if i < 0:
        print(f"!! {path} :: {pattern} NOT FOUND")
        return
    print(f"===== {path} @{pattern}")
    print(t[i - before:i + after])

base = r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\src"
show(base + r"\driver\native\duckdb.rs", 'conn.execute("SELECT 1"', 400, 200)
show(base + r"\duckdb\manager.rs", "fn test_read_conn_round_robin", 200, 1200)
show(base + r"\driver\native\duckdb.rs", "fn test_is_read_only_flag", 400, 800)
