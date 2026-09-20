//! 文件档位（A13）：按大小决定挂哪些重能力
//!
//! 规格（原型 §4 降级矩阵、架构 §8）：
//!
//! | 档位 | 门槛 | 策略 |
//! | --- | --- | --- |
//! | `Normal` | < 50 MB | 全功能 |
//! | `Large` | 50 MB – 200 MB | 关补全与折叠（重能力）；高亮由内核自行降级（>约 5 万行不出 token，已有回归） |
//! | `Huge` | ≥ 200 MB | **只读打开 + 提示卡**（不建可编辑会话） |
//!
//! ## 为什么判定是纯函数
//!
//! 档位决定"能不能编辑 / 有没有提示"，是最不该出错的一件事；做成 `tier_for_size` 之后
//! 边界（49.9 MB / 50 MB / 199.9 MB / 200 MB）可以逐条断言，不必造真文件。
//! 取文件大小那一层（[`tier_for_path`]）是 I/O，**只从事件路径调用**（打开文档时一次）。
//!
//! ## 判据从哪里落地（判据 → 真实开关）
//!
//! - 补全：`EditorShared::completion_enabled`（B9）；
//! - 折叠：`EditorShared::folding_enabled` → 宿主面板调 `set_folding(false)`（B17）——
//!   不只是留一个判据，而是把内核开关真的关上（大文件下折叠每键要重扫全文）；
//! - 分块加载（原型 §13 P5）已明确推到 1b 之后，这里不做。

use std::path::Path;

/// 档位 × 能力的**降级表**（B18）：逐能力一位，不是"关重能力"一句话
///
/// 为什么要成表：降级口径原先散在三个判据里（`disables_completion` / `disables_folding` /
/// 高亮门槛），且 zqlz 那份 `large_file_policy` 给的教训是——**逐能力降级**才能既保住能用的部分、
/// 又不把重活带上。表建好后，各处只问表，不再各写一套 `matches!(self, Normal)`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapabilityPlan {
    /// 语法/语义着色（词法扫描，O(n)）
    pub highlight: bool,
    /// 折叠候选（要一次全文词法扫描，见 `fold` 模块）
    pub folding: bool,
    /// SQL 补全（要schema 目录 + 每次输入问一次内核）
    pub completion: bool,
    /// 打字期词法诊断（与折叠共用一趟词法扫描）
    pub diagnostics: bool,
    /// 语句数（状态栏要它，成本最低，一般不开）
    pub statements: bool,
}

/// 档位 → 降级表（**纯函数**，边界可逐条断言）
impl FileTier {
    pub fn plan(self) -> CapabilityPlan {
        match self {
            // 常规：全开
            Self::Normal => CapabilityPlan {
                highlight: true,
                folding: true,
                completion: true,
                diagnostics: true,
                statements: true,
            },
            // 大文件：留下"看"的能力（高亮由内核按行数自行降级），关掉每次输入都要重算的那些
            Self::Large => CapabilityPlan {
                highlight: true,
                folding: false,
                completion: false,
                diagnostics: false,
                statements: true,
            },
            // 超大：内容根本没读进来，一个都不开
            Self::Huge => CapabilityPlan {
                highlight: false,
                folding: false,
                completion: false,
                diagnostics: false,
                statements: false,
            },
        }
    }
}

/// 大文件门槛（50 MiB）
pub const LARGE_FILE_BYTES: u64 = 50 * 1024 * 1024;

/// 超大文件门槛（200 MiB）
pub const HUGE_FILE_BYTES: u64 = 200 * 1024 * 1024;

/// 文件档位
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileTier {
    /// 常规：全功能
    Normal,
    /// 大：关补全（高亮由内核按行数自行降级）
    Large,
    /// 超大：只读打开 + 提示卡
    Huge,
}

impl FileTier {
    /// 状态栏 / 提示卡用的档位名
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "常规",
            Self::Large => "大文件",
            Self::Huge => "超大文件",
        }
    }

    /// 是否关掉补全
    ///
    /// 判据的**唯一来源**是 [`FileTier::plan`]（B18 收口；这里保留方法名是因为调用点语义清楚）
    pub fn disables_completion(self) -> bool {
        !self.plan().completion
    }

    /// 是否关掉折叠（B17 已接线：`view/host` 据此对内核 `set_folding(false)`）
    pub fn disables_folding(self) -> bool {
        !self.plan().folding
    }

    /// 是否只读打开（超大文件不做可编辑会话）
    pub fn opens_read_only(self) -> bool {
        matches!(self, Self::Huge)
    }

    /// 提示卡文案（`None` = 不显示提示卡）
    pub fn notice(self) -> Option<&'static str> {
        match self {
            Self::Normal => None,
            Self::Large => Some("大文件：已关闭补全等重能力，高亮按需降级"),
            Self::Huge => {
                Some("超大文件（>200MB）：未加载内容，以免占用大量内存；请用外部工具查看或截取片段")
            }
        }
    }
}

/// 按字节数判定档位（**纯函数**：边界可逐条断言）
pub fn tier_for_size(bytes: u64) -> FileTier {
    if bytes >= HUGE_FILE_BYTES {
        FileTier::Huge
    } else if bytes >= LARGE_FILE_BYTES {
        FileTier::Large
    } else {
        FileTier::Normal
    }
}

/// 按路径取大小并判定档位（**I/O**：只从打开文档的事件路径调用）
///
/// 取不到元数据（文件不存在 / 无权限）时按 `Normal` 处理：让"打开"这条路径去报真实错误，
/// 而不是在这里先造一个档位出来。
pub fn tier_for_path(path: &Path) -> FileTier {
    match std::fs::metadata(path) {
        Ok(metadata) => tier_for_size(metadata.len()),
        Err(_) => FileTier::Normal,
    }
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{FileTier, HUGE_FILE_BYTES, LARGE_FILE_BYTES, tier_for_path, tier_for_size};

    #[test]
    fn tiers_split_exactly_at_the_documented_thresholds() {
        assert_eq!(tier_for_size(0), FileTier::Normal);
        assert_eq!(tier_for_size(LARGE_FILE_BYTES - 1), FileTier::Normal);
        assert_eq!(
            tier_for_size(LARGE_FILE_BYTES),
            FileTier::Large,
            "50 MiB 起是大文件"
        );
        assert_eq!(tier_for_size(HUGE_FILE_BYTES - 1), FileTier::Large);
        assert_eq!(
            tier_for_size(HUGE_FILE_BYTES),
            FileTier::Huge,
            "200 MiB 起只读打开"
        );
        assert_eq!(tier_for_size(u64::MAX), FileTier::Huge);
    }

    #[test]
    fn only_the_huge_tier_opens_read_only() {
        assert!(!FileTier::Normal.opens_read_only());
        assert!(!FileTier::Large.opens_read_only());
        assert!(FileTier::Huge.opens_read_only());
    }

    #[test]
    fn heavy_capabilities_turn_off_from_the_large_tier_on() {
        assert!(!FileTier::Normal.disables_completion());
        assert!(!FileTier::Normal.disables_folding());
        assert!(FileTier::Large.disables_completion());
        assert!(FileTier::Huge.disables_completion());
        assert!(FileTier::Large.disables_folding());
    }

    #[test]
    fn every_tier_says_what_it_means() {
        assert_eq!(FileTier::Normal.notice(), None, "常规文件不显示提示卡");
        assert!(FileTier::Large.notice().expect("有提示").contains("补全"));
        let huge_notice = FileTier::Huge.notice().expect("有提示");
        assert!(
            huge_notice.contains("未加载"),
            "超大文件的提示要说清“内容没被加载”：{huge_notice}"
        );
        assert_eq!(FileTier::Huge.label(), "超大文件");
    }

    #[test]
    fn a_missing_file_is_normal_so_that_open_reports_the_real_error() {
        let missing = std::env::temp_dir().join("rds_tier_missing_文件.sql");
        let _ = std::fs::remove_file(&missing);
        assert_eq!(tier_for_path(&missing), FileTier::Normal);
    }

    #[test]
    fn a_real_small_file_is_normal() {
        let dir = std::env::temp_dir().join(format!("rds_tier_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建目录");
        let path = dir.join("small.sql");
        std::fs::write(&path, "select 1;").expect("写盘");

        assert_eq!(tier_for_path(&path), FileTier::Normal);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 真机档位：造一个 200 MiB 的稀疏文件，确认判成 `Huge`（快，不真写满磁盘）
    #[test]
    fn a_sparse_huge_file_is_recognised_without_reading_it() {
        let dir = std::env::temp_dir().join(format!("rds_tier_huge_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建目录");
        let path = dir.join("huge.sql");

        let file = std::fs::File::create(&path).expect("建文件");
        file.set_len(HUGE_FILE_BYTES + 1024).expect("扩到 200 MiB");
        drop(file);

        assert_eq!(tier_for_path(&path), FileTier::Huge);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
