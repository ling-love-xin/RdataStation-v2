//! 构建期版本戳：把 git 状态与构建环境编进二进制。
//!
//! 目的：真机验证前能确认「这个 exe 就是目标那次提交、有没有未提交改动」，
//! 避免拿旧二进制去验证新结论。运行期读取见 `crates/workbench/src/version.rs`，
//! 展示见 `crates/workbench/src/components/version_dialog.rs`。
//!
//! 注入的常量（缺失时一律 `unknown`，不阻断构建）：
//! - `RDS_BUILD_COMMIT`        git 短哈希
//! - `RDS_BUILD_BRANCH`        当前分支名
//! - `RDS_BUILD_DIRTY`         工作树是否有未提交改动（`1` / `0` / `unknown`）
//! - `RDS_BUILD_COMMIT_TIME`   HEAD 的提交时间（Unix 秒）
//! - `RDS_BUILD_PROFILE`       debug / release
//! - `RDS_BUILD_TARGET`        目标三元组
//! - `RDS_BUILD_LAX_SQL`       `Cargo.lock` 里 `lax-sql` 的锁定版本（SQL 排版引擎）
//! - `RDS_BUILD_GPUI_KIT`      `Cargo.lock` 里 `gpui-kit` 的锁定版本（UI 框架）
use std::path::{Path, PathBuf};
use std::process::Command;

/// 跑一条 git 命令；失败（无 git / 非仓库）返回 `None`，不 panic。
fn git(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}

/// 同 [`git`]，但允许空输出（`git status --porcelain` 干净时就是空串）。
fn git_stdout(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).to_string())
}

/// 从 `Cargo.lock` 读某个包的锁定版本（状态机：`name = "x"` 后第一条 `version = "..."`）。
fn lock_version(root: &Path, package: &str) -> Option<String> {
    let lock = std::fs::read_to_string(root.join("Cargo.lock")).ok()?;
    let mut matched = false;
    for line in lock.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("name = \"") {
            matched = rest.trim_end_matches('"') == package;
            continue;
        }
        if matched {
            if let Some(rest) = line.strip_prefix("version = \"") {
                return Some(rest.trim_end_matches('"').to_string());
            }
        }
    }
    None
}

fn emit(key: &str, value: impl AsRef<str>) {
    println!("cargo:rustc-env={key}={}", value.as_ref());
}

fn main() {
    let manifest = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR 由 cargo 提供"),
    );
    let root = manifest.join("../..");

    // git 信息：commit / 分支 / 脏标记 / 提交时间。
    emit(
        "RDS_BUILD_COMMIT",
        git(&root, &["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "unknown".into()),
    );
    emit(
        "RDS_BUILD_BRANCH",
        git(&root, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_else(|| "unknown".into()),
    );
    let dirty = match git_stdout(&root, &["status", "--porcelain"]) {
        Some(out) => {
            if out.trim().is_empty() {
                "0"
            } else {
                "1"
            }
        }
        None => "unknown",
    };
    emit("RDS_BUILD_DIRTY", dirty);
    emit(
        "RDS_BUILD_COMMIT_TIME",
        git(&root, &["log", "-1", "--format=%ct"]).unwrap_or_else(|| "unknown".into()),
    );

    // 构建环境：档位与目标三元组。
    emit(
        "RDS_BUILD_PROFILE",
        std::env::var("PROFILE").unwrap_or_else(|_| "unknown".into()),
    );
    emit(
        "RDS_BUILD_TARGET",
        std::env::var("TARGET").unwrap_or_else(|_| "unknown".into()),
    );

    // 关键依赖的锁定版本：给「关于」弹窗核对排版/UI 引擎用。
    emit(
        "RDS_BUILD_LAX_SQL",
        lock_version(&root, "lax-sql").unwrap_or_else(|| "unknown".into()),
    );
    emit(
        "RDS_BUILD_GPUI_KIT",
        lock_version(&root, "gpui-kit").unwrap_or_else(|| "unknown".into()),
    );

    // 触发条件：自己、依赖锁、git 指针/索引。
    // 不加全量源码监听——源码变更本来就会重编本 crate，而 commit / add 只动 HEAD / index。
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../Cargo.lock");
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/index");
}
