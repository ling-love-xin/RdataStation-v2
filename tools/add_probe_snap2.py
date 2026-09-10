# -*- coding: utf-8 -*-
import pathlib
f = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\src\duckdb\snapshot.rs")
t = f.read_text(encoding="utf-8")
old = r'''        let info2 = manager.create_snapshot(&db_path, None)?;
        println!("[probe] create2 ok: {} exists={}", info2.path.display(), info2.path.exists());
        let all = manager.list_snapshots()?;
        println!("[probe] list len={}", all.len());'''
new = r'''        let info2 = manager.create_snapshot(&db_path, None)?;
        println!("[probe] create2 ok: {} exists={}", info2.path.display(), info2.path.exists());
        let all = manager.list_snapshots()?;
        println!("[probe] list len={}", all.len());
        let _ = fs::remove_dir_all(&db_dir);
        Ok(())
    }

    #[test]
    fn probe_snapshot_cleanup() -> Result<(), CoreError> {
        let (db_path, db_dir) = setup_test_db()?;
        let manager = SnapshotManager::new(&db_path, 2)?;
        manager.create_snapshot(&db_path, None)?;
        println!("[probe] c1 done");
        std::thread::sleep(std::time::Duration::from_secs(1));
        manager.create_snapshot(&db_path, None)?;
        println!("[probe] c2 done");
        std::thread::sleep(std::time::Duration::from_secs(1));
        let info3 = manager.create_snapshot(&db_path, None)?;
        println!("[probe] c3 ok: {} exists={}", info3.path.display(), info3.path.exists());
        let all = manager.list_snapshots()?;
        for s in &all {
            println!("[probe] listed name={} created_at={}", s.name, s.created_at);
        }
        let _ = fs::remove_dir_all(&db_dir);
        Ok(())
    }
'''
assert old in t
t = t.replace(old, new)
f.write_text(t, encoding="utf-8")
print("probe2 added")
