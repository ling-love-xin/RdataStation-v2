# -*- coding: utf-8 -*-
import pathlib, sqlite3, re
d = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\migrations\connection_metadata")
dbp = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\tools\_mig_p5.sqlite")
if dbp.exists():
    dbp.unlink()
conn = sqlite3.connect(str(dbp))
for f in sorted(d.glob("*.sql")):
    if f.name >= "007_incremental_sync.sql":
        break
    conn.executescript(f.read_text(encoding="utf-8"))
    print("OK  ", f.name)
t = (d / "007_incremental_sync.sql").read_text(encoding="utf-8")
# 保留注释但按 ; 切分（简单切分）
stmts = re.split(r";\s*\n", t)
for idx, s in enumerate(stmts):
    s = s.strip()
    if not s:
        continue
    try:
        conn.execute(s + ";")
    except Exception as e:
        print("STMT", idx, "FAIL ::", str(e)[:100])
        print("   ", s[:220].replace("\n", " "))
        break
else:
    print("007 all stmts OK")
conn.close()
dbp.unlink()
