//! 代码里的“依赖源码依据”必须指向**当前锁定**的依赖版本（版本引用不能烂掉）。
//!
//! 本仓多处按“读到哪一行”立论——尤其是驱动侧的事实（`property_spec` 的允许键清单、
//! `url_params::append_ssl_params` 的 TLS 参数词汇、对话框架构里的 TLS 档位表）：
//! 这些结论是**某个版本的客户端库**的事实，升级依赖后可能就不再成立，而没人会记得回来复查。
//!
//! 本测试把“代码里的 `<crate>-<版本>` 引用”与 `Cargo.lock` 绑上：
//!
//! 1. 扫 `crates/**/*.rs`，取出形如 `sqlx-mysql-0.9.0` / `tokio-postgres-0.7.18` 的 token（只认受关注的那几个 crate）；
//! 2. 逐个与 `Cargo.lock` 里该 crate 的实际版本比对；
//! 3. 不一致 → 报出**文件:行号**，并提醒「升依赖后请复读源码并同步引用与结论」。
//!
//! 它**不**校验源码行号（那需要 CARGO_HOME 里的源码，CI 上可能没有）——行号只在人复查时用；
//! 本测试管的是“版本号对不对”这一层，也就是“要不要重新复查”的扳机。
//!
//! ## 为什么还要读插件的 lock（D3，2026-11-12）
//!
//! `sqlx` 的实现整体搬到 sidecar 插件后，根 `Cargo.lock` 里**不再有** sqlx 家族（内核一行
//! sqlx 代码都不剩）。而 `crates/` 里仍按 sqlx 源码立论的结论（`property_spec` 的「静默忽略
//! 未知键」、`url_params` 的 TLS 参数词汇）**依然成立**——只是客户端库现在由插件消费
//! （`tools/plugin-drivers/sqlx-rust/Cargo.lock` 才是它真正的版本锁）。所以这里把插件
//! 的 lock 一并读进来：引用照旧钉版本，钉的是「实际使用它的那份锁」。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// 受关注（代码里引其源码、行号立论）的依赖。新增这类引用时把 crate 名补进来。
const CITED_CRATES: [&str; 7] = [
    "mysql_async",
    "tokio-postgres",
    "sqlx",
    "sqlx-mysql",
    "sqlx-postgres",
    "rusqlite",
    "duckdb",
];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

/// `Cargo.lock` 里各 crate 的版本（同名多版本时取第一个，够用：本仓不会同时锁两个大版本）。
fn locked_versions(lock: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut name: Option<String> = None;
    for line in lock.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("name = \"") {
            name = rest.strip_suffix('"').map(str::to_string);
        } else if let Some(rest) = t.strip_prefix("version = \"") {
            if let (Some(n), Some(v)) = (name.take(), rest.strip_suffix('"')) {
                out.entry(n).or_insert_with(|| v.to_string());
            }
        }
    }
    out
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// 行里出现的 `<crate>-<x.y.z>` 引用（crate 名取自 [`CITED_CRATES`]，避免把无关字符串当版本）。
fn citations_in(line: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for crate_name in CITED_CRATES {
        let needle = format!("{crate_name}-");
        let mut rest = line;
        while let Some(idx) = rest.find(&needle) {
            rest = &rest[idx + needle.len()..];
            let version: String = rest
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            // 版本至少 x.y 才算引用（避免把 `mysql_async-` 后面接的普通词当版本）
            if version.matches('.').count() >= 1
                && version.starts_with(|c: char| c.is_ascii_digit())
            {
                out.push((crate_name.to_string(), version));
            }
        }
    }
    out
}

#[test]
fn cited_dependency_versions_match_the_lockfile() {
    let root = workspace_root();
    let mut lock_paths = vec![root.join("Cargo.lock")];
    // 独立 workspace 的插件各有自己的 lock：`crates/` 里按 sqlx 源码立论的结论由它们消费。
    let plugins = root.join("tools").join("plugin-drivers");
    if let Ok(entries) = std::fs::read_dir(&plugins) {
        for entry in entries.flatten() {
            let lock = entry.path().join("Cargo.lock");
            if lock.is_file() {
                lock_paths.push(lock);
            }
        }
    }

    let mut locked: BTreeMap<String, String> = BTreeMap::new();
    for path in &lock_paths {
        let lock = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("读不到 {}：{e}", path.display()));
        for (k, v) in locked_versions(&lock) {
            locked.entry(k).or_insert(v); // 根 lock 优先；同一 crate 多版本时取先出现者
        }
    }

    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    assert!(
        files.len() > 100,
        "扫到的源文件太少（{}）——路径解析错了吧？",
        files.len()
    );

    let mut checked = 0usize;
    let mut mismatches: Vec<String> = Vec::new();
    for file in &files {
        let Ok(src) = std::fs::read_to_string(file) else {
            continue;
        };
        for (idx, line) in src.lines().enumerate() {
            for (crate_name, cited) in citations_in(line) {
                let Some(actual) = locked.get(&crate_name) else {
                    mismatches.push(format!(
                        "{}:{} 引用了 {crate_name}-{cited}，但 Cargo.lock 里没有这个 crate",
                        file.display(),
                        idx + 1
                    ));
                    continue;
                };
                // 引用可能是 `0.9`（前缀写法）——允许锁定版本以其为前缀
                if actual == &cited || actual.starts_with(&format!("{cited}.")) {
                    checked += 1;
                } else {
                    mismatches.push(format!(
                        "{}:{} 引用 {crate_name}-{cited}，而锁定的版本是 {actual}",
                        file.display(),
                        idx + 1
                    ));
                    checked += 1;
                }
            }
        }
    }

    assert!(
        mismatches.is_empty(),
        "依赖源码引用与 Cargo.lock 不一致（{} 处）：\n{}\n\
         修法：升依赖后**复读源码**（允许键清单 / TLS 参数词汇 / 行号引用），\
         把结论与版本号一起更新——只改版本号不改结论是自欺。",
        mismatches.len(),
        mismatches.join("\n")
    );
    assert!(
        checked > 0,
        "一处依赖源码引用都没扫到？——是不是把引用写法改掉了（预期形如 `sqlx-mysql-0.9.0/src/...`）"
    );
    eprintln!("✅ 依赖源码引用与 Cargo.lock 一致（{checked} 处）");
}
