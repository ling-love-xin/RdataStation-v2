//! 版本标识：构建期由 crate 根 `build.rs` 注入，运行时只读。
//!
//! 用途：真机验证前核对「这个 exe 就是目标那次提交」，避免拿旧二进制验新结论。
//! 展示见 `components::version_dialog`；`BuildInfo::tag()` 是给人念的一行标识，
//! 形如 `0.2.11+1fab7d89`，工作树有未提交改动时追加 `-dirty`。
//!
//! 不知情即报不知情：无 git 检出（发布包 / 拷贝出去的目录）时各字段为 `unknown`，
//! 不猜、不填默认值。

use chrono::{DateTime, Local};

/// 读构建期注入的常量，缺失回退 `unknown`（`option_env!` 需要字面量，故用宏）。
macro_rules! build_env {
    ($key:literal) => {
        match option_env!($key) {
            Some(value) => value,
            None => "unknown",
        }
    };
}

/// 版本号：workspace `[workspace.package].version`。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const COMMIT: &str = build_env!("RDS_BUILD_COMMIT");
const BRANCH: &str = build_env!("RDS_BUILD_BRANCH");
const DIRTY: &str = build_env!("RDS_BUILD_DIRTY");
const COMMIT_TIME: &str = build_env!("RDS_BUILD_COMMIT_TIME");
const PROFILE: &str = build_env!("RDS_BUILD_PROFILE");
const TARGET: &str = build_env!("RDS_BUILD_TARGET");
const LAX_SQL: &str = build_env!("RDS_BUILD_LAX_SQL");
const GPUI_KIT: &str = build_env!("RDS_BUILD_GPUI_KIT");

/// 一次构建的完整标识。全部 `&'static str`，复制无成本。
#[derive(Clone, Debug)]
pub struct BuildInfo {
    pub version: &'static str,
    /// git 短哈希；无 git 检出时为 `unknown`。
    pub commit: &'static str,
    pub branch: &'static str,
    /// `Some(true)` 工作树有未提交改动；`Some(false)` 干净；`None` 无法判定。
    pub dirty: Option<bool>,
    /// HEAD 提交时间（Unix 秒）。
    pub commit_time: Option<i64>,
    pub profile: &'static str,
    pub target: &'static str,
    /// SQL 排版引擎的锁定版本。
    pub lax_sql: &'static str,
    /// UI 组件库的锁定版本。
    pub gpui_kit: &'static str,
}

/// 当前二进制的构建信息。
pub fn info() -> BuildInfo {
    BuildInfo {
        version: VERSION,
        commit: COMMIT,
        branch: BRANCH,
        dirty: match DIRTY {
            "1" => Some(true),
            "0" => Some(false),
            _ => None,
        },
        commit_time: COMMIT_TIME.parse::<i64>().ok(),
        profile: PROFILE,
        target: TARGET,
        lax_sql: LAX_SQL,
        gpui_kit: GPUI_KIT,
    }
}

impl BuildInfo {
    /// 基础标识：`0.2.11+1fab7d89`，不含工作树后缀。
    ///
    /// 弹窗顶部用它 + 状态徽标（`Tag`），避免"标识里带 -dirty、旁边再说一遍脏"。
    pub fn tag_base(&self) -> String {
        let mut tag = self.version.to_string();
        if self.commit != "unknown" {
            tag.push('+');
            tag.push_str(self.commit);
        }
        tag
    }

    /// 一行版本标识：`0.2.11+1fab7d89`，脏工作树加 `-dirty`，提交未知时只有版本号。
    pub fn tag(&self) -> String {
        let mut tag = self.tag_base();
        if self.dirty == Some(true) {
            tag.push_str("-dirty");
        }
        tag
    }

    /// 工作树状态的可读说法（验证时最该看的一格）。
    pub fn tree_text(&self) -> String {
        match self.dirty {
            Some(true) => "有未提交改动（与上面提交不完全一致）".to_string(),
            Some(false) => "干净（与上面提交一致）".to_string(),
            None => "unknown（非 git 检出，无法判定）".to_string(),
        }
    }

    /// HEAD 提交时间的本地可读形式；拿不到就报 unknown。
    pub fn commit_time_text(&self) -> String {
        match self
            .commit_time
            .and_then(|secs| DateTime::from_timestamp(secs, 0))
        {
            Some(utc) => utc
                .with_timezone(&Local)
                .format("%Y-%m-%d %H:%M:%S %:z")
                .to_string(),
            None => "unknown".to_string(),
        }
    }

    /// 「关于」弹窗里逐行展示的（字段名, 值）。
    pub fn rows(&self) -> Vec<(&'static str, String)> {
        vec![
            ("版本标识", self.tag()),
            ("版本号", self.version.to_string()),
            ("提交", format!("{}（{}）", self.commit, self.branch)),
            ("工作树", self.tree_text()),
            ("提交时间", self.commit_time_text()),
            ("构建档", self.profile.to_string()),
            ("目标平台", self.target.to_string()),
            ("SQL 排版引擎", format!("lax-sql {}", self.lax_sql)),
            ("UI 组件库", format!("gpui-kit {}", self.gpui_kit)),
        ]
    }

    /// 写进剪贴板的多行文本：粘到 issue / 聊天里就能对上号。
    pub fn clipboard_text(&self) -> String {
        let dirty = match self.dirty {
            Some(true) => "yes",
            Some(false) => "no",
            None => "unknown",
        };
        format!(
            "RdataStation {}\n\
             commit   {} ({})\n\
             dirty    {}\n\
             time     {}\n\
             profile  {}\n\
             target   {}\n\
             lax-sql  {}\n\
             gpui-kit {}",
            self.tag(),
            self.commit,
            self.branch,
            dirty,
            self.commit_time_text(),
            self.profile,
            self.target,
            self.lax_sql,
            self.gpui_kit,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(dirty: Option<bool>, commit: &'static str) -> BuildInfo {
        BuildInfo {
            version: "0.2.11",
            commit,
            branch: "feat/sql-format-lax",
            dirty,
            commit_time: Some(1_764_000_000),
            profile: "debug",
            target: "x86_64-pc-windows-msvc",
            lax_sql: "0.3.0",
            gpui_kit: "0.9.0",
        }
    }

    #[test]
    fn tag_reflects_dirty_tree() {
        assert_eq!(sample(Some(false), "1fab7d89").tag(), "0.2.11+1fab7d89");
        assert_eq!(
            sample(Some(true), "1fab7d89").tag(),
            "0.2.11+1fab7d89-dirty"
        );
        assert_eq!(sample(None, "unknown").tag(), "0.2.11");
        // 弹窗顶部用 tag_base + Tag 徽标，后缀只出现一次，故 base 必须不带 -dirty。
        assert_eq!(
            sample(Some(true), "1fab7d89").tag_base(),
            "0.2.11+1fab7d89"
        );
        assert_eq!(sample(None, "unknown").tag_base(), "0.2.11");
    }

    #[test]
    fn tree_text_distinguishes_unknown_from_clean() {
        assert!(sample(Some(false), "abc").tree_text().starts_with("干净"));
        assert!(sample(Some(true), "abc").tree_text().contains("未提交改动"));
        assert!(sample(None, "abc").tree_text().contains("无法判定"));
    }

    #[test]
    fn rows_and_clipboard_carry_the_key_fields() {
        let info = sample(Some(true), "1fab7d89");
        let rows = info.rows();
        assert_eq!(rows.len(), 9);
        assert!(
            rows.iter()
                .any(|(k, v)| *k == "版本标识" && v == "0.2.11+1fab7d89-dirty")
        );
        assert!(rows.iter().any(|(k, _)| *k == "工作树"));

        let text = info.clipboard_text();
        assert!(text.starts_with("RdataStation 0.2.11+1fab7d89-dirty"));
        assert!(text.contains("lax-sql  0.3.0"));
        assert!(text.contains("gpui-kit 0.9.0"));
        assert!(text.contains("dirty    yes"));
    }

    #[test]
    fn commit_time_formats_in_civil_form() {
        let text = sample(Some(false), "abc").commit_time_text();
        assert!(text.contains('-') && text.contains(':'), "got {text}");
        assert!(!text.contains("unknown"));
    }
}
