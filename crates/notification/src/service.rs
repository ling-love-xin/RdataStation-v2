//! 通知服务
//!
//! 提供全局通知推送、合并、历史记录等核心功能。
//!
//! # 线程与持久化口径
//!
//! `notify` 跑在 UI 线程：这里**不做磁盘 I/O**，只改内存并标脏，
//! 由后台 `rds-notify-flush` 线程节流落盘；进程退出时用 [`flush_now`] 补最后一次。
//! 早先的实现在每次推送里同步 `write` 整份 JSON，连续通知会把 UI 卡住。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use gpui_kit::component::notification::{
    Notification, NotificationList, NotificationType as GpuiNotificationType,
};
use gpui_kit::{App, AppContext as _, Entity, SharedString, Window};

use crate::core::{NotificationRecord, NotificationType, MAX_HISTORY};
use crate::settings::read_notify_config;

/// 落盘文件名（放在 `<RDS_HOME>/config/` 下）
const HISTORY_FILE: &str = "notification_history.json";

/// 后台线程空闲时的兜底唤醒间隔
const FLUSH_IDLE: Duration = Duration::from_millis(500);

/// 全局 NotificationList 句柄（per-App 全局，非进程级单例）
#[derive(Clone)]
struct NotificationHandle(Entity<NotificationList>);

impl gpui_kit::Global for NotificationHandle {}

/// 历史记录存储
///
/// `records` 用 `Arc` 共享：通知中心每帧渲染都取快照，若直接 clone 整个 Vec
/// 会在弹出面板的每一帧里复制一遍记录；`Arc` 只加一次引用计数。
struct HistoryStore {
    records: Arc<Vec<NotificationRecord>>,
    dirty: bool,
}

impl HistoryStore {
    fn empty() -> Self {
        Self {
            records: Arc::new(Vec::new()),
            dirty: false,
        }
    }

    fn snapshot(&self) -> Arc<Vec<NotificationRecord>> {
        // Arc::make_mut 在后续写入时把快照隔离开，读侧拿到的就是稳定视图
        self.records.clone()
    }

    /// 记录一次推送；`merged` 为真表示命中合并窗口，走计数累加而不是新增条目
    ///
    /// 返回该主题合并后的计数，调用方据此给 toast 打 `×N`（与历史同一口径）。
    fn record(
        &mut self,
        type_: NotificationType,
        title: &str,
        message: Option<&String>,
        merged: bool,
    ) -> u32 {
        let hit = if merged {
            // 新记录总是插在头部，因此「最新一条同主题」就是第一个匹配项
            self.records
                .iter()
                .position(|r| r.type_ == type_ && r.title == title)
        } else {
            None
        };

        let count = match hit {
            Some(ix) => {
                let records = Arc::make_mut(&mut self.records);
                // 合并命中的记录必须挪到头部：它的时间戳会被刷成「刚刚」，
                // 若留在原位，A → B → A 之后列表里就是「B 排在更晚的 A 上面」。
                // 这个列表对外是扁平时间线（不分组、不额外排序），顺序一旦不代表时间，
                // 用户会把最新那条当成旧的。长度不变，不需要再 truncate。
                let mut rec = records.remove(ix);
                rec.absorb(message);
                let count = rec.count;
                records.insert(0, rec);
                count
            }
            None => {
                let rec = NotificationRecord::new(type_, title.to_string(), message.cloned());
                let count = rec.count;
                let records = Arc::make_mut(&mut self.records);
                records.insert(0, rec);
                if records.len() > MAX_HISTORY {
                    records.truncate(MAX_HISTORY);
                }
                count
            }
        };
        self.dirty = true;
        count
    }

    fn unread_count(&self) -> usize {
        self.records.iter().filter(|r| !r.read).count()
    }

    /// 返回是否真的发生了「未读 → 已读」：调用方据此决定是否唤醒落盘线程
    fn mark_read(&mut self, id: &str) -> bool {
        let records = Arc::make_mut(&mut self.records);
        // 只有从未读变已读才算「脏」：重复点同一条已读行不该触发一次落盘
        if let Some(rec) = records.iter_mut().find(|r| r.id == id)
            && !rec.read
        {
            rec.read = true;
            self.dirty = true;
            return true;
        }
        false
    }

    /// 删除单条（通知中心的 × ）。返回是否真的删掉了东西。
    fn remove(&mut self, id: &str) -> bool {
        let records = Arc::make_mut(&mut self.records);
        let before = records.len();
        records.retain(|r| r.id != id);
        let changed = records.len() != before;
        if changed {
            self.dirty = true;
        }
        changed
    }

    /// 返回是否真的发生了变更：全部已读时再点一次不该唤醒落盘线程
    fn mark_all_read(&mut self) -> bool {
        let records = Arc::make_mut(&mut self.records);
        let mut changed = false;
        for rec in records.iter_mut() {
            if !rec.read {
                rec.read = true;
                changed = true;
            }
        }
        if changed {
            self.dirty = true;
        }
        changed
    }

    /// 返回是否真的清空了：空历史再点「清空」不该唤醒落盘线程
    fn clear(&mut self) -> bool {
        if self.records.is_empty() {
            return false;
        }
        self.records = Arc::new(Vec::new());
        self.dirty = true;
        true
    }
}

/// 存储 + 后台落盘线程共用的唤醒器
struct Store {
    inner: Mutex<HistoryStore>,
    cv: Condvar,
}

impl Store {
    fn lock(&self) -> MutexGuard<'_, HistoryStore> {
        // 通知记录里没有会中毒的复合不变量；锁中毒只说明别的线程 panic 过，
        // 历史数据本身仍可用，取回内层继续，避免整个通知系统跟着失联。
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 唤醒落盘线程（在 drop 掉 guard 之后调用）
    fn kick(&self) {
        self.cv.notify_one();
    }
}

static STORE: OnceLock<Store> = OnceLock::new();

/// 后台落盘线程的启动闸（避免 `store()` 在 `OnceLock` 初始化闭包里重入）
static FLUSH_WORKER: OnceLock<()> = OnceLock::new();

fn store() -> &'static Store {
    let store = STORE.get_or_init(|| {
        // 进程内只读一次盘。首次 `store()` 的调用点是首帧渲染或首次推送——
        // 读 50 条 JSON 是微秒级，不值得为它加启动预热与额外的初始化顺序约束。
        Store {
            inner: Mutex::new(load_history()),
            cv: Condvar::new(),
        }
    });
    // 线程体只认 `STORE.get()`：此处 `STORE` 已经初始化完成，
    // 线程里再调 `store()` 会重入 OnceLock 的初始化闭包。
    FLUSH_WORKER.get_or_init(|| {
        // 线程起不来时运行期就不会自动落盘（退出时 `flush_now` 仍补一次），
        // 这个失败必须留下痕迹，否则「历史不刷新」会被误当成 UI bug 查。
        if let Err(err) = std::thread::Builder::new()
            .name("rds-notify-flush".to_string())
            .spawn(|| {
                let Some(store) = STORE.get() else {
                    return;
                };
                loop {
                    let mut guard = store.lock();
                    if !guard.dirty {
                        let (next, _) = store
                            .cv
                            .wait_timeout(guard, FLUSH_IDLE)
                            .unwrap_or_else(|e| e.into_inner());
                        guard = next;
                        if !guard.dirty {
                            continue;
                        }
                    }
                    // 只清脏标记就解锁：磁盘写入不持 STORE 锁，UI 线程不会被落盘挡住；
                    // 真正要写的内容由 `flush_snapshot` 在写锁内重读（并发时后到者落最新）
                    guard.dirty = false;
                    drop(guard);
                    if !flush_snapshot() {
                        // 写失败（磁盘满、文件被占用）：把脏标回去等下一轮。
                        // 不这么做的话这次变更就永久留不在盘上，直到下次通知才顺带写出去。
                        let mut guard = store.lock();
                        guard.dirty = true;
                        drop(guard);
                        // 脏标记已置回，直接循环会忙等重写；睡一个节流周期再试
                        std::thread::sleep(FLUSH_IDLE);
                    }
                }
            })
        {
            tracing::error!(
                error = %err,
                "通知落盘线程启动失败，历史将在进程退出时一次性写盘"
            );
        }
    });
    store
}

fn history_path() -> PathBuf {
    paths::config_dir().join(HISTORY_FILE)
}

/// 解析历史文件内容
///
/// 先按正常路径整体解析；整份失败时退回逐条解析，只留下**能读的记录**。
/// 一条坏记录（手工编辑、旧版本字段、写盘被中断留下的半截 JSON）不该让用户
/// 丢掉其余 49 条历史——这是「整体反序列化」最容易踩的坑。
fn parse_history(content: &str) -> Vec<NotificationRecord> {
    if let Ok(records) = serde_json::from_str::<Vec<NotificationRecord>>(content) {
        return records;
    }
    let Ok(values) = serde_json::from_str::<Vec<serde_json::Value>>(content) else {
        tracing::warn!("通知历史文件不是 JSON 数组，本次以空历史启动");
        return Vec::new();
    };
    let mut records = Vec::with_capacity(values.len());
    let mut skipped = 0usize;
    for value in values {
        match serde_json::from_value::<NotificationRecord>(value) {
            Ok(record) => records.push(record),
            Err(_) => skipped += 1,
        }
    }
    if skipped > 0 {
        tracing::warn!(
            skipped,
            kept = records.len(),
            "通知历史中有记录无法解析，已跳过"
        );
    }
    records
}

fn load_history() -> HistoryStore {
    let mut store = HistoryStore::empty();
    let Ok(content) = std::fs::read_to_string(history_path()) else {
        return store;
    };
    let mut records = parse_history(&content);
    records.truncate(MAX_HISTORY);
    store.records = Arc::new(records);
    store
}

/// 原子落盘到指定路径：先写同目录临时文件，再 rename 覆盖
///
/// 直接 `fs::write` 会在进程被强杀 / 崩溃时留下半截 JSON，下次启动整份历史作废。
/// 同目录 rename 在 Windows 与 Unix 上都是原子替换。返回是否写成功——
/// 调用方据此决定要不要把 `dirty` 标回去重试。
///
/// **本函数不做并发保护**：两个调用方同时进来会互相覆盖 `.tmp`，
/// 甚至把对方写了一半的内容 rename 成正式文件。所有生产路径都经
/// [`write_history_at`]（持 [`WRITE_LOCK`]）或 [`flush_snapshot`]；本函数只被它们调用。
fn write_at(path: &Path, records: &[NotificationRecord]) -> bool {
    let Some(dir) = path.parent() else {
        tracing::warn!("通知历史路径没有父目录，跳过落盘");
        return false;
    };
    if let Err(err) = std::fs::create_dir_all(dir) {
        tracing::warn!(error = %err, path = %dir.display(), "通知历史目录创建失败");
        return false;
    }
    let json = match serde_json::to_string_pretty(records) {
        Ok(json) => json,
        Err(err) => {
            tracing::warn!(error = %err, "通知历史序列化失败");
            return false;
        }
    };
    let tmp = path.with_extension("json.tmp");
    if let Err(err) = std::fs::write(&tmp, &json) {
        tracing::warn!(error = %err, path = %tmp.display(), "通知历史临时文件写入失败");
        return false;
    }
    if let Err(err) = std::fs::rename(&tmp, path) {
        // rename 失败（目标被占用、跨卷等）：清掉临时文件，别在配置目录留垃圾
        let _ = std::fs::remove_file(&tmp);
        tracing::warn!(error = %err, path = %path.display(), "通知历史替换失败");
        return false;
    }
    true
}

/// 取写锁后落盘到指定路径
///
/// 这是**唯一带并发保护的写路径**。生产侧 [`flush_snapshot`] 走同一把锁但要在锁内重读
/// store，故不复用本函数；并发测试直接调它来验证「拿同一把锁就不会写出坏文件」。
fn write_history_at(path: &Path, records: &[NotificationRecord]) -> bool {
    let _write = WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    write_at(path, records)
}

/// 落盘到 `history_path()`
fn write_history(records: &[NotificationRecord]) -> bool {
    write_history_at(&history_path(), records)
}

/// 写盘串行锁
///
/// 后台线程与 [`flush_now`]（退出路径）都可能写盘，两者共用同一个 `.tmp` 路径。
/// 并发时的实际后果不是「谁赢」，而是**互相踩**：A 写完 tmp、B 覆盖 tmp、
/// A 把 B 还没写完的内容 rename 成正式文件——轻则落一份旧快照（丢最后一次变更），
/// 重则正式文件变成半截 JSON，下次启动整份历史作废（`parse_history` 的逐条容错也救不了，
/// 因为它连 JSON 数组都不是）。写盘是毫秒级，串行化没有可感知的代价。
static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// 取写锁后**重读**当前历史再落盘
///
/// 重读而不是沿用调用方手里的快照：并发时「谁后拿到写锁」就该落「谁看到的最新内容」。
/// 否则会出现「先进入者拿着旧快照、后完成，把新快照盖掉」——最后一条变更只活在内存里。
///
/// 锁顺序固定为 `WRITE_LOCK → STORE`；没有任何路径反着拿（写盘期间不持 STORE 锁），
/// 因此不会死锁。
fn flush_snapshot() -> bool {
    let _write = WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(store) = STORE.get() else {
        return true;
    };
    let records = {
        let guard = store.lock();
        guard.records.clone()
    };
    write_at(&history_path(), &records)
}

/// 立即把历史写盘（进程退出前调用；正常运行时不必手动调）
///
/// 注意顺序：`on_app_quit` 里必须先调它再做其他清理，否则可能还没等到写盘
/// 进程就被收掉了。
///
/// 这里**不看 `dirty` 就写**：后台线程可能刚清掉脏标记、正写到一半，此时进程退出会把它
/// 杀掉（留下半截 `.tmp` 或让正式文件停在旧版本）。`flush_snapshot` 取写锁会等它写完，
/// 再在锁内重读最新记录补写一次，确保退出路径上不存在「在飞的写盘」。
///
/// 唯一的例外是「内存里没有记录、磁盘上也没有文件」——那是从未产生过通知的应用，
/// 不该凭空在配置目录里建一个空历史。
pub fn flush_now() {
    let Some(store) = STORE.get() else {
        return;
    };
    let nothing_to_write = store.lock().records.is_empty() && !history_path().exists();
    if nothing_to_write {
        return;
    }
    if !flush_snapshot() {
        tracing::warn!("退出时通知历史落盘失败，最后一次变更未能写入磁盘");
    }
}

/// 合并窗口：同 (类型, 标题) 在窗口内只保留一条
///
/// 键是业务键而不是单槽：早先只留了一个 `PendingMerge`，A/B/A 交替推送时
/// 合并直接失效，同源风暴会照样刷屏。
static MERGES: OnceLock<Mutex<HashMap<(NotificationType, String), Instant>>> = OnceLock::new();

fn merge_slots() -> &'static Mutex<HashMap<(NotificationType, String), Instant>> {
    MERGES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 判定本次推送是否落在合并窗口内
///
/// 窗口从**首次**推送开始计，命中不重置：否则持续推送会把窗口一路延长，
/// 一条 `×N` 永远不落定。返回 true 时调用方应累加计数而不是新建记录。
fn hit_merge_window(type_: NotificationType, title: &str, window: Duration) -> bool {
    if window.is_zero() {
        return false;
    }
    let key = (type_, title.to_string());
    let mut slots = merge_slots().lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    // 顺便清掉过期槽位：不清的话键会随标题无限增长
    slots.retain(|_, first_at| now.duration_since(*first_at) < window);
    if slots.contains_key(&key) {
        return true;
    }
    slots.insert(key, now);
    false
}

/// 初始化全局通知系统
///
/// 在 WorkbenchView 初始化时调用一次，创建 NotificationList entity。
pub fn init_notifications(window: &mut Window, cx: &mut App) -> Entity<NotificationList> {
    if let Some(handle) = cx.try_global::<NotificationHandle>() {
        return handle.0.clone();
    }
    let list = cx.new(|cx| NotificationList::new(window, cx));
    cx.set_global(NotificationHandle(list.clone()));
    list
}

/// 获取全局 NotificationList entity（懒初始化）
///
/// 在 render 时调用，确保第一次 render 时创建 NotificationList。
pub fn ensure_notifications(window: &mut Window, cx: &mut App) -> Entity<NotificationList> {
    init_notifications(window, cx)
}

/// 一次推送的通道去向
///
/// 由「静音弹出」「静音系统」与「该类型是否推系统」三个开关组合而来。
/// 单独抽出来不是因为代码短，而是因为**早退条件**与**vendor delivery 映射**
/// 必须出自同一个判断：分散成两处（`if !in_app && !to_system { return }` 与后面的
/// `match (in_app, to_system)`）时，改一处忘另一处就会变成「早退了却还在组 delivery」，
/// 或者「两组开关都关却仍走了 System 分支」。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Delivery {
    /// 两条通道都静音：只留历史
    HistoryOnly,
    /// 只弹 in-app toast
    InAppOnly,
    /// 只推系统通知（vendor `System` delivery，不建 toast 实体）
    SystemOnly,
    /// 弹出 + 系统通知
    Both,
}

impl Delivery {
    fn plan(in_app: bool, to_system: bool) -> Self {
        match (in_app, to_system) {
            (true, true) => Self::Both,
            (false, true) => Self::SystemOnly,
            (true, false) => Self::InAppOnly,
            (false, false) => Self::HistoryOnly,
        }
    }
}

/// 推送一个通知
///
/// # 参数
/// - `type_` - 通知类型
/// - `title` - 通知标题
/// - `message` - 通知内容（可选）
/// - `cx` - App 上下文
pub fn notify(
    type_: NotificationType,
    title: impl Into<String>,
    message: Option<impl Into<String>>,
    cx: &mut App,
) {
    let title = title.into();
    let msg = message.map(|m| m.into());

    // 治理（2026-10-30）：UI 可见通知必须进日志——错误/警告按级别记录，
    // 信息/成功只留 debug（避免成功通知刷屏）。此前通知自身不打日志，
    // 造成「页面有报错但日志没有」。
    match type_ {
        NotificationType::Error => tracing::error!(title = %title, message = ?msg, "UI 错误通知"),
        NotificationType::Warning => tracing::warn!(title = %title, message = ?msg, "UI 警告通知"),
        _ => tracing::debug!(title = %title, message = ?msg, "UI 通知"),
    }

    let config = read_notify_config(cx);
    // 系统通道 =「静音系统通知」全局开关 ∧ 该类型的「推送到系统」开关。
    // 两者不合并成一个：静音是「全屏工作时临时闭嘴」，类型开关是长期偏好，
    // 合并后用户改了类型开关却不生效，会以为设置坏了。
    let to_system = !config.silent_system && config.should_deliver_to_system(type_);
    // in-app 通道 =「静音弹出通知」取反。
    let delivery = Delivery::plan(!config.silent_in_app, to_system);
    let merge_window = Duration::from_secs(config.merge_window_secs as u64);
    let merged = hit_merge_window(type_, &title, merge_window);

    // 历史先落：静音通道时同样要留痕，用户之后仍能在通知中心翻到。
    // 顺带拿到合并后的计数，弹出标题与历史里的 `×N` 用同一个数（两处不能各算一套）。
    let count = {
        let store = store();
        let mut guard = store.lock();
        let count = guard.record(type_, &title, msg.as_ref(), merged);
        drop(guard);
        store.kick();
        count
    };

    // 历史已变，先刷新状态栏铃铛的未读计数。
    // 必须放在所有早退分支之前：双通道都静音、list 未初始化、无窗口三种情况都不会弹出，
    // 若不在这里刷新，用户会看到「详情页报了 5 个错，铃铛还是 0」。
    crate::view::notify_changed(cx);

    // 通道决策留痕（debug）：实测「该弹的没弹」时先看这一行——它把两条通道的最终结论
    // 和合并计数一次给出，不用让排查者从三四个开关反推是哪一步拦下的。
    tracing::debug!(
        notification_type = ?type_,
        delivery = ?delivery,
        merged,
        count,
        "通知通道决策"
    );

    // === 两条通道都静音：只进历史 ===
    // toast 与系统通知是两个独立开关，只有都关掉才完全不弹；
    // 只静音 in-app 时仍要走 vendor 的 System delivery（见下方映射）。
    if delivery == Delivery::HistoryOnly {
        tracing::debug!("in-app 与系统通知均已静音，仅记录历史: {}", title);
        return;
    }

    let Some(list) = cx
        .try_global::<NotificationHandle>()
        .map(|handle| handle.0.clone())
    else {
        tracing::warn!("NotificationList 尚未初始化，本次跳过弹出与系统通知: {}", title);
        return;
    };

    // 与通知中心里的 `×N` 用同一种写法：两处文案不一致时，用户会以为是两个不同的计数
    let display_title = if count > 1 {
        format!("{title} ×{count}")
    } else {
        title.clone()
    };

    let gpui_type = type_.to_gpui_type();
    let mut notification = match gpui_type {
        GpuiNotificationType::Info => Notification::info(SharedString::from(display_title.as_str())),
        GpuiNotificationType::Success => {
            Notification::success(SharedString::from(display_title.as_str()))
        }
        GpuiNotificationType::Warning => {
            Notification::warning(SharedString::from(display_title.as_str()))
        }
        GpuiNotificationType::Error => {
            Notification::error(SharedString::from(display_title.as_str()))
        }
    }
    // `id1` 的 `T` 只用于把 key 限定到某个类型空间（内部拼 `TypeId + key`），
    // 这里不需要区分来源，统一用 `()`：同一 (类型, 标题) 就复用同一个 toast 槽位。
    .id1::<()>(SharedString::from(format!(
        "rds-notify::{:?}::{}",
        type_, title
    )));

    if let Some(ref msg) = msg {
        notification = notification.message(SharedString::from(msg.as_str()));
    }

    // 四种类型各一份秒数：0 = 不自动消失，否则按秒数超时。
    // 早先 Success/Info 共用一个时长，而系统通知开关却是分开的——
    // 「信息静默、成功要弹」这种组合当时表达不出来。
    let autohide_secs = match type_ {
        NotificationType::Success => config.success_autohide_secs,
        NotificationType::Info => config.info_autohide_secs,
        NotificationType::Warning => config.warning_autohide_secs,
        NotificationType::Error => config.error_autohide_secs,
    };
    notification = if autohide_secs == 0 {
        notification.autohide(false)
    } else {
        notification
            .autohide(true)
            .timeout(Duration::from_secs(autohide_secs as u64))
    };

    // 通道映射：`Both` / `SystemOnly` 走 vendor 的 delivery 组合；
    // `InAppOnly` 用默认 `InApp`；`HistoryOnly` 已在上面早退。
    notification = match delivery {
        Delivery::Both => notification.in_app_and_system(),
        Delivery::SystemOnly => notification.system(),
        Delivery::InAppOnly | Delivery::HistoryOnly => notification,
    };

    // 优先推到**当前聚焦窗口**：设置页/对话框是独立窗口，用固定"第一个窗口"时
    // toast 会跑到被遮挡的主窗口后面，用户以为通知没弹。全部窗口在后台时退回第一个。
    // System delivery 同样要求窗口：vendor 会把 window_handle 登记进
    // SystemNotificationRegistry，用做点击系统通知后的回跳。
    let Some(window) = cx
        .active_window()
        .or_else(|| cx.windows().first().copied())
    else {
        tracing::warn!("无可用窗口，本次跳过弹出与系统通知: {}", title);
        return;
    };
    // 窗口在 `active_window()` 到这一刻之间被关掉是可能的：历史已经写入，只是这次不弹。
    if window
        .update(cx, move |_, window, cx| {
            list.update(cx, |list, cx| {
                list.push(notification, window, cx);
            });
        })
        .is_err()
    {
        tracing::debug!("通知已进入历史，但目标窗口已关闭，本次不弹: {}", title);
    }
}

/// 推送一个信息通知
pub fn notify_info(title: impl Into<String>, message: Option<impl Into<String>>, cx: &mut App) {
    notify(NotificationType::Info, title, message, cx);
}

/// 推送一个成功通知
pub fn notify_success(title: impl Into<String>, message: Option<impl Into<String>>, cx: &mut App) {
    notify(NotificationType::Success, title, message, cx);
}

/// 推送一个警告通知
pub fn notify_warning(title: impl Into<String>, message: Option<impl Into<String>>, cx: &mut App) {
    notify(NotificationType::Warning, title, message, cx);
}

/// 推送一个错误通知
pub fn notify_error(title: impl Into<String>, message: Option<impl Into<String>>, cx: &mut App) {
    notify(NotificationType::Error, title, message, cx);
}

/// 历史快照（通知中心用；`Arc` 共享，读到的是稳定视图）
pub fn history() -> Arc<Vec<NotificationRecord>> {
    store().lock().snapshot()
}

/// 获取未读计数（铃铛图标用）
pub fn unread_count() -> usize {
    store().lock().unread_count()
}

/// 把单条标记为已读（通知中心点某一行）
///
/// 返回是否发生变更：点一条**已经已读**的行不该唤醒落盘线程
/// （否则每次误点都会让 flush worker 白转一圈）。
pub fn mark_read(id: &str) -> bool {
    let store = store();
    let changed = store.lock().mark_read(id);
    if changed {
        store.kick();
    }
    changed
}

/// 删除单条历史（通知中心行尾的 ×）
///
/// 返回是否真的删掉了：删一条不存在的记录不唤醒落盘线程。
pub fn remove(id: &str) -> bool {
    let store = store();
    let changed = store.lock().remove(id);
    if changed {
        store.kick();
    }
    changed
}

/// 标记全部已读
pub fn mark_all_read() {
    let store = store();
    if store.lock().mark_all_read() {
        store.kick();
    }
}

/// 清除所有历史
pub fn clear_history() {
    let store = store();
    if store.lock().clear() {
        store.kick();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 四种开关组合必须一一对应四种去向；任何一格变化都会让「早退」与「delivery 映射」
    /// 走上不同的分支。
    #[test]
    fn delivery_plan_covers_all_four_switch_combinations() {
        assert_eq!(Delivery::plan(true, true), Delivery::Both);
        assert_eq!(Delivery::plan(false, true), Delivery::SystemOnly);
        assert_eq!(Delivery::plan(true, false), Delivery::InAppOnly);
        assert_eq!(Delivery::plan(false, false), Delivery::HistoryOnly);
        // 只有都关才是「只留历史」——单关任一条都必须仍有东西送达
        assert_ne!(Delivery::plan(true, false), Delivery::HistoryOnly);
        assert_ne!(Delivery::plan(false, true), Delivery::HistoryOnly);
    }

    #[test]
    fn history_store_merges_into_the_head_record() {
        let mut store = HistoryStore::empty();
        store.record(NotificationType::Error, "执行失败", None, false);
        store.record(
            NotificationType::Error,
            "执行失败",
            Some(&"新原因".to_string()),
            true,
        );
        assert_eq!(store.records.len(), 1, "合并命中不该新增条目");
        assert_eq!(store.records[0].count, 2);
    }

    /// A → B → A：合并命中会刷新 A 的时间戳，A 就必须回到列表头部。
    /// 否则扁平时间线里 B 会排在更新的 A 上面，用户以为 A 是旧的。
    #[test]
    fn merged_record_moves_back_to_the_top() {
        let mut store = HistoryStore::empty();
        store.record(NotificationType::Warning, "甲", None, false);
        store.record(NotificationType::Warning, "乙", None, false);
        assert_eq!(store.records[0].title, "乙", "新记录前插");

        // 再推甲，命中合并窗口
        store.record(NotificationType::Warning, "甲", None, true);
        assert_eq!(store.records.len(), 2, "合并不该新增条目");
        assert_eq!(store.records[0].title, "甲", "刚被刷新的那条必须在最上面");
        assert_eq!(store.records[0].count, 2);
        assert_eq!(store.records[1].title, "乙", "其余顺序不变");
    }

    #[test]
    fn history_store_truncates_at_limit() {
        let mut store = HistoryStore::empty();
        for i in 0..(crate::core::MAX_HISTORY + 10) {
            store.record(NotificationType::Info, &format!("通知 {i}"), None, false);
        }
        assert_eq!(store.records.len(), crate::core::MAX_HISTORY);
    }

    #[test]
    fn unread_and_read_bookkeeping() {
        let mut store = HistoryStore::empty();
        store.record(NotificationType::Info, "甲", None, false);
        store.record(NotificationType::Info, "乙", None, false);
        assert_eq!(store.unread_count(), 2);

        let id = store.records[0].id.clone();
        store.mark_read(&id);
        assert_eq!(store.unread_count(), 1);

        store.mark_all_read();
        assert_eq!(store.unread_count(), 0);

        store.clear();
        assert!(store.records.is_empty());
    }

    #[test]
    fn mark_read_only_reports_real_changes() {
        let mut store = HistoryStore::empty();
        store.record(NotificationType::Error, "失败", None, false);
        let id = store.records[0].id.clone();

        assert!(store.mark_read(&id), "首次未读 → 已读应有变更");
        assert!(!store.mark_read(&id), "重复点已读行不该再算一次变更");
        assert!(!store.mark_read("不存在的 id"), "未知 id 不算变更");
    }

    #[test]
    fn remove_drops_one_record() {
        let mut store = HistoryStore::empty();
        store.record(NotificationType::Info, "甲", None, false);
        store.record(NotificationType::Info, "乙", None, false);
        let id = store.records[0].id.clone();

        assert!(store.remove(&id));
        assert_eq!(store.records.len(), 1);
        // `record` 是新记录前插（`insert(0, …)`）：推甲、乙 之后列表是 [乙, 甲]，
        // 删掉 `records[0]`（乙）应只剩甲
        assert_eq!(store.records[0].title, "甲", "只该删掉指定那条");
        assert!(!store.remove(&id), "重复删除同一个 id 不该算变更");
    }

    /// 一条坏记录不该毁掉其余历史（整体反序列化最容易踩的坑）
    #[test]
    fn parse_history_keeps_readable_records() {
        // 第一条里带着已从模型移除的 `delivered_to_system`——旧版本写出的历史文件长这样，
        // 顺手验证「未知字段被忽略、不导致整条解析失败」。别把这个字段从样例里清理掉。
        let content = r#"[
            {"id":"a","type_":"Error","title":"失败","message":null,"timestamp":"2026-01-01T00:00:00+08:00","count":2,"read":false,"delivered_to_system":true},
            {"id":"b","type_":"Info"},
            {"id":"c","type_":"Warning","title":"告警","message":"细节","timestamp":"2026-01-01T00:00:00+08:00","count":1,"read":true,"delivered_to_system":false}
        ]"#;
        // 中间那条缺 title（必填字段）：整份解析必然失败，逐条解析应保留 2 条
        let records = parse_history(content);
        assert_eq!(records.len(), 2, "只该跳过无法解析的那一条");
        assert_eq!(records[0].title, "失败");
        assert_eq!(records[0].count, 2);
        assert!(!records[0].read);
        assert_eq!(records[1].title, "告警");
    }

    /// 字段演进（旧版本缺字段）不能让整份历史作废
    #[test]
    fn parse_history_fills_missing_optional_fields() {
        let records = parse_history(r#"[{"id":"a","title":"最小记录"}]"#);
        assert_eq!(records.len(), 1);
        let rec = &records[0];
        assert_eq!(rec.type_, NotificationType::Info, "缺 type_ 退回默认 Info");
        assert_eq!(rec.count, 1, "缺 count 按 1 计");
        assert!(!rec.read, "缺 read 视为未读");
        assert!(rec.message.is_none());
        assert!(rec.timestamp.is_empty());
    }

    #[test]
    fn parse_history_survives_garbage() {
        assert!(parse_history("not json").is_empty());
        assert!(
            parse_history(r#"{"id":"x"}"#).is_empty(),
            "不是数组就返回空"
        );
    }

    /// 批量操作与单条操作保持同一口径：无变更就不唤醒落盘线程
    #[test]
    fn bulk_operations_only_report_real_changes() {
        let mut store = HistoryStore::empty();
        assert!(!store.clear(), "空历史清空不算变更");
        assert!(!store.mark_all_read(), "空历史全部已读不算变更");

        store.record(NotificationType::Info, "甲", None, false);
        assert!(store.mark_all_read(), "有未读时应算变更");
        assert!(!store.mark_all_read(), "已全部已读后再点不算变更");
        assert!(store.clear(), "有记录时清空应算变更");
        assert!(store.records.is_empty());
    }

    /// 真写盘再读回：验证原子写路径本身（`test-support` 把数据根隔离到临时目录）
    #[test]
    fn history_file_round_trips_through_the_write_path() {
        // 这条断言是必须的护栏：`test-support` 一旦没生效，这个测试会往**真实配置目录**
        // 写 `notification_history.json`，把用户的历史盖掉。宁可测试红，也不许悄悄污染。
        assert!(
            paths::is_isolated_root(),
            "测试写盘必须落在隔离临时根（paths 的 test-support 未生效）"
        );

        let mut store = HistoryStore::empty();
        store.record(
            NotificationType::Error,
            "执行失败",
            Some(&"连接超时".to_string()),
            false,
        );
        store.record(NotificationType::Info, "已连接", None, false);

        assert!(write_history(&store.records), "落盘应成功");
        assert!(
            !history_path().with_extension("json.tmp").exists(),
            "rename 之后不该留下临时文件"
        );

        let loaded = load_history();
        assert_eq!(loaded.records.len(), 2);
        // `record` 前插：先推「执行失败」，再推「已连接」，读回顺序应保持
        assert_eq!(loaded.records[0].title, "已连接");
        assert_eq!(loaded.records[1].title, "执行失败");
        assert_eq!(loaded.records[1].message.as_deref(), Some("连接超时"));
        assert!(!loaded.records[1].read);
    }

    /// 并发写同一路径不能产出坏文件
    ///
    /// 这条测试守的是 `WRITE_LOCK` 本身：把 [`write_history_at`] 里的取锁去掉，四个线程
    /// 会各自往同一个 `.tmp` 里写、再抢着 rename，结果是「文件里混两次快照」或
    /// 「rename 时 tmp 已被对方改名走」——断言会红。
    ///
    /// 用独立的子目录而不是 `history_path()`：同一个测试进程里
    /// `history_file_round_trips_through_the_write_path` 也在写 `history_path()`，
    /// 并行跑会互相踩（那是另一个测试的职责，不是本测试要验证的东西）。
    #[test]
    fn concurrent_writes_never_corrupt_the_file() {
        assert!(
            paths::is_isolated_root(),
            "并发写测试必须落在隔离临时根（paths 的 test-support 未生效）"
        );

        const THREADS: u32 = 4;
        const ROUNDS: u32 = 40;
        const LEN: usize = 200;

        let dir = paths::config_dir().join("concurrent-write-test");
        let path = dir.join("notification_history.json");
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(THREADS as usize));

        let mut handles = Vec::new();
        for round in 0..THREADS {
            let path = path.clone();
            let barrier = barrier.clone();
            handles.push(std::thread::spawn(move || {
                barrier.wait();
                for i in 0..ROUNDS {
                    let records: Vec<NotificationRecord> = (0..LEN)
                        .map(|n| NotificationRecord {
                            id: format!("{round}-{i}-{n}"),
                            type_: NotificationType::Error,
                            title: format!("线程 {round}"),
                            message: Some(format!("第 {i} 次 / 第 {n} 条")),
                            timestamp: "2026-01-01T00:00:00+08:00".to_string(),
                            count: 1,
                            read: false,
                        })
                        .collect();
                    assert!(write_history_at(&path, &records), "并发写应始终成功");
                }
            }));
        }
        for handle in handles {
            handle.join().expect("写线程不该 panic");
        }

        let content = std::fs::read_to_string(&path).expect("并发写后文件应存在");
        let parsed: Vec<NotificationRecord> =
            serde_json::from_str(&content).expect("并发写后文件必须是完整 JSON 数组");
        assert_eq!(parsed.len(), LEN, "落盘的必须是某一次完整快照");
        let owner = parsed[0].id.split('-').next().unwrap().to_string();
        assert!(
            parsed.iter().all(|r| r.id.starts_with(&format!("{owner}-"))),
            "文件里不能混入两个线程的记录"
        );
        assert!(
            !path.with_extension("json.tmp").exists(),
            "并发写不该留下临时文件"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
