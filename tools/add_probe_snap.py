# -*- coding: utf-8 -*-
import pathlib
f = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\src\duckdb\snapshot.rs")
t = f.read_text(encoding="utf-8")
probe = r'''
    #[test]
    fn probe_snapshot_debug() -> Result<(), CoreError> {
        let (db_path, db_dir) = setup_test_db()?;
        println!("[probe] db_path={}", db_path.display());
        println!("[probe] db exists={}", db_path.exists());
        let manager = SnapshotManager::new(&db_path, 5)?;
        println!("[probe] snapshot_dir={}", manager.snapshot_dir().display());
        println!("[probe] dir exists={}", manager.snapshot_dir().exists());
        let info = manager.create_snapshot(&db_path, None)?;
        println!("[probe] create1 ok: {} exists={}", info.path.display(), info.path.exists());
        let info2 = manager.create_snapshot(&db_path, None)?;
        println!("[probe] create2 ok: {} exists={}", info2.path.display(), info2.path.exists());
        let all = manager.list_snapshots()?;
        println!("[probe] list len={}", all.len());
        let _ = fs::remove_dir_all(&db_dir);
        Ok(())
    }
'''
t = t.replace("#[test]\n    fn test_estimate_backup_time", probe + "\n    #[test]\n    fn test_estimate_backup_time")
f.write_text(t, encoding="utf-8")
print("probe added")
