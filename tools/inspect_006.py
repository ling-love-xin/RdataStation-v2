# -*- coding: utf-8 -*-
import pathlib, re
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\migrations\connection_metadata\006_add_metadata_index.sql")
t = p.read_text(encoding="utf-8")
for mm in re.finditer(r"CREATE TABLE (?:IF NOT EXISTS )?sync_log[\s\S]{0,600}?\);", t):
    print(mm.group(0)[:700])
for mm in re.finditer(r"CREATE INDEX [^;]*sync_log[^;]*;", t):
    print("INDEX:", mm.group(0)[:200])
