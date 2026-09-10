# -*- coding: utf-8 -*-
import pathlib, sqlite3, sys, re
d = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\migrations\connection_metadata")
dbp = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\tools\_mig_p4.sqlite")
if dbp.exists():
    dbp.unlink()
conn = sqlite3.connect(str(dbp))
for f in sorted(d.glob("*.sql")):
    if f.name > "007_incremental_sync.sql":
        break
    t = f.read_text(encoding="utf-8")
    conn.executescript(t)
    print("OK  ", f.name)
# 逐语句跑 007
t = (d / "007_incremental_sync.sql").read_text(encoding="utf-8")
stmts = re.split(r";\s*\n", t)
for idx, s in enumerate(stmts):
    s = s.strip()
    if not s:
        continue
    try:
        conn.execute(s + ";")
    except Exception as e:
        print("STMT", idx, "FAIL ::", str(e)[:100])
        print("   ", s[:200].replace("\n", " "))
        break
conn.close()
dbp.unlink()
