# -*- coding: utf-8 -*-
import pathlib
root = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\migrations")
for f in root.rglob("*.sql"):
    t = f.read_text(encoding="utf-8", errors="ignore")
    if "ADD COLUMN IF NOT EXISTS" in t:
        n = t.count("ADD COLUMN IF NOT EXISTS")
        t = t.replace(
            "ADD COLUMN IF NOT EXISTS",
            "-- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS\n    ADD COLUMN",
        )
        f.write_text(t, encoding="utf-8")
        print(f.name, "fixed", n)
