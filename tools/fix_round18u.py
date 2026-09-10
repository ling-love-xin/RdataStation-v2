# -*- coding: utf-8 -*-
import pathlib

# ---- 1. crypto.rs: 进程内盐缓存（消除并行测试读-写竞态） ----
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\shared\src\crypto.rs")
t = p.read_text(encoding="utf-8")

old_import = "use std::fs;\nuse std::path::PathBuf;"
new_import = "use std::fs;\nuse std::path::PathBuf;\nuse std::sync::OnceLock;"
assert old_import in t
t = t.replace(old_import, new_import)

old_fn = """/// 获取或生成安装级随机盐值，存储到文件
fn get_or_create_salt() -> Vec<u8> {
    let sp = salt_path();
    if let Ok(data) = fs::read(&sp) {
        if data.len() >= 32 {
            return data;
        }
    }

    // 生成 32 字节随机盐值
    let mut salt = vec![0u8; 32];
    OsRng.fill_bytes(&mut salt);

    if let Some(parent) = sp.parent() {
        let _ = fs::create_dir_all(parent);
        let _ = fs::write(&sp, &salt);
    }
    salt
}"""
new_fn = """/// 进程内盐缓存：避免并行测试/并发调用下多个实例各自生成盐值
/// 互相覆盖加密文件，导致一方加密后另一方解不开
static SALT_CACHE: OnceLock<Vec<u8>> = OnceLock::new();

/// 获取或生成安装级随机盐值，存储到文件
fn get_or_create_salt() -> Vec<u8> {
    SALT_CACHE
        .get_or_init(|| {
            let sp = salt_path();
            if let Ok(data) = fs::read(&sp) {
                if data.len() >= 32 {
                    return data;
                }
            }

            // 生成 32 字节随机盐值
            let mut salt = vec![0u8; 32];
            OsRng.fill_bytes(&mut salt);

            if let Some(parent) = sp.parent() {
                let _ = fs::create_dir_all(parent);
                let _ = fs::write(&sp, &salt);
            }
            salt
        })
        .clone()
}"""
assert old_fn in t
t = t.replace(old_fn, new_fn)
p.write_text(t, encoding="utf-8")
print("crypto.rs salt cache added")

# ---- 2. port_negotiation.rs: 测试断言按真实语义修正 ----
p2 = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\shared\src\port_negotiation.rs")
t2 = p2.read_text(encoding="utf-8")
old_test = """        // 高位端口应该可用
        let port = negotiator.negotiate(None)?;
        assert!(negotiator.is_port_available(port)?);

        // 分配后应该不可用
        assert!(!negotiator.is_port_available(port)?);

        negotiator.release_port(port)?;
        Ok(())"""
new_test = """        // 协商分配后端口应标记为不可用
        let port = negotiator.negotiate(None)?;
        assert!(!negotiator.is_port_available(port)?);

        // 释放后应恢复可用
        negotiator.release_port(port)?;
        assert!(negotiator.is_port_available(port)?);
        Ok(())"""
assert old_test in t2
t2 = t2.replace(old_test, new_test)
p2.write_text(t2, encoding="utf-8")
print("port_negotiation.rs test semantics fixed")
