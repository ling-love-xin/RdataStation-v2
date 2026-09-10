# -*- coding: utf-8 -*-
import pathlib

def show(path, pat, before, after):
    t = pathlib.Path(path).read_text(encoding="utf-8", errors="ignore")
    i = t.find(pat)
    if i < 0:
        print(f"!! {pat} NOT FOUND in {path}")
        return
    print(f"===== {path} @{pat}")
    print(t[i - before:i + after])

base = r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\src"
show(base + r"\duckdb\snapshot.rs", "fn setup_test_db", 100, 600)
show(base + r"\duckdb\snapshot.rs", "pub fn create_snapshot", 100, 900)
show(base + r"\duckdb\import_export.rs", "pub fn validate_file_path", 300, 300)
