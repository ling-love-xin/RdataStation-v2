# -*- coding: utf-8 -*-
import pathlib, re
f = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\src\persistence\metadata_cache.rs")
t = f.read_text(encoding="utf-8")
i = t.find("fn test_metadata_cache_ops")
print(t[i:i + 900])
print("--- operation query ---")
for mm in re.finditer(r'operation: "query"', t):
    j = mm.start()
    print(t[j - 300:j + 60].replace(chr(10), " | "))
