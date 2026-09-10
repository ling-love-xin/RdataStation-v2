# -*- coding: utf-8 -*-
import pathlib
f = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\src\duckdb\snapshot.rs")
t = f.read_text(encoding="utf-8")

# 1) from_path：created_at 优先从快照文件名解析（Windows copy 保留源 mtime 导致排序失效）
old = """        let created_at = metadata
            .modified()
            .unwrap_or(UNIX_EPOCH)
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();"""
new = """        // 优先从快照文件名解析创建时间（格式：<db_stem>_snapshot_<unix_secs>.duckdb）。
        // 注意：Windows 上 std::fs::copy 会保留源文件 mtime，同一源的多个快照 mtime
        // 相同，会导致 cleanup 排序失效误删最新快照；文件名时间戳是创建时刻，更可靠。
        let created_at = path
            .file_stem()
            .and_then(|s| s.to_string_lossy().rsplit_once("_snapshot_"))
            .and_then(|(_, sec)| sec.parse::<u64>().ok())
            .unwrap_or_else(|| {
                metadata
                    .modified()
                    .unwrap_or(UNIX_EPOCH)
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
            });"""
assert old in t, "from_path created_at not found"
t = t.replace(old, new)

# 2) 移除探针测试
i = t.find("    #[test]\n    fn probe_snapshot_debug")
j = t.find("    #[test]\n    fn test_estimate_backup_time")
assert i > 0 and j > i, f"probe block not found i={i} j={j}"
t = t[:i] + t[j:]

f.write_text(t, encoding="utf-8")
print("snapshot.rs: filename timestamp + probes removed")
