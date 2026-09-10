# -*- coding: utf-8 -*-
import pathlib, sqlite3, sys
d = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\migrations\connection_metadata")
dbp = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\tools\_mig_probe.sqlite")
if dbp.exists():
    dbp.unlink()
conn = sqlite3.connect(str(dbp))
for f in sorted(d.glob("*.sql")):
    t = f.read_text(encoding="utf-8")
    try:
        conn.executescript(t)
        print("OK  ", f.name)
    except Exception as e:
        msg = str(e)
        print("FAIL", f.name, "::", msg[:200].replace("\n", " "))
        # 打印失败语句附近
        conn.close()
        sys.exit(1)
conn.close()
dbp.unlink()
print("ALL OK")
