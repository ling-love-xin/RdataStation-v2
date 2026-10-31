//! DuckDB 文件打开助手
//!
//! 自 engine `duckdb::manager::DuckDBManager::open_file_with_retry` 下沉
//! （2026-10-29，engine 瘦身第二刀）：rds-store 的全局 DuckDB 长连接需要它，
//! 但 store 不能依赖 engine（会成环），故放 shared（engine 与 store 两侧共用）。
//!
//! 除打开文件本身，这里还提供 [`assert_native_format`] ——**格式守门**。只要打开
//! DuckDB 分析库，就该先过这一道，理由见该函数的注释。

use std::path::{Path, PathBuf};

use duckdb::Connection;

use crate::error::{CommonError, CoreError};

/// DuckDB 存储头的魔数。注意它**不在偏移 0**：前 8 字节是校验和。
const DUCKDB_MAGIC: [u8; 4] = *b"DUCK";
const DUCKDB_MAGIC_OFFSET: usize = 8;
/// 能检出魔数所需的最小文件长度
const DUCKDB_HEADER_MIN_LEN: usize = DUCKDB_MAGIC_OFFSET + DUCKDB_MAGIC.len();

/// 校验一个**已存在**的 DuckDB 文件是原生格式，而不是别的引擎写出来的同名文件。
///
/// ## 为什么需要这道闸
///
/// DuckDB 内置 SQLite 存储后端（`duckdb_databases().type = 'sqlite'`），因此一个由
/// rusqlite 建出的 `analytics.duckdb` **照样能被 `Connection::open` 打开**。也就是说
/// 「能打开」从来不等于「格式对」——项目早年正是只断言了「能打开」，让这个缺陷
/// 逃过了一整套测试，之后才在 store / project 的**测试**里补了魔数断言。
///
/// 治理（2026-11-01）：把断言抽到这里并接到生产打开路径上——盖子此前只盖在测试机上。
///
/// ## 判定口径
///
/// | 情形 | 结果 | 理由 |
/// | --- | --- | --- |
/// | 文件不存在 | 放行 | 马上要新建，没有「历史格式错误」可言 |
/// | 长度 0 | 放行 | 占位文件 / 上次建到一半，DuckDB 会自己写头 |
/// | 长度 < 12 字节 | 拒绝 | 连头都写不全，必是损坏或被别的程序改写 |
/// | 偏移 8..12 ≠ `DUCK` | 拒绝 | 非原生格式（最常见元凶：rusqlite 建的同名库） |
pub fn assert_native_format(path: &Path) -> Result<(), CoreError> {
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => {
            return Err(CoreError::common(CommonError::General(format!(
                "读取 DuckDB 文件元信息失败 {}: {}",
                path.display(),
                e
            ))))
        }
    };
    // 目录 / 符号链接等特殊形态交给下游 `Connection::open` 报错，这里不越权。
    if !meta.is_file() {
        return Ok(());
    }
    if meta.len() == 0 {
        return Ok(());
    }
    if meta.len() < DUCKDB_HEADER_MIN_LEN as u64 {
        return Err(reject(path, &[]));
    }

    use std::io::Read as _;
    let mut head = [0u8; DUCKDB_HEADER_MIN_LEN];
    let mut file = std::fs::File::open(path).map_err(|e| {
        CoreError::common(CommonError::General(format!(
            "打开 DuckDB 文件头失败 {}: {}",
            path.display(),
            e
        )))
    })?;
    file.read_exact(&mut head).map_err(|e| {
        CoreError::common(CommonError::General(format!(
            "读取 DuckDB 文件头失败 {}: {}",
            path.display(),
            e
        )))
    })?;

    if head[DUCKDB_MAGIC_OFFSET..] != DUCKDB_MAGIC {
        return Err(reject(path, &head));
    }
    Ok(())
}

/// 统一的拒绝错误：把「看到的头部」摆出来，方便一眼认出元凶。
fn reject(path: &Path, head: &[u8]) -> CoreError {
    let preview: String = head
        .iter()
        .take(16)
        .map(|b| {
            if b.is_ascii_graphic() {
                format!("{}", *b as char)
            } else {
                format!("\\x{b:02x}")
            }
        })
        .collect();
    CoreError::common(CommonError::General(format!(
        "{} 不是 DuckDB 原生格式（偏移 {} 处应为魔数 {:?}）{}；\
         常见成因是该文件被 SQLite 系驱动抢先创建。请删除或改用新路径后重试。",
        path.display(),
        DUCKDB_MAGIC_OFFSET,
        std::str::from_utf8(&DUCKDB_MAGIC).unwrap_or("DUCK"),
        if head.is_empty() {
            "：文件长度不足以容纳文件头".to_string()
        } else {
            format!("，实际头部为 \"{preview}\"")
        }
    )))
}

/// 打开 DuckDB 文件（自动创建父目录；失败带路径上下文的 CoreError）
///
/// 与 engine `DuckDBManager::ensure_parent_dir` 语义一致：父目录不存在则建，
/// 建不成就报带路径的错误，避免 `Connection::open` 的裸错误难定位。
///
/// 打开前先过 [`assert_native_format`]：已存在但非原生格式的文件在这里就被拦下，
/// 不会等到某个 SQL 执行失败才以难懂的形式暴露。
pub fn open_file_with_retry(path: &str) -> Result<Connection, CoreError> {
    let path = PathBuf::from(path);
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| {
                CoreError::common(CommonError::General(format!(
                    "创建目录失败 {:?}: {}",
                    parent, e
                )))
            })?;
        }
    }
    assert_native_format(&path)?;
    Connection::open(&path).map_err(|e| {
        CoreError::common(CommonError::General(format!(
            "打开 DuckDB 文件失败 {}: {}",
            path.display(),
            e
        )))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rds-duckdb-format-{}-{name}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("建临时目录");
        dir.join("analytics.duckdb")
    }

    /// 文件不存在 → 放行（马上要新建）
    #[test]
    fn missing_file_is_allowed() {
        let path = scratch("missing");
        let _ = std::fs::remove_file(&path);
        assert_native_format(&path).expect("不存在的文件应放行");
    }

    /// 长度 0 → 放行（占位文件，DuckDB 会自己写头）
    #[test]
    fn empty_file_is_allowed() {
        let path = scratch("empty");
        std::fs::write(&path, b"").expect("写空文件");
        assert_native_format(&path).expect("0 字节文件应放行");
    }

    /// 原生格式 → 放行。这里真的让 DuckDB 建一次库，避免 Mock 出假绿。
    #[test]
    fn native_file_is_allowed() {
        let path = scratch("native");
        let _ = std::fs::remove_file(&path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("建父目录");
        }
        {
            let conn = Connection::open(&path).expect("建原生库");
            conn.execute_batch("CREATE TABLE t (a INTEGER);").expect("建表");
        }
        assert_native_format(&path).expect("原生格式应放行");
    }

    /// SQLite 抢先建出的同名文件 → 拒绝。这正是历史上逃过整套测试的那个缺陷。
    #[test]
    fn sqlite_header_is_rejected() {
        let path = scratch("sqlite");
        let head = b"SQLite format 3\0 and then some padding bytes".to_vec();
        std::fs::write(&path, head).expect("写 SQLite 头");
        let err = assert_native_format(&path).expect_err("SQLite 头必须被拒");
        let msg = format!("{err}");
        assert!(msg.contains("不是 DuckDB 原生格式"), "错误信息应说明原因：{msg}");
        assert!(msg.contains("SQLite"), "错误信息应点出元凶：{msg}");
    }

    /// 连文件头都写不全 → 拒绝
    #[test]
    fn truncated_header_is_rejected() {
        let path = scratch("truncated");
        std::fs::write(&path, b"DU").expect("写过短文件");
        assert_native_format(&path).expect_err("不足 12 字节必须被拒");
    }
}
