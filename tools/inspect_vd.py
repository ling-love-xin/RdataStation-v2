# -*- coding: utf-8 -*-
import pathlib, re
t = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\migrations\connection_metadata\004_refactor_to_normalized.sql").read_text(encoding="utf-8")
for mm in re.finditer(r"CREATE TABLE (?:IF NOT EXISTS )?view_definitions[\s\S]{0,700}?\);", t):
    print(mm.group(0)[:800])
