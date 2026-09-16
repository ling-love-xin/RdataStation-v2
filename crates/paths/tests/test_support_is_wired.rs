//! 契约：**每个依赖 `paths` / `engine` / `shared` 的成员，都要在 `[dev-dependencies]`
//! 里打开 `paths` 的 `test-support` feature**。
//!
//! 为什么要有这条契约：数据根隔离是**构建期**的事——`test-support` 只被各成员的
//! dev-dependencies 打开，测试构建才会把数据根落到临时目录（见
//! `docs/architecture/runtime/data-paths.md` §5）。某个成员漏开，它的测试就会往
//! **产品数据根**写东西；而旧布局迁移是"只补不盖"，写进去的垃圾还会把真数据挡在门外。
//! 漏开这件事在运行时看不出来（每个测试进程只知道自己），所以用静态契约兜住——
//! 与 `crates/workbench/tests/ui_contract.rs` 同一路数。
//!
//! 注意：本文件是 integration test，`paths` 的 lib 在这种构建里**不带** `test-support`
//! （互不相干：契约只读文件，不调隔离 API）。

use std::fs;
use std::path::PathBuf;

/// `paths` 自己不需要（它的单测走 `cfg(test)`，见 `lib.rs::test_root`）。
const OWNER: &str = "paths";

/// 这类依赖意味着"能走到 `paths::*`"（经产品代码间接走到也算）。
const TOUCHES_PATHS: [&str; 3] = ["paths.workspace", "engine.workspace", "shared.workspace"];

/// 期望出现在 `[dev-dependencies]` 里的那一行。
const REQUIRED: &str = "paths = { workspace = true, features = [\"test-support\"] }";

fn workspace_crates_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/paths 的父目录是 crates/")
        .to_path_buf()
}

#[test]
fn every_member_that_can_reach_paths_enables_test_support() {
    let dir = workspace_crates_dir();
    let mut checked = 0usize;
    let mut missing: Vec<String> = Vec::new();

    let entries = fs::read_dir(&dir).expect("读 crates/ 目录");
    for entry in entries.flatten() {
        let manifest = entry.path().join("Cargo.toml");
        if !manifest.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name == OWNER {
            continue;
        }
        let text = fs::read_to_string(&manifest).expect("读 Cargo.toml");

        // 只约束"能走到 paths"的成员；其余（如 workbench_shell 这种叶子）不要求。
        if !TOUCHES_PATHS.iter().any(|needle| text.contains(needle)) {
            continue;
        }
        checked += 1;
        if !text.contains(REQUIRED) {
            missing.push(format!(
                "{name}：请在 [dev-dependencies] 加 `{REQUIRED}`（原因见本文件头）"
            ));
        }
    }

    assert!(
        checked >= 10,
        "只扫到 {checked} 个成员，路径或依赖判定可能失效（期望 10+）"
    );
    assert!(
        missing.is_empty(),
        "以下成员没有打开测试数据根隔离：\n{}",
        missing.join("\n")
    );
}
