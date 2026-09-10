# -*- coding: utf-8 -*-
import pathlib
t = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\src\duckdb\snapshot.rs").read_text(encoding="utf-8")
i = t.find("pub fn create_snapshot")
j = t.find("pub fn delete_all_snapshots")
print(t[i:j])
