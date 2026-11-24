//! 版本标识：构建期由 crate 根 `build.rs` 注入，运行时只读。
//!
//! 用途：真机验证前核对「这个 exe 就是目标那次提交」，避免拿旧二进制验新结论。
//! 展示见 `components::version_dialog`；`BuildInfo::tag()` 是给人念的一行标识，
//! 形如 `0.2.11+1fab7d89`，工作树有未提交改动时追加 `-dirty`。
//!
//! 不知情即报不知情：无 git 检出（发布包 / 拷贝出去的目录）时各字段为 `unknown`，
//! 不猜、不填默认值。唯一的例外是 SQLite 引擎版本——它编在二进制里，由
//! [`rusqlite::version()`] 直接返回 `SQLITE_VERSION` 常量，永远是确切值。
//!
//! 字段口径见 `docs/architecture/ui/version-info-page-design.md`（14 项）。

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
const SQLGLOT: &str = build_env!("RDS_BUILD_SQLGLOT");
const FAKE: &str = build_env!("RDS_BUILD_FAKE");
const GPUI_FAST: &str = build_env!("RDS_BUILD_GPUI_FAST");
const DUCKDB: &str = build_env!("RDS_BUILD_DUCKDB");
const RUSQLITE: &str = build_env!("RDS_BUILD_RUSQLITE");

/// 一次构建的完整标识。除 `commit_time` 外全是 `&'static str`，复制无成本。
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
    /// SQL 语义引擎（`sqlglot-rust`）的锁定版本。
    pub sqlglot: &'static str,
    /// SQL 排版引擎（`lax-sql`）的锁定版本。
    pub lax_sql: &'static str,
    /// UI 组件库（`gpui-kit`）的锁定版本。
    pub gpui_kit: &'static str,
    /// 渲染内核（`gpui-fast`）的锁定版本。
    pub gpui_fast: &'static str,
    /// DuckDB 的 crate 版本（形如 `1.10506.1`，**不是**引擎版本）。
    pub duckdb: &'static str,
    /// SQLite 的 crate 版本（`rusqlite`）。
    pub rusqlite: &'static str,
    /// Mock 数据（`fake`）的锁定版本。
    pub fake: &'static str,
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
        sqlglot: SQLGLOT,
        lax_sql: LAX_SQL,
        gpui_kit: GPUI_KIT,
        gpui_fast: GPUI_FAST,
        duckdb: DUCKDB,
        rusqlite: RUSQLITE,
        fake: FAKE,
    }
}

/// 编进二进制的 SQLite 引擎版本（`SQLITE_VERSION` 常量，不查库、不开连接）。
pub fn sqlite_engine() -> &'static str {
    rusqlite::version()
}

/// DuckDB 的 crate 版本 → 引擎版本，返回 `None` 表示无法判定。
///
/// `duckdb-rs` 用 `1.MAJOR_MINOR_PATCH.x` 编码引擎版本，规则照
/// `libduckdb-sys` 的 `build.rs::duckdb_version_from_pkg_version`：
/// `1.10506.1` → 第二段 `10506` → major `10506/10000=1` / minor `(10506/100)%100=5`
/// / patch `10506%100=6` → `1.5.6`。
///
/// 与上游不同的是这里**不 panic**：`unknown`、缺段、小于 `1.0.0` 的编码一律返回
/// `None`，由调用方显式显示 `unknown`。
pub fn duckdb_engine_from(pkg_version: &str) -> Option<String> {
    let encoded = pkg_version.split('.').nth(1)?.parse::<u32>().ok()?;
    // 1.0.0 编码为 10000；更小的数说明不是这套编码（例如被写成 `unknown`）。
    if encoded < 10_000 {
        return None;
    }
    let major = encoded / 10_000;
    let minor = (encoded / 100) % 100;
    let patch = encoded % 100;
    Some(format!("{major}.{minor}.{patch}"))
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

    /// DuckDB 一行的值：引擎版本在前（验证要对的就是它），crate 版本放括号里。
    pub fn duckdb_text(&self) -> String {
        match duckdb_engine_from(self.duckdb) {
            Some(engine) => format!("DuckDB {engine}（crate {}）", self.duckdb),
            None => format!("DuckDB unknown（crate {}）", self.duckdb),
        }
    }

    /// SQLite 一行的值：引擎版本来自编译期常量，永远确切。
    pub fn sqlite_text(&self) -> String {
        format!("SQLite {}（rusqlite {}）", sqlite_engine(), self.rusqlite)
    }

    /// 「关于」弹窗里逐行展示的（字段名, 值）。14 项，前 5 项是身份、后 9 项是构建与依赖。
    ///
    /// 弹窗与剪贴板都由这一份顺序生成，避免两处漂移（见设计稿 §8）。
    pub fn rows(&self) -> Vec<(&'static str, String)> {
        vec![
            ("版本标识", self.tag()),
            ("版本号", self.version.to_string()),
            ("提交", format!("{} · {}", self.commit, self.branch)),
            ("工作树", self.tree_text()),
            ("提交时间", self.commit_time_text()),
            ("构建档", self.profile.to_string()),
            ("目标平台", self.target.to_string()),
            ("SQL 语义", format!("sqlglot {}", self.sqlglot)),
            ("SQL 排版", format!("lax-sql {}", self.lax_sql)),
            ("分析引擎", self.duckdb_text()),
            ("本地存储", self.sqlite_text()),
            ("UI 组件库", format!("gpui-kit {}", self.gpui_kit)),
            ("渲染内核", format!("gpui-fast {}", self.gpui_fast)),
            ("Mock 数据", format!("fake {}", self.fake)),
        ]
    }

    /// 写进剪贴板的多行文本：粘到 issue / 聊天里就能对上号。
    ///
    /// 键列宽 11，值从第 12 个字符起；14 行与 [`Self::rows`] 一一对应。
    pub fn clipboard_text(&self) -> String {
        let dirty = match self.dirty {
            Some(true) => "yes",
            Some(false) => "no",
            None => "unknown",
        };
        let duckdb = match duckdb_engine_from(self.duckdb) {
            Some(engine) => format!("{engine} (crate {})", self.duckdb),
            None => format!("unknown (crate {})", self.duckdb),
        };
        format!(
            "RdataStation {}\n\
             version     {}\n\
             commit      {} ({})\n\
             dirty       {}\n\
             time        {}\n\
             profile     {}\n\
             target      {}\n\
             sqlglot     {}\n\
             lax-sql     {}\n\
             duckdb      {}\n\
             sqlite      {} (rusqlite {})\n\
             gpui-kit    {}\n\
             gpui-fast   {}\n\
             fake        {}",
            self.tag(),
            self.version,
            self.commit,
            self.branch,
            dirty,
            self.commit_time_text(),
            self.profile,
            self.target,
            self.sqlglot,
            self.lax_sql,
            duckdb,
            sqlite_engine(),
            self.rusqlite,
            self.gpui_kit,
            self.gpui_fast,
            self.fake,
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
            sqlglot: "0.11.2",
            lax_sql: "0.3.0",
            gpui_kit: "0.9.0",
            gpui_fast: "0.1.17",
            duckdb: "1.10506.1",
            rusqlite: "0.40.2",
            fake: "5.1.0",
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
        assert_eq!(sample(Some(true), "1fab7d89").tag_base(), "0.2.11+1fab7d89");
        assert_eq!(sample(None, "unknown").tag_base(), "0.2.11");
    }

    #[test]
    fn tree_text_distinguishes_unknown_from_clean() {
        assert!(sample(Some(false), "abc").tree_text().starts_with("干净"));
        assert!(sample(Some(true), "abc").tree_text().contains("未提交改动"));
        assert!(sample(None, "abc").tree_text().contains("无法判定"));
    }

    #[test]
    fn rows_carry_all_fourteen_fields() {
        let info = sample(Some(true), "1fab7d89");
        let rows = info.rows();
        assert_eq!(rows.len(), 14);
        for key in [
            "版本标识",
            "版本号",
            "提交",
            "工作树",
            "提交时间",
            "构建档",
            "目标平台",
            "SQL 语义",
            "SQL 排版",
            "分析引擎",
            "本地存储",
            "UI 组件库",
            "渲染内核",
            "Mock 数据",
        ] {
            assert!(
                rows.iter().any(|(k, _)| *k == key),
                "missing row key: {key}"
            );
        }
        assert!(
            rows.iter()
                .any(|(k, v)| *k == "SQL 语义" && v == "sqlglot 0.11.2")
        );
        assert!(
            rows.iter()
                .any(|(k, v)| *k == "渲染内核" && v == "gpui-fast 0.1.17")
        );
        assert!(
            rows.iter()
                .any(|(k, v)| *k == "Mock 数据" && v == "fake 5.1.0")
        );
        // 提交用 `·` 分隔（弹窗与截图一致），不是全角括号。
        assert!(rows.iter().any(|(k, v)| *k == "提交" && v.contains(" · ")));
    }

    #[test]
    fn duckdb_row_pair_engine_with_crate() {
        let info = sample(Some(false), "abc");
        let value = info.duckdb_text();
        assert!(value.starts_with("DuckDB 1.5.6"), "got {value}");
        assert!(value.contains("crate 1.10506.1"), "got {value}");
    }

    #[test]
    fn duckdb_engine_decodes_pkg_version() {
        assert_eq!(
            duckdb_engine_from("1.10506.1").as_deref(),
            Some("1.5.6"),
            "1.10506.1 应解码为 1.5.6"
        );
        assert_eq!(duckdb_engine_from("1.10000.0").as_deref(), Some("1.0.0"));
        // 不是这套编码 / 段数不足：不 panic，显式 None。
        assert_eq!(duckdb_engine_from("unknown"), None);
        assert_eq!(duckdb_engine_from("1.1"), None);
        assert_eq!(duckdb_engine_from(""), None);
    }

    #[test]
    fn sqlite_engine_is_the_compiled_constant() {
        // 编译期常量，形如 3.51.3；至少保证非空且是三段式。
        let v = sqlite_engine();
        assert_eq!(v.split('.').count(), 3, "got {v}");
    }

    #[test]
    fn clipboard_text_has_fourteen_lines_and_new_rows() {
        let info = sample(Some(true), "1fab7d89");
        let text = info.clipboard_text();
        assert_eq!(text.lines().count(), 14, "got {text}");
        assert!(text.starts_with("RdataStation 0.2.11+1fab7d89-dirty"));
        assert!(text.contains("dirty       yes"));
        assert!(text.contains("sqlglot     0.11.2"));
        assert!(text.contains("lax-sql     0.3.0"));
        assert!(text.contains("duckdb      1.5.6 (crate 1.10506.1)"));
        assert!(text.contains("gpui-fast   0.1.17"));
        assert!(text.contains("fake        5.1.0"));
    }

    #[test]
    fn commit_time_formats_in_civil_form() {
        let text = sample(Some(false), "abc").commit_time_text();
        assert!(text.contains('-') && text.contains(':'), "got {text}");
        assert!(!text.contains("unknown"));
    }
}
