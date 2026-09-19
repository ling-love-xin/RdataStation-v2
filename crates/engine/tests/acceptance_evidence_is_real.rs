//! 验收证据必须指向**真实存在**的用例（D10 口径的机械兜底）。
//!
//! 能力字典里 `Acceptance::verified("…")` 的文字会被界面原样展示成「已验收 · <证据>」；
//! 一旦引用的测试目标 / 用例被改名或删除，界面就在指向不存在的东西——**已验收的声称**
//! 也就随之失效（本仓的规矩：未真机验收不算可用，而“验收证据”本身也不能烂掉）。
//!
//! 本测试读工作区文件系统做核对，因此放在 `tests/`（单测看不到其他 crate 的测试目标）：
//!
//! 1. 收集“存在的名字”：所有 `crates/*/tests/*.rs` 的**文件名**，加上 `crates/**` 下
//!    所有 `fn <名>(` 的函数名（用例函数名走这一支）；
//! 2. 逐条取已验收的能力键，按 `capability::Acceptance` 的写法约定解析：
//!    去掉全角括号里的补充说明，再按 ` + ` 拆开，每个名字都要能在上面找到；
//! 3. 找不到就报错，并把“怎么修”写在消息里（改名测试 → 同步字典；或把声称降为未验收）。
//!
//! 不在本测试里做的事：**不检查用例今天是否通过**（那是跑测试的事），也不检查
//! “这条用例是否真的覆盖了该能力”（那需要人读；D10 的判据是「有可复现的用例」）。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// 工作区根（`crates/engine` 往上两级）。
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

/// 递归收集 `crates/` 下的 `.rs` 文件。
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

/// 提取 `fn <名>(` 里的函数名（够用的粗匹配：字典引用的都是测试 / 集成用例名）。
fn function_names(source: &str, out: &mut BTreeSet<String>) {
    let mut rest = source;
    while let Some(idx) = rest.find("fn ") {
        rest = &rest[idx + 3..];
        let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        let after = &rest[name.len()..];
        if !name.is_empty() && after.trim_start().starts_with('(') {
            out.insert(name);
        }
    }
}

/// 去掉全角 / 半角括号里的补充说明（约定：括号内不参与校验）。
fn strip_parenthetical(evidence: &str) -> String {
    let mut out = String::new();
    let mut depth = 0usize;
    for ch in evidence.chars() {
        match ch {
            '（' | '(' => depth += 1,
            '）' | ')' => depth = depth.saturating_sub(1),
            other if depth == 0 => out.push(other),
            _ => {}
        }
    }
    out
}

#[test]
fn every_cited_acceptance_case_exists() {
    let root = workspace_root();
    let crates_dir = root.join("crates");
    assert!(crates_dir.is_dir(), "找不到 crates 目录：{}", crates_dir.display());

    // 1) 存在的名字：测试目标文件名 + 所有函数名
    let mut known: BTreeSet<String> = BTreeSet::new();
    let mut files = Vec::new();
    rust_files(&crates_dir, &mut files);
    assert!(files.len() > 100, "扫到的源文件太少（{}）——路径解析错了吧？", files.len());
    for f in &files {
        if f.parent().and_then(|p| p.file_name()).is_some_and(|d| d == "tests") {
            if let Some(stem) = f.file_stem().and_then(|s| s.to_str()) {
                known.insert(stem.to_string());
            }
        }
        if let Ok(src) = std::fs::read_to_string(f) {
            function_names(&src, &mut known);
        }
    }

    // 2) 逐条核对已验收的键
    let mut checked = 0usize;
    for spec in rds_engine::driver::CAPABILITY_DICTIONARY {
        if !spec.acceptance.verified {
            continue;
        }
        let evidence = spec.acceptance.evidence;
        assert!(
            !evidence.trim().is_empty(),
            "{}：已验收必须给出用例名（写进 capability.rs）",
            spec.key
        );
        let stripped = strip_parenthetical(evidence);
        let mut names: Vec<&str> = stripped
            .split('+')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        assert!(
            !names.is_empty(),
            "{}：证据 `{evidence}` 里没有可校验的用例名",
            spec.key
        );
        for name in names.drain(..) {
            assert!(
                known.contains(name),
                "{}：证据里的 `{name}` 在工作区里找不到（既不是 crates/*/tests/<名>.rs，也不是任何 fn 名）\n\
                 修法二选一：① 改名的测试 → 在 capability.rs 同步成新名字；\
                 ② 用例没了 → 把该键降为 Acceptance::unverified()（未验收不算可用）\n\
                 原证据：{evidence}",
                spec.key
            );
            checked += 1;
        }
    }
    assert!(checked >= 6, "已验收的键应有若干（实得 {checked} 条名字）——字典是不是被清空了？");
    eprintln!("✅ 验收证据全部指向存在的用例（{checked} 条名字）");
}

/// 反向兜底：调用库自己的解析函数不在这里（避免测试替实现说话）——相反对 **未验收** 的键
/// 不做要求（允许留空），但已验收的键必须给出证据（上面已断言）。
#[test]
fn unverified_keys_leave_evidence_empty() {
    for spec in rds_engine::driver::CAPABILITY_DICTIONARY {
        if !spec.acceptance.verified {
            assert!(
                spec.acceptance.evidence.is_empty(),
                "{}：未验收的键不该带证据文字（界面会把它当“已验收”展示）",
                spec.key
            );
        }
    }
}
