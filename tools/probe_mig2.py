# -*- coding: utf-8 -*-
import pathlib, sqlite3, sys
d = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\migrations\connection_metadata")
dbp = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\tools\_mig_probe2.sqlite")
if dbp.exists():
    dbp.unlink()
conn = sqlite3.connect(str(dbp))
for f in sorted(d.glob("*.sql")):
    t = f.read_text(encoding="utf-8")
    try:
        conn.executescript(t)
        print("OK  ", f.name)
    except Exception as e:
        print("FAIL", f.name, "::", str(e)[:150].replace("\n", " "))
        break
# 检查所有表结构
cur = conn.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
for (name,) in cur.fetchall():
    if "fts" in name:
        continue
    cols = conn.execute(f"PRAGMA table_info({name})").fetchall()
    cnames = [c[1] for c in cols]
    print(f"TABLE {name}: {cnames}")
conn.close()
dbp.unlink()
