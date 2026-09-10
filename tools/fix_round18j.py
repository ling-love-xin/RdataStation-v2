# -*- coding: utf-8 -*-
import pathlib
f = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\src\duckdb\snapshot.rs")
t = f.read_text(encoding="utf-8")
old = """        manager.create_snapshot(&db_path, None)?;
        manager.create_snapshot(&db_path, None)?;

        let count = manager.delete_all_snapshots()?;
        assert_eq!(count, 2);"""
new = """        manager.create_snapshot(&db_path, None)?;
        // 快照文件名基于秒级时间戳，间隔 1 秒避免同名合并
        std::thread::sleep(std::time::Duration::from_secs(1));
        manager.create_snapshot(&db_path, None)?;

        let count = manager.delete_all_snapshots()?;
        assert_eq!(count, 2);"""
assert old in t, "delete_all test not found"
t = t.replace(old, new)
f.write_text(t, encoding="utf-8")
print("snapshot.rs: delete_all test sleep")
