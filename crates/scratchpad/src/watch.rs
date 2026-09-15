//! 草稿箱文件监控（外部改动 → 变更标记）。
//!
//! 设计取舍：**不做「事件 → 精确增量同步」**，只做「变更标记」——OS 事件只把标记置位，
//! 由视图侧按自己的节奏（去抖后）重新拉取一次列表。理由：
//! ① 增量同步要复刻 `scan_dir_tree` 的排序 / 过滤 / 懒加载 / 虚拟列表行号语义，必然出现两处真相；
//! ② 草稿箱是**临时区**，一次重拉成本可控（懒加载 + 只取模块根与已展开目录）；
//! ③ 标记法天然合并事件风暴（编辑器保存一次常触发多条事件），不会把 UI 打成刷新循环。
//!
//! 监听范围：只监听 `{项目}/scratchpad/`（递归）；`.RSmeta/` **不监听**，
//! 因此模块自身的配置写入（引用增删改、`file_meta`）不会造成自激刷新。
//!
//! 生命周期：`ScratchpadWatcher` 被 drop 时，内部 watcher 释放 → 回调发送端随之释放 →
//! 收敛线程的 `recv` 结束并自行退出（无需显式 stop 标志）。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

/// 变更标记（可跨线程共享；`take` 取值即清零）。
#[derive(Clone, Default)]
pub struct ChangeFlag(Arc<AtomicBool>);

impl ChangeFlag {
    /// 置位（由监控回调调用）。
    pub fn mark(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// 读取并清零（由视图侧的轮询调用）。
    pub fn take(&self) -> bool {
        self.0.swap(false, Ordering::SeqCst)
    }
}

/// 草稿箱目录监控器（递归监听模块根）。
pub struct ScratchpadWatcher {
    dir: PathBuf,
    flag: ChangeFlag,
    /// 持有它才保持监听；drop 即 unwatch。
    _watcher: RecommendedWatcher,
    /// 事件收敛线程（自然退出，不 join）。
    _thread: std::thread::JoinHandle<()>,
}

impl ScratchpadWatcher {
    /// 开始监听模块目录（目录不存在时先创建——否则监控一个不存在的路径会失败）。
    pub fn start(dir: PathBuf) -> Result<Self, String> {
        std::fs::create_dir_all(&dir).map_err(|e| format!("创建监控目录失败: {e}"))?;
        let flag = ChangeFlag::default();
        let (tx, rx) = mpsc::channel::<notify::Result<notify::Event>>();
        let mut watcher =
            notify::recommended_watcher(tx).map_err(|e| format!("创建文件监控失败: {e}"))?;
        watcher
            .watch(&dir, RecursiveMode::Recursive)
            .map_err(|e| format!("监控目录失败: {e}"))?;

        let thread_flag = flag.clone();
        let thread = std::thread::Builder::new()
            .name("rds-scratchpad-watch".to_string())
            .spawn(move || {
                // 事件的具体内容不关心（含 `Err`，如盘符脱机/恢复）：一律置位，
                // 让视图重新拉一次列表，一致性由「重拉」保证。
                for _ in rx {
                    thread_flag.mark();
                }
            })
            .map_err(|e| format!("启动监控线程失败: {e}"))?;

        Ok(Self {
            dir,
            flag,
            _watcher: watcher,
            _thread: thread,
        })
    }

    /// 读取并清零「有外部改动」标记。
    pub fn take_changed(&self) -> bool {
        self.flag.take()
    }

    /// 被监控的目录（用于判断当前项目根是否变化）。
    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn change_flag_marks_and_clears() {
        let flag = ChangeFlag::default();
        assert!(!flag.take(), "初始应为未变更");
        flag.mark();
        assert!(flag.take(), "置位后应读到变更");
        assert!(!flag.take(), "取走后应清零");
    }

    /// 外部写入被观测到（Windows 走 `ReadDirectoryChangesW`，直接子文件创建是可靠事件）。
    ///
    /// 给足 10 s 容忍慢盘/杀软扫描；超时不 panic 而是断言失败并给出提示。
    #[test]
    fn external_write_is_observed() {
        let project = std::env::temp_dir().join(format!(
            "rds_scratchpad_watch_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        ));
        let dir = project.join("scratchpad");
        let watcher = ScratchpadWatcher::start(dir.clone()).expect("watcher should start");
        // 起手先清零（`create_dir_all` 本身可能触发一次事件）。
        let _ = watcher.take_changed();

        std::fs::write(dir.join("outer.sql"), "select 1").unwrap();

        let deadline = Instant::now() + Duration::from_secs(10);
        let mut observed = false;
        while Instant::now() < deadline {
            if watcher.take_changed() {
                observed = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(observed, "外部写入应在 10 s 内被观测到");

        drop(watcher);
        std::fs::remove_dir_all(&project).ok();
    }
}
