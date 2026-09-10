# -*- coding: utf-8 -*-
import pathlib, sqlite3, sys, re
d = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\migrations\connection_metadata")
dbp = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\tools\_mig_probe3.sqlite")
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
        m = re.search(r'line (\d+), column (\d+)', msg)
        print("FAIL", f.name, "::", msg[:120].replace("\n", " "))
        if m:
            ln = int(m.group(1))
            lines = t.splitlines()
            print("  ctx:", " | ".join(x.strip()[:120] for x in lines[max(0, ln - 4):ln + 1]))
        conn.close()
        sys.exit(1)
conn.close()
dbp.unlink()
print("ALL OK")
