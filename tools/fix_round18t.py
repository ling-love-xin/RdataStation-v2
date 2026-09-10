# -*- coding: utf-8 -*-
import pathlib
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\plugin\src\manifest.rs")
t = p.read_text(encoding="utf-8")
old = """    fn write_temp_toml(content: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir();
        let path = dir.join("test_rdata_plugin.toml");
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(content.as_bytes()).unwrap();
        path
    }"""
new = """    // v1 缺陷修复：固定临时文件名导致并行测试互相覆盖（读到他测的文件内容，
    // 使 parse 结果不定）；改用进程内原子计数器保证唯一
    static TEMP_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    fn write_temp_toml(content: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir();
        let seq = TEMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let path = dir.join(format!(
            "test_rdata_plugin_{}_{}.toml",
            std::process::id(),
            seq
        ));
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(content.as_bytes()).unwrap();
        path
    }"""
assert old in t
t = t.replace(old, new)
p.write_text(t, encoding="utf-8")
print("write_temp_toml fixed")
