//! 规则目录监听：规则文件内容变化时自动重载规则集。
//!
//! # 为什么是「内容哈希轮询」而不是事件监听
//!
//! 1. **零新依赖**：`notify` 在本仓只是传递依赖（未在 `workspace.dependencies` 声明），
//!    直接使用属隐式依赖；而 `gpui-kit::ThemeRegistry::watch_dir` 需要 gpui 的
//!    `Context`，洞察 crate 在视图归属拍板前不依赖 gpui。
//! 2. **去抖天然成立**：变更判定**不看事件、只看内容哈希**——编辑器保存、工具重写、
//!    同一文件连写多次都只会得到「内容变了」这一个结论，不需要防抖窗口。
//! 3. **成本可忽略**：规则文件是几十个小 TOML，读取走页缓存；间隔 2 秒的整批哈希
//!    远低于任何可感知开销。
//!
//! # 跟随项目切换
//!
//! 监听器不接收固定的目录列表，而是每轮读 [`watched_project_root`] 现算目录：
//! 宿主在项目打开 / 切换 / 关闭时调用 [`set_watched_project_root`] 更新即可。
//! 这样避免「只在启动时算一次目录 → 切项目后监听旧项目」的缺口，
//! 也让洞察 crate 无需感知项目会话（那是 `project` / `workbench` 的职责）。
//!
//! 用进程级静态量而非参数传递，理由同 `DISABLED_RULES`：宿主是单线程的 GPUI 视图，
//! 监听是后台线程，跨线程只需要「一个很小的、变动很少的值」。
//!
//! # 与索引的关系
//!
//! 监听只做**规则集重载**（[`crate::reload_insight_rules`]），立即影响分析结果；
//! **索引表（`insight_rule_index`）不在此刷新**——它需要项目库连接，把数据库访问
//! 拉进后台轮询线程不划算。改为置一个**陈旧标记**（[`index_is_stale`]），
//! 由规则管理视图在打开时检查并先跑一次 `sync_project_rules`。
//!
//! 好处：分析正确性不依赖数据库；索引的刷新时机与「谁在看它」对齐。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::rule_registry::{get_global_rules_dir, get_project_rules_dir};

/// 默认轮询间隔。规则改动属低频操作，2 秒对使用者已是「立即生效」。
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(2);

/// 索引陈旧标记：监听检测到磁盘变化后置位，由规则管理视图消费。
static INDEX_STALE: AtomicBool = AtomicBool::new(false);

/// 宿主当前打开的项目根（监听线程据此现算要看的目录）。
static WATCHED_PROJECT_ROOT: RwLock<Option<PathBuf>> = RwLock::new(None);

/// 设置当前项目根。宿主在**项目打开 / 切换 / 关闭**时调用。
///
/// 传 `None` 表示无项目：监听仍会继续，但只看全局层。
pub fn set_watched_project_root(project_root: Option<PathBuf>) {
    match WATCHED_PROJECT_ROOT.write() {
        Ok(mut slot) => *slot = project_root,
        Err(e) => tracing::warn!("Failed to set watched project root: {}", e),
    }
}

/// 读取当前项目根（监听线程每轮调用）。
pub fn watched_project_root() -> Option<PathBuf> {
    WATCHED_PROJECT_ROOT
        .read()
        .map(|slot| slot.clone())
        .unwrap_or(None)
}

/// 索引是否落后于磁盘。
pub fn index_is_stale() -> bool {
    INDEX_STALE.load(Ordering::Relaxed)
}

/// 清除索引陈旧标记（**唯一消费点**：规则管理视图在完成同步后调用）。
pub fn clear_index_stale() {
    INDEX_STALE.store(false, Ordering::Relaxed);
}

/// 监听某项目需要关注的全部规则目录。
///
/// 内置层不入监听：它随二进制分发，运行期不可能变化。
pub fn watch_dirs(project_root: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    if let Some(root) = project_root {
        dirs.push(get_project_rules_dir(root));
    }

    // 系统目录不可用时降级为「只监听项目层」——内置规则仍可用，
    // 不应因为拿不到系统目录就让整个监听失效。
    match engine::migration::get_system_dir() {
        Ok(system_dir) => dirs.push(get_global_rules_dir(&system_dir)),
        Err(e) => tracing::warn!(
            "System dir unavailable, rules watcher will skip the global layer: {}",
            e
        ),
    }

    dirs
}

/// 计算一组目录下全部规则文件的**内容指纹**。
///
/// 指纹 = 对所有 `.toml` 文件（按相对路径排序）的 `路径 + 内容哈希` 再做一次哈希。
/// 目录不存在、为空、或文件读不出来都不影响调用（读不出的文件按固定标记计入，
/// 其出现 / 消失本身就会改变指纹）。
pub fn rules_fingerprint(dirs: &[PathBuf]) -> u64 {
    let mut entries: Vec<(String, String)> = Vec::new();

    for dir in dirs {
        collect_files(dir, dir, &mut entries);
    }
    entries.sort();

    let mut hasher = Sha256::new();
    for (path, hash) in &entries {
        hasher.update(path.as_bytes());
        hasher.update(b":");
        hasher.update(hash.as_bytes());
        hasher.update(b"\n");
    }
    let digest = hasher.finalize();
    u64::from_be_bytes(digest[..8].try_into().unwrap_or([0u8; 8]))
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<(String, String)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(root, &path, out);
        } else if path.extension().and_then(|s| s.to_str()) == Some("toml") {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let hash = std::fs::read(&path)
                .map(|bytes| format!("{:x}", Sha256::digest(&bytes)))
                .unwrap_or_else(|_| "<unreadable>".to_string());
            out.push((rel, hash));
        }
    }
}

/// 规则目录监听句柄；**drop 即停止**监听线程。
pub struct RulesWatcher {
    stop: Arc<AtomicBool>,
    reloads: Arc<AtomicU64>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl RulesWatcher {
    /// 按默认间隔启动监听（目录由 [`watched_project_root`] 现算，自动跟随项目切换）。
    pub fn spawn() -> Self {
        Self::spawn_with(DEFAULT_POLL_INTERVAL)
    }

    /// 自定义轮询间隔（测试与特殊场景使用）。
    pub fn spawn_with(interval: Duration) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let reloads = Arc::new(AtomicU64::new(0));

        // 基线指纹必须在**起线程之前**建立：若放到线程闭包里算，
        // 在 `spawn_with` 返回后、线程首次取样前发生的文件变化会被当成
        // 「本来就是那样」而**永久漏报**（测试以超时暴露过这个问题）。
        let initial_root = watched_project_root();
        let initial_fp = rules_fingerprint(&watch_dirs(initial_root.as_deref()));

        let thread = {
            let stop = stop.clone();
            let reloads = reloads.clone();
            std::thread::Builder::new()
                .name("rds-insight-rules-watcher".to_string())
                .spawn(move || {
                    let mut last_root = initial_root;
                    let mut last_fp = initial_fp;

                    while !stop.load(Ordering::Relaxed) {
                        std::thread::sleep(interval);
                        if stop.load(Ordering::Relaxed) {
                            break;
                        }

                        // 每轮现算：项目根可能已被宿主换掉（切换项目）。
                        let root = watched_project_root();
                        let fp = rules_fingerprint(&watch_dirs(root.as_deref()));

                        if fp == last_fp && root == last_root {
                            continue;
                        }
                        last_root = root.clone();
                        last_fp = fp;

                        // 内容或项目变了：重扫三层并应用启停。
                        // 这一步是同步的，因此本线程不需要 tokio 运行时。
                        let count = crate::reload_insight_rules(root.as_deref());
                        INDEX_STALE.store(true, Ordering::Relaxed);
                        reloads.fetch_add(1, Ordering::Relaxed);
                        tracing::info!(
                            "Insight rules changed (project: {}), reloaded {} rule(s)",
                            root.as_ref()
                                .map(|p| p.display().to_string())
                                .unwrap_or_else(|| "<none>".to_string()),
                            count
                        );
                    }
                })
                .ok()
        };

        if thread.is_none() {
            tracing::warn!("Failed to spawn insight rules watcher thread; hot-reload disabled");
        }

        Self {
            stop,
            reloads,
            thread,
        }
    }

    /// 已触发的重载次数（可观测性与测试用）。
    pub fn reload_count(&self) -> u64 {
        self.reloads.load(Ordering::Relaxed)
    }

    /// 显式停止并等待线程退出。
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for RulesWatcher {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const RULE_A: &str = r#"
[meta]
id = "watch-rule-a"
name = "监听测试规则"
category = "column"
applies_to = ["Any"]
builtin = false

[query]
template = "SELECT 1"
"#;

    const RULE_B: &str = r#"
[meta]
id = "watch-rule-b"
name = "监听测试规则 B"
category = "column"
applies_to = ["Any"]
builtin = false

[query]
template = "SELECT 2"
"#;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rds_rules_watch_{}_{}",
            std::process::id(),
            tag
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    /// 等条件成立，最多等 `timeout`；返回是否成立。
    fn wait_until(timeout: Duration, mut cond: impl FnMut() -> bool) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        while std::time::Instant::now() < deadline {
            if cond() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        cond()
    }

    /// 指纹只由内容与路径决定：内容不变则稳定，内容和路径变化都要体现。
    #[test]
    fn test_fingerprint_tracks_content_and_path() {
        let dir = temp_dir("fp");
        let file = dir.join("a.rule.toml");
        std::fs::write(&file, RULE_A).expect("write");

        let fp1 = rules_fingerprint(&[dir.clone()]);
        assert_eq!(rules_fingerprint(&[dir.clone()]), fp1, "内容未变，指纹必须稳定");

        // 重写同样内容（模拟编辑器保存）不应改变指纹
        std::fs::write(&file, RULE_A).expect("rewrite same");
        assert_eq!(rules_fingerprint(&[dir.clone()]), fp1, "同样内容不算变化");

        // 改内容 → 变
        std::fs::write(&file, RULE_B).expect("modify");
        let fp2 = rules_fingerprint(&[dir.clone()]);
        assert_ne!(fp2, fp1, "内容变化必须体现在指纹上");

        // 文件改名（内容同）→ 变（路径参与指纹）
        std::fs::rename(&file, dir.join("b.rule.toml")).expect("rename");
        assert_ne!(rules_fingerprint(&[dir.clone()]), fp2, "路径参与指纹");

        // 删除 → 变
        let before_delete = rules_fingerprint(&[dir.clone()]);
        std::fs::remove_file(dir.join("b.rule.toml")).expect("remove");
        assert_ne!(rules_fingerprint(&[dir.clone()]), before_delete);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_fingerprint_handles_missing_and_nested_dirs() {
        let dir = temp_dir("fp_nested");
        // 不存在的目录不应 panic
        let _ = rules_fingerprint(&[dir.join("nope")]);

        std::fs::create_dir_all(dir.join("column")).expect("mkdir");
        std::fs::write(dir.join("column").join("c.rule.toml"), RULE_A).expect("write nested");
        let with_nested = rules_fingerprint(&[dir.clone()]);

        // 同样的内容放在根目录 → 路径不同 → 指纹不同
        std::fs::write(dir.join("c.rule.toml"), RULE_A).expect("write flat");
        assert_ne!(rules_fingerprint(&[dir.clone()]), with_nested);

        // 非 .toml 文件不参与
        std::fs::write(dir.join("README.md"), "not a rule").expect("write md");
        let before = rules_fingerprint(&[dir.clone()]);
        std::fs::write(dir.join("README.md"), "changed").expect("rewrite md");
        assert_eq!(rules_fingerprint(&[dir.clone()]), before, "非 toml 不参与指纹");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 端到端：改动规则文件 → 监听线程自动重载 → 新规则在规则集里生效。
    #[test]
    fn test_watcher_reloads_on_content_change() {
        let _guard = crate::tests::rule_state_guard();

        let root = temp_dir("watch_content");
        let rules_dir = get_project_rules_dir(&root);
        std::fs::create_dir_all(&rules_dir).expect("mkdir rules");
        set_watched_project_root(Some(root.clone()));

        let watcher = RulesWatcher::spawn_with(Duration::from_millis(30));
        assert_eq!(watcher.reload_count(), 0, "启动时不应触发重载");

        std::fs::write(rules_dir.join("a.rule.toml"), RULE_A).expect("write rule");

        assert!(
            wait_until(Duration::from_secs(5), || watcher.reload_count() >= 1),
            "规则文件变化应触发重载"
        );
        assert!(index_is_stale(), "重载后应置索引陈旧标记");
        assert!(
            crate::with_rules(Some(&root), |reg| Ok(reg.get("watch-rule-a").is_some()))
                .expect("with_rules"),
            "重载后新规则应生效"
        );

        watcher.stop();
        reset();
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 切换项目：宿主只改「当前项目根」，监听应自动改看重载目标。
    #[test]
    fn test_watcher_follows_project_switch() {
        let _guard = crate::tests::rule_state_guard();

        let root_a = temp_dir("watch_switch_a");
        let root_b = temp_dir("watch_switch_b");
        let rules_a = get_project_rules_dir(&root_a);
        let rules_b = get_project_rules_dir(&root_b);
        std::fs::create_dir_all(&rules_a).expect("mkdir a");
        std::fs::create_dir_all(&rules_b).expect("mkdir b");
        std::fs::write(rules_a.join("a.rule.toml"), RULE_A).expect("write a");

        set_watched_project_root(Some(root_a.clone()));
        let watcher = RulesWatcher::spawn_with(Duration::from_millis(30));

        // 切到 B：即使 B 目录内容为空，项目根变了也应触发一次重载
        set_watched_project_root(Some(root_b.clone()));
        assert!(
            wait_until(Duration::from_secs(5), || watcher.reload_count() >= 1),
            "切换项目应触发重载"
        );
        // 关键断言：重载的是 B（A 的规则不在 B 的规则集里）
        assert!(
            crate::with_rules(Some(&root_b), |reg| Ok(reg.get("watch-rule-a").is_none()))
                .expect("with_rules"),
            "切换后重载的应是新项目的规则集"
        );

        watcher.stop();
        reset();
        let _ = std::fs::remove_dir_all(&root_a);
        let _ = std::fs::remove_dir_all(&root_b);
    }

    /// 无项目时也能启动（只监听全局层），且不因缺少项目目录而失败。
    #[test]
    fn test_watcher_without_project() {
        let _guard = crate::tests::rule_state_guard();

        set_watched_project_root(None);
        let watcher = RulesWatcher::spawn_with(Duration::from_millis(20));
        assert_eq!(watcher.reload_count(), 0);
        watcher.stop();
        reset();
    }

    /// drop 即停止线程（RAII 语义：宿主只需持有句柄）。
    #[test]
    fn test_watcher_stops_on_drop() {
        let _guard = crate::tests::rule_state_guard();

        let root = temp_dir("watch_drop");
        set_watched_project_root(Some(root.clone()));
        let watcher = RulesWatcher::spawn_with(Duration::from_millis(20));
        let count_before = watcher.reload_count();
        drop(watcher); // shutdown() 会 join，返回即线程已退出

        // 线程已停：再改文件也不会增加计数（这里只能断言 drop 返回，join 本身即验证）
        std::fs::create_dir_all(get_project_rules_dir(&root)).expect("mkdir");
        std::fs::write(get_project_rules_dir(&root).join("a.rule.toml"), RULE_A).expect("write");
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(count_before, 0, "停止前不应有重载");

        reset();
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 每个用例结束后清掉本测试的全局状态，避免污染后续用例。
    fn reset() {
        set_watched_project_root(None);
        clear_index_stale();
        crate::clear_disabled_rules_cache();
    }
}
