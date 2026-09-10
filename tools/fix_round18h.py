# -*- coding: utf-8 -*-
import pathlib
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\src\duckdb\snapshot.rs")
t = p.read_text(encoding="utf-8")
old = """        let created_at = path
            .file_stem()
            .and_then(|s| s.to_string_lossy().rsplit_once("_snapshot_"))
            .and_then(|(_, sec)| sec.parse::<u64>().ok())
            .unwrap_or_else(|| {"""
new = """        let created_at = path
            .file_stem()
            .and_then(|s| {
                let name = s.to_string_lossy();
                name.rsplit_once("_snapshot_")
                    .and_then(|(_, sec)| sec.parse::<u64>().ok())
            })
            .unwrap_or_else(|| {"""
assert old in t
t = t.replace(old, new)
p.write_text(t, encoding="utf-8")
print("fixed")
