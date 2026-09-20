//! 后台扫描（B18）：语句数 / 折叠候选 / 打字期词法诊断——**一次遍历、三个产物、一条工作线程**
//!
//! ## 为什么要有它（性能账，不是猜）
//!
//! 项目自己的性能基线（`crates/editor/tests/offline_baseline.rs`，`--nocapture` 可看数字）实测
//! **debug 档**：5 千行脚本，一次文本变更里 `split_statements` + `fold::spans` 合计约 90–130 ms；
//! 贴近 1 MB 的脚本约 0.6–1.0 s。而这三件事原先全在 `InputEvent::Change` 上**同步**做——
//! 每敲一下就把主线程按住这么久，是"大脚本打字卡顿"的根因。
//!
//! 做法照本项目既有两条成熟路子（`execution::ExecQueue` 的"一条专用线程 + mpsc"、
//! `nav_jobs` 的"主线程轮询回填"），并借 navop 的**过期结果丢弃**口径：
//!
//! | 纪律 | 落点 |
//! | --- | --- |
//! | **修订号合并** | 同一时刻只关心最新文本：新任务覆盖旧任务（不是排队把每一下都算一遍） |
//! | **陈旧丢弃** | 回执带修订号，主线程只接受 `revision == 当前修订号` 的那一份（[`is_current`]） |
//! | **零 GPUI** | 本模块不依赖视图层：`ScanReport` 是纯数据，测起来不用开窗口 |
//!
//! ## 退出（别留孤儿线程）
//!
//! 进程/面板走的时候要能收工：`Drop` 置**停机标志** + 叫醒线程 + `join` 等它走完。**判据必须是标志**，
//! 不能靠 `Arc::strong_count` 之类的"引用计数到 1"——`Drop` 自己的那个引用还在，计数永远到不了 1，
//! 于是 `join` 死等（这个坑当场踩过，测试套件整批挂住）。
//!
//! ## 不做什么
//!
//! - **不做增量**：整篇重算（门槛内的文本本来就是几十毫秒级），增量的复杂度留给"真的不够快"那天；
//! - **不碰 I/O、不碰数据库**：与 `fold` / `language_service` 一样是纯计算，所以能离线跑。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

use crate::channel::ExecChannel;
use crate::completion::Catalog;
use crate::fold::{self, FoldSpan};
use crate::language_service::{self, Diagnostic, Request};

/// 一次扫描的产物（**纯数据**：可跨线程、可断言）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanReport {
    /// 提交时的修订号（主线程据此丢弃陈旧回执）
    pub revision: u64,
    /// 语句数（`engine::sql::split` 口径）
    pub statements: usize,
    /// 折叠候选（`fold::spans` 口径）
    pub folds: Vec<FoldSpan>,
    /// 打字期词法诊断（`language_service::diagnostics` 口径）
    pub diagnostics: Vec<Diagnostic>,
}

/// 回执是否是"当下这一份"（陈旧丢弃的唯一判据）
///
/// 单独成函数是为了能逐条断言：**旧修订号的回执一律不采用**（哪怕它内容看起来对）。
pub fn is_current(report: &ScanReport, current_revision: u64) -> bool {
    report.revision == current_revision
}

/// 扫描范围：由档位的能力表决定（`limits::CapabilityPlan`）——大文件不扫重活
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanScope {
    /// 算折叠候选（关掉只影响 chevron，不影响编辑）
    pub folds: bool,
    /// 算打字期诊断（关掉只影响波浪线）
    pub diagnostics: bool,
}

impl ScanScope {
    /// 全开（常规档位）
    pub fn full() -> Self {
        Self {
            folds: true,
            diagnostics: true,
        }
    }

    /// 全关（超大档位：内容根本没读进来，没什么可扫）
    pub fn none() -> Self {
        Self {
            folds: false,
            diagnostics: false,
        }
    }
}

/// 一份待算的活（只留最新那一份）
struct Job {
    revision: u64,
    text: String,
    channel: ExecChannel,
    scope: ScanScope,
}

/// 待算槽位 + 唤醒信号 + 停机标志
///
/// 槽位是"**覆盖式**"的：`Condvar` 而不是队列——合并就是"把槽里的那一份换掉"。
struct Shared {
    slot: Mutex<Option<Job>>,
    signal: Condvar,
    shutdown: AtomicBool,
}

/// 扫描器：一条工作线程，活到进程结束（线程空闲时阻塞在条件变量上，不占 CPU）
pub struct Scanner {
    shared: Arc<Shared>,
    reports: Receiver<ScanReport>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Scanner {
    /// 起一条工作线程
    pub fn new() -> Self {
        let shared = Arc::new(Shared {
            slot: Mutex::new(None),
            signal: Condvar::new(),
            shutdown: AtomicBool::new(false),
        });
        let (sender, reports) = mpsc::channel::<ScanReport>();
        let worker = {
            let shared = Arc::clone(&shared);
            thread::Builder::new()
                .name("rds-editor-scan".to_string())
                .spawn(move || worker_loop(&shared, &sender))
                .ok()
        };
        Self {
            shared,
            reports,
            worker,
        }
    }

    /// 提交一份文本（**覆盖**尚未开算的那一份）
    pub fn submit(&self, revision: u64, text: String, channel: ExecChannel, scope: ScanScope) {
        let mut guard = self
            .shared
            .slot
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        *guard = Some(Job {
            revision,
            text,
            channel,
            scope,
        });
        self.shared.signal.notify_one();
    }

    /// 取一份已完成的回执（没有就是 `None`；同一时刻只可能有一份在算）
    pub fn try_recv(&self) -> Option<ScanReport> {
        match self.reports.try_recv() {
            Ok(report) => Some(report),
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => None,
        }
    }
}

impl Default for Scanner {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Scanner {
    /// 停机：置标志 → 叫醒 → 等它走完（**判据是标志**，不靠引用计数，见模块文档）
    fn drop(&mut self) {
        self.shared.shutdown.store(true, Ordering::SeqCst);
        {
            // 拿一次锁保证"取活"那一刻的互斥，再叫醒（`Condvar` 的常规配合）
            let _guard = self
                .shared
                .slot
                .lock()
                .unwrap_or_else(|error| error.into_inner());
        }
        self.shared.signal.notify_all();
        if let Some(worker) = self.worker.take() {
            // 最多等一份扫描算完（几十毫秒级；门槛内）
            let _ = worker.join();
        }
    }
}

/// 工作线程：等活 → 取活 → 算 → 回执（活被覆盖时只算最新的那一份）
fn worker_loop(shared: &Shared, sender: &Sender<ScanReport>) {
    loop {
        let job = {
            let mut guard = shared
                .slot
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            loop {
                if shared.shutdown.load(Ordering::SeqCst) {
                    break None;
                }
                if let Some(job) = guard.take() {
                    break Some(job);
                }
                guard = shared
                    .signal
                    .wait(guard)
                    .unwrap_or_else(|error| error.into_inner());
            }
        };
        let Some(job) = job else {
            return;
        };
        let report = run(&job);
        if sender.send(report).is_err() {
            // 主线程已经不在了（面板销毁）：收工
            return;
        }
    }
}

/// 算一份（纯计算，可离线断言）
fn run(job: &Job) -> ScanReport {
    let statements = engine::sql::split_statements(&job.text).len();
    let folds = if job.scope.folds {
        fold::spans(&job.text)
    } else {
        Vec::new()
    };
    let diagnostics = if job.scope.diagnostics {
        // 诊断只要文本本身（目录与连接是补全面的事，这里给空的就是"不参与"）
        language_service::diagnostics(&Request {
            text: &job.text,
            cursor: 0,
            selection: None,
            catalog: &Catalog::default(),
            connection: None,
            channel: job.channel,
        })
    } else {
        Vec::new()
    };
    ScanReport {
        revision: job.revision,
        statements,
        folds,
        diagnostics,
    }
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{Job, ScanScope, Scanner, is_current, run};
    use crate::channel::ExecChannel;
    use std::time::{Duration, Instant};

    fn wait_for(scanner: &Scanner, revision: u64) -> Option<super::ScanReport> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(report) = scanner.try_recv() {
                if report.revision == revision {
                    return Some(report);
                }
                continue;
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn a_scan_reports_statements_folds_and_diagnostics() {
        let scanner = Scanner::new();
        // 一段**词法扫得动**的文本：语句数与折叠候选都该有
        scanner.submit(
            7,
            "SELECT (\n  1\n);\nSELECT 2;".to_string(),
            ExecChannel::Source,
            ScanScope::full(),
        );
        let report = wait_for(&scanner, 7).expect("回执应当回来");
        assert_eq!(report.revision, 7);
        assert_eq!(report.statements, 2);
        assert_eq!(report.folds.len(), 1, "括号组是一个折叠候选");
        assert!(report.diagnostics.is_empty(), "干净的 SQL 不该有诊断");
    }

    #[test]
    fn a_failed_lex_gives_a_diagnostic_and_no_fold_candidates() {
        // 两条口径合起来看（有意如此）：未闭合字符串 → 词法失败 → **无折叠候选**（不猜），
        // 但**有一条诊断**（这是词法层能证明的错）。同一段文本两个产物不一样。
        let scanner = Scanner::new();
        scanner.submit(
            9,
            "SELECT (\n  1\n);\nSELECT 'x".to_string(),
            ExecChannel::Source,
            ScanScope::full(),
        );
        let report = wait_for(&scanner, 9).expect("回执应当回来");
        assert!(report.folds.is_empty(), "词法失败就不给候选");
        assert!(!report.diagnostics.is_empty(), "但要说得出错在哪");
    }

    #[test]
    fn the_scope_switches_heavy_products_off() {
        let job = Job {
            revision: 1,
            text: "SELECT (\n  1\n)".to_string(),
            channel: ExecChannel::Source,
            scope: ScanScope::none(),
        };
        let report = run(&job);
        assert!(report.folds.is_empty(), "关掉折叠就不该算候选");
        assert!(report.diagnostics.is_empty(), "关掉诊断就不该算波浪线");
        assert_eq!(
            report.statements, 1,
            "语句数便宜且状态栏要它，不归 scope 管"
        );
    }

    #[test]
    fn only_the_latest_submission_is_computed() {
        // **合并**：连提交三次，最终拿到的必须是最后那一次的修订号（前面两份被覆盖）
        let scanner = Scanner::new();
        for revision in [1, 2, 3] {
            scanner.submit(
                revision,
                format!("SELECT {revision};"),
                ExecChannel::Source,
                ScanScope::full(),
            );
        }
        let report = wait_for(&scanner, 3).expect("最新一份的回执应当回来");
        assert_eq!(report.revision, 3);
        assert_eq!(report.statements, 1);
        // 把已经到的都排空：不该出现比 3 更新的东西（也不该出现更旧的、被采用的那份）
        while let Some(report) = scanner.try_recv() {
            assert!(report.revision <= 3);
        }
    }

    #[test]
    fn revision_three_is_the_only_one_accepted_when_current_is_three() {
        let scanner = Scanner::new();
        scanner.submit(
            3,
            "SELECT 1;".to_string(),
            ExecChannel::Source,
            ScanScope::full(),
        );
        let report = wait_for(&scanner, 3).expect("回执应当回来");
        assert!(is_current(&report, 3), "与当前修订号相等才采用");
        assert!(!is_current(&report, 4), "用户又敲了一下 → 这份过期，丢掉");
    }

    #[test]
    fn dropping_the_scanner_stops_the_worker_without_waiting_for_references() {
        // 这条钉住的是那个坑：`Drop` 必须靠**停机标志**退出，不能靠引用计数
        // （靠计数会让 `join` 死等——测试套件整批挂住就是这么来的）
        let scanner = Scanner::new();
        scanner.submit(
            1,
            "SELECT 1;".to_string(),
            ExecChannel::Source,
            ScanScope::full(),
        );
        let start = Instant::now();
        drop(scanner);
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "Drop 不该卡住（最多等一份扫描算完）"
        );
    }
}
