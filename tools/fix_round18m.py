# -*- coding: utf-8 -*-
import pathlib
f = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\src\persistence\cache_version_migration.rs")
t = f.read_text(encoding="utf-8")
old = """        // 执行迁移
        let records = manager.migrate(&conn)?;
        assert_eq!(records.len(), 1);
        assert!(records[0].success);"""
new = """        // 执行迁移：从 version=1 到 CURRENT_CACHE_VERSION，每个版本策略记 1 条
        let records = manager.migrate(&conn)?;
        assert_eq!(records.len(), (CURRENT_CACHE_VERSION - 1) as usize);
        assert!(records.iter().all(|r| r.success));"""
assert old in t
t = t.replace(old, new)
f.write_text(t, encoding="utf-8")
print("assert fixed")
