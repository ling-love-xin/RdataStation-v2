# `rds-notification` 设计与实现

> 状态：已实现并接入工作台（2026-11-21）。
> 本文描述**当前代码**的行为与取舍；不再保留「待开发」清单，未做的部分集中在 §8。

## 1. 定位

通知系统的唯一入口。其他 crate 只调 `notify_*`，不自己弹窗、不自己写盘。

分成两件事，职责不重叠：

| 通道 | 载体 | 生命周期 |
|------|------|----------|
| 即时提醒 | gpui-kit `NotificationList` 的 toast | 秒级，可自动消失 |
| 事后回看 | 本 crate 的 `HistoryStore` | 跨重启，最多 50 条 |
| 系统级 | `NotificationDelivery::System` → OS 通知中心 | 由操作系统管理，点击可回跳窗口 |

前两条共用同一份记录与同一个计数：toast 标题上的 `×N` 与通知中心里的 `×N` 是同一个数。
第三通道复用同一个 `Notification` 实例（`in_app_and_system()`），标题里的 `×N` 也同源。

## 2. 分层

```
crates/notification/
├── src/lib.rs       # 公共导出
├── src/core.rs      # 纯数据：类型、记录、配置、相对时间格式化（零 I/O）
├── src/service.rs   # 推送、合并、节流落盘、已读/清空
├── src/settings.rs  # 设置槽位 → NotifyConfig 的桥
└── src/view.rs      # 状态栏铃铛 + 通知中心浮层
```

依赖方向：`view → service → core`，`settings` 只被 `service` 读配置时用到。`core` 不依赖 gpui 的任何运行时设施，因此可以在普通 `#[test]` 里直接跑。

## 3. 数据模型（`core.rs`）

```rust
pub struct NotificationRecord {
    pub id: String,               // 历史侧标识（UUID v4）；必填，缺失无法定位「标记已读 / 删除」
    pub type_: NotificationType,  // Info / Success / Warning / Error；缺失按 Info
    pub title: String,            // 必填，缺失是一块空白行
    pub message: Option<String>,  // 缺失为 None
    pub timestamp: String,        // RFC 3339（Local 时区）；缺失为空串
    pub count: u32,               // 合并次数；缺失按 1
    pub read: bool,               // 缺失视为未读
}
```

模型里刻意**不留只写不读的字段**：早先有一版带 `delivered_to_system`（记录本次是否推送到系统），但它没有任何读取方——UI 不展示、逻辑不消费，而且开关改了或合并命中后它的值会漂。诊断「这条为什么没进系统通知」改用 `notify()` 的 debug「通知通道决策」日志，那里给的是**当时**的两条通道结论，比一份事后快照可信。旧历史文件里的这个字段会被 serde 直接忽略，不影响解析（`parse_history_keeps_readable_records` 的样例里就留着它，用作未知字段的回归样本）。

除 `id` / `title` 外全部带 `#[serde(default)]`：历史文件虽然是本系统自己写的，但它会被旧版本写、会被用户手工改、也可能被写到一半中断。整体反序列化只要碰到一个缺字段就整份失败，代价是用户丢掉全部历史——这份代价远大于「缺字段用默认值」。

要点：

- `count` 带 `#[serde(default = "…")]`：老版本落盘的记录没有这个字段，读进来按 1 算，不因升级丢历史。
- `absorb(message)` 是合并的唯一实现：`count += 1`、时间戳刷新到最新、`read` 重置为 false、正文取最新一次的内容——**包括最新一次没有正文（置 `None`）**。只在 `Some` 时覆盖会造成「这次推送没有正文，历史里却还留着上一条的正文」：toast 与系统通知用的都是本次 `message`，同一条记录两套口径。因此历史正文严格等于最后一次推送的正文。`count` 递增后「同一条错误连报 5 次」在列表里仍是一行。
- `format_relative_time(&str, now: DateTime<Local>)` 负责展示层换算：

  | 距今 | 输出 |
  |------|------|
  | < 60 s | `刚刚` |
  | < 1 h | `N 分钟前` |
  | 同日 | `今天 HH:MM` |
  | 昨天 | `昨天 HH:MM` |
  | 更早 | `MM-DD HH:MM` |
  | 解析失败 / 时钟回拨 | 原样返回 / 退化为 `MM-DD HH:MM` |

  时钟回拨必须处理：否则会显示「-5 分钟前」。

## 4. 存储与线程（`service.rs`）

```rust
struct HistoryStore { records: Arc<Vec<NotificationRecord>>, dirty: bool }
struct Store { inner: Mutex<HistoryStore>, cv: Condvar }
static STORE: OnceLock<Store>;
static FLUSH_WORKER: OnceLock<()>;
static WRITE_LOCK: Mutex<()>;
```

- **进程级单例**：`OnceLock` 在首次 `store()` 时读盘（`<config_dir>/notification_history.json`）。首次调用点是首帧渲染（`history()` / `unread_count()`）或首次推送，读 50 条 JSON 是微秒级，没有加启动预热的必要。
- **节流落盘**：`record()` 只置 `dirty` 并 `notify` 条件变量；后台线程 `rds-notify-flush` 在 `FLUSH_IDLE = 500 ms` 内无新写入才快照落盘。绝不在 UI 事件路径上同步写文件。
- **原子写**：先写 `notification_history.json.tmp`，再 `rename` 覆盖。直接 `fs::write` 在进程被强杀时会留下半截 JSON，下次启动整份历史作废；同目录 rename 在 Windows 与 Unix 上都是原子替换。rename 失败（目标被占用等）时清掉临时文件，不在配置目录留垃圾。
- **写盘串行 + 锁内重读**：后台线程与 `flush_now()`（退出路径）都会写盘，且共用同一个 `.tmp` 路径。并发时不加锁会出现「A 写完 tmp、B 覆盖 tmp、A 把 B 还没写完的内容 rename 成正式文件」——轻则落一份旧快照，重则正式文件变成半截 JSON（连 `parse_history` 的逐条容错都救不了，因为它已经不是 JSON 数组）。因此 `write_at()`（纯 IO）之上有两层入口，都取同一把 `WRITE_LOCK`：`write_history_at(path, records)` 取锁后落盘；`flush_snapshot()` 取锁后**先重读** store 当前记录再落盘。重读而不是沿用调用方手里的快照，是为了保证「谁后拿到写锁就落谁看到的最新内容」；锁顺序固定 `WRITE_LOCK → STORE`，写盘期间不持 STORE 锁，无死锁。`write_at()` 本身不做并发保护，只被这两个入口调用。这条不变量有回归测试守（见 §9 的 `concurrent_writes_never_corrupt_the_file`：摘掉锁后该测试立即红在 `write_history_at` 返回 false）。
- **写失败可重试**：`flush_snapshot()` 返回 `bool`。后台线程失败后把 `dirty` 标回去、睡一个 `FLUSH_IDLE` 再写；不标回的话这次变更会永远留在内存里，直到下次通知才顺带写出去。`flush_now()` 失败同样标回（退出路径上没有重试机会，但语义上「这份变更没落盘」）。
- **加载容错**：先整体解析，失败后**逐条**解析（`Vec<serde_json::Value>` → 逐条 `from_value`），跳过坏记录并 `warn`。一条坏记录（手工编辑、旧版本字段、写到一半的 JSON）不该让用户丢掉其余 49 条。
- **字段向前兼容**：`NotificationRecord` 除 `id` / `title` 外全部带 `#[serde(default)]`，缺字段就用默认值。只有 `id` 与 `title` 必填——ID 缺失无法定位「标记已读 / 删除」，标题缺失是一块空白行，跳过它们比留下废行更合理。
- **批量操作与单条操作同一口径**：`mark_read` / `remove` / `mark_all_read` / `clear` 都返回「是否真的发生变更」，`service` 层据此决定要不要 `kick()`。对已全部已读的历史再点「全部已读」、对空历史点「清空」，都不唤醒落盘线程。
- **退出兜底**：`flush_now()` 挂到 `on_app_quit`（见 §7）。它**不看 `dirty` 就写**：后台线程可能刚清掉脏标记、正写到一半，此时退出会把它杀掉（留下半截 `.tmp`，或让正式文件停在旧版本）。`flush_snapshot` 取写锁会等它写完，再在锁内重读最新记录补写一次。唯一跳过的情况是「内存无记录且磁盘无文件」——从未产生过通知的应用不该凭空建一个空历史。写失败只 `warn`（退出路径上已没有重试机会，也没有人再读 `dirty`）。
- **中毒恢复**：`Mutex` 中毒时取回内层数据继续用（记录不致命，没必要 panic）。

合并窗口另存一个 `HashMap<(NotificationType, String), Instant>`，**按 (类型, 标题) 分槽**而非单槽：

- 槽位从**首次**推送起算，命中不重置计时（避免「一直被合并、永远不超时」）；
- 每次检查都会清理过期槽位，表不会无限增长。

单槽实现的问题是 A/B/A 交替推送时合并直接失效，这里必须分槽。

命中合并后记录会**移到列表头部**：`absorb` 把时间戳刷成最新，若留在原位，A → B → A 之后列表里就是「B 排在更晚的 A 上面」。通知中心是扁平时间线（不分组、不额外排序），顺序一旦不代表时间，用户就会把最新那条当成旧的。长度不变，不需要再截断。

## 5. 推送流程

**全仓只有一个出口**：`crates/` 下（`render.rs` 的挂载点除外）没有任何第二处直接构造
`Notification` 或调 `NotificationList::push`，所有业务代码都走 `notify_*` 一族。这是三条
通道能保持一致的前提——静音、合并、历史记账、系统开关四项判定全部在这一个函数里做，
任何新增调用方只要绕过它，就会同时绕过这四项。

```
1. 打日志（Error→error!, Warning→warn!, 其他→debug!）
2. 读 NotifyConfig
3. 通道判定：`in_app = !silent_in_app`；`to_system = !silent_system && 该类型 *_to_system`
4. hit_merge_window(type, title)
5. HistoryStore::record(...) → 拿到合并后的 count（无论哪条通道被静音都记账）
6. kick 落盘线程
7. view::notify_changed(cx)     ← 必须早于所有 return，否则铃铛读数不动
   （紧随一条 `debug!`「通知通道决策」：delivery / merged / count —— 实测排查入口）
8. 两条通道都关？ → 到此为止（只留历史）
9. NotificationList 未初始化？ → 警告并返回
10. 构造 Notification：
    · 标题带 `×N`（N > 1 时）
    · id1::<()>("rds-notify::{类型}::{标题}") → 同一条只替换、不堆叠
    · autohide / timeout 按类型档位（四类各一份秒数）
    · delivery 由 `Delivery::plan(in_app, to_system)` 决定（四值枚举：
        HistoryOnly / InAppOnly / SystemOnly / Both）
      —— 早退条件与 vendor delivery 映射共用这一个判断，不写成两处独立表达式
        Both → in_app_and_system()；SystemOnly → system()；InAppOnly → 默认 InApp
11. 优先推给 `cx.active_window()`，全部在后台时退回 `cx.windows().first()`
```

第 7 步放的位置是有意为之：两条通道都可能被静音，此时没有 toast，`NotificationList` 不会 notify，铃铛计数只能靠这条路径刷新。

第 10 步的 delivery 组合依赖 vendor 的三值枚举：`vendor/gpui-component/src/notification.rs` 的 `NotificationDelivery` 是 `InApp` / `System` / `InAppAndSystem`，`push` 里先 `if delivery.includes_system() { Self::push_system(...) }`，再 `if !delivery.includes_in_app() { return }`。所以「静音 in-app 但保留系统通知」不需要绕过 vendor，直接给 `Notification::system()` 即可——系统通知照发，toast 实体根本不创建。这是把「勿扰」拆成两个独立开关的技术前提。

第 10 步的 `id1` 同时管住了两件事：一是 in-app toast 的 id（同 id 的 `Notification` 被替换而不是叠加），二是**系统通知的 tag**——`push_system` 取 `let tag = notification.id.system_tag()`，tag 由 id 派生，所以合并窗口内连推 5 次同标题通知，平台侧也是替换最新那条，不会堆 5 个系统弹窗。这一点曾怀疑是缺陷，读源码后确认不成立。

`autohide` 档位（四种类型走**同一段规则**：秒数为 0 就常驻，否则按秒数超时）：

| 类型 | 时长（设置项） | 默认 |
|------|------|------|
| Success | `success_autohide_secs` | 3 秒 |
| Info | `info_autohide_secs` | 3 秒 |
| Warning | `warning_autohide_secs` | 5 秒 |
| Error | `error_autohide_secs` | 0（常驻） |

Success 与 Info 早先共用一个时长，而系统通知开关却是按类型分开的——「信息静默、成功要弹」这种组合当时表达不出来。现在四类各一份，与 `*_to_system` 的粒度对齐。Error 也曾用 `error_autohide: bool` + 复用警告时长，设置页一处开关一处下拉、语义两套；统一成秒数口径后，四类共用同一段 `if secs == 0 { autohide(false) } else { timeout(secs) }`。

### 三条通道的设置维度

| 通道 | 开关 | 说明 |
|------|------|------|
| 弹出 toast | 四个 `*_autohide_secs` + `silent_in_app` | 时长按类型；`silent_in_app` 是全局临时静音，不影响历史与系统通知 |
| 历史 / 铃铛 | 无开关 | 有意为之：历史零打扰、成本低，静音时也照记 |
| Windows/macOS 系统通知 | 四个 `*_to_system` + `silent_system` | 类型开关管长期偏好，`silent_system` 管「全屏工作时全静音」；两者取交集 |

两个静音开关取代了原来的 `do_not_disturb` 总开关：旧开关一刀切关掉两条通道，做不到「全屏工作时只留系统通知」或「只留弹出但不进系统通知中心」。现在四种组合都可达，其中「两条都关」等价于旧的勿扰。

## 6. 通知中心 UI（`view.rs`）

### 结构

```
状态栏 ── Button(icon=Bell|BellOff, label=未读数, tooltip=随静音状态变化)
              └ Popover(anchor=BottomRight, offset=8px)
                    ├ 头部：标题 + 统计 + [全部已读] [清空]
                    ├ Separator(显式 h=1px)
                    ├ TabBar(segmented)  全部 / 未读 / 问题
                    └ 列表（width 360，max_h 360，可滚）
                         行 = 类型图标(+未读点) │ 标题(未读加粗) ×N 时间(右)  [×]
                                              └ 正文（可选，灰、单行省略）
```

- **铃铛图标即静音状态**：`silent_in_app` 开启时换成 `IconName::BellOff`（`bell-off.svg` 在 `AllAssets` 全量目录内，`app/src/assets.rs` 用的正是 `AllAssets`，不受 `default-icons.txt` 子集限制）。只跟 in-app 静音走——铃铛代表「通知会不会到眼前」，系统通道静音后通知中心仍有记录，图标不必变。tooltip 按四种组合分别说明是哪一条通道被静音。理由：用户看到「有未读数字却没弹过」时，若只能靠悬停才发现是自己开了静音，代价太高。

- **铃铛是 entity**（`NotificationBell`，字段只有 `filter`）。面板里的按钮回调只有 `&mut App`，要刷新触发器的徽标与面板内容，必须借宿主 entity 的 `cx.notify()`。
- **实体获取走 `Global + WeakEntity`**，不用 `window.use_keyed_state`：窗口销毁后 `upgrade()` 失败即重建，不留悬垂强引用。`bell(window, cx)` 是唯一入口。
- **读数不缓存**：面板内容每次重绘都从 `service::history()` / `unread_count()` 现读，不另存一份列表，避免与 toast 口径分叉。这一条有源码依据：`Popover` 把 content 交给 `BasePopover::content`，后者在 `render` 里是 `state.update(cx, |state, cx| (content)(state, window, cx))`——**每次浮层重绘都重新执行闭包**，所以 `NotificationBell` 一次 `notify()` 就能让面板与铃铛徽标同时换新。`Esc` 关闭浮层也由 `gpui-base` 内置（`KeyBinding::new("escape", Cancel, Some(CONTENT_CONTEXT))`），不需要自己绑定。
- **行用 `ListItem`**，且**只塞一个容器子元素**（外层 `h_flex` 再放图标与文本块）。`ListItem` 会把 children 收进同一个 `w_full` 的 div，并列多个 child 时的排布依赖默认方向，不可控；项目里其它 `ListItem` 用法也都是「一个容器」。
- **行尾「删除这条」走 `suffix`**，不塞进内容容器：`suffix` 在 `ListItem` 的 `justify_between` 末尾单独成块，点它不会和整行的「标记已读」抢事件（`editor/src/view/history.rs` 是同一套用法）。
- **行是两行布局**：标题与时间同一行（标题 `flex_1` 占满、超长省略，时间 `flex_none` 右对齐），正文单独一行。时间从第三行提到第一行后，一条从 3 行压到 2 行，50 条历史的可扫读条数翻倍。
- **未读用字重 + `Badge::dot()`**：标题 `FontWeight::MEDIUM`，类型图标右上叠标准 `Badge` 圆点，点取 `theme.accent` 而**不是**类型色（Info 的蓝图标配蓝点几乎看不见；「未读」应是稳定视觉语言，不该随类型漂移）。**不用** `ListItem::selected` 做整行底色——那是「选中」语义色（`accent`），做未读高亮太扎眼，而且 `list_item.rs` 里 hover 是 `if is_active { this } else { bg(list_hover) }`，active 行会直接压掉 hover 反馈。
- **头部统计跟着筛选走**：全部档显示「N 条 · M 未读」，未读档显示「N 条未读」，问题档显示「N 条问题」。切档后还写总数会和眼前的行数对不上；文案去掉「共」字，给 360 宽下的标题与按钮让出余量。问题档不能叫「待处理」——这一档包含已读的错误/警告，已读的显然已经看过。
- **一帧只取一次「现在」**：`center()` 里取一次 `Local::now()`，逐行传进去算相对时间。若每行各调一次 `Local::now()`，在秒边界上会出现同一屏里「刚刚」与「1 分钟前」并存。
- **面板宽 360 是内容区宽**：vendor 的 toast 面板总宽是 382（`notification.rs` 的 `DEFAULT_NOTIFICATION_WIDTH`），加上 Popover 自身内边距后与 360 接近，两种提示摆一起不会一宽一窄。
- **间距一律 4px 网格**：列表行间与行内文本块都用 `gap_1`（4px）。`rds-design` 禁止 6/10 这类非 4 倍数，2px 同样不该用。
- **历史满 50 条时列表底部给出说明**：`records.len() >= MAX_HISTORY` 时追加一行灰字「更早的通知已移除，仅保留最近 50 条」。不提示的话用户只会觉得「前天那条怎么没了」。仅「全部」档显示——筛选档条数少不代表历史被删。
- **空态不用 `empty::Empty`**：它是面板级空态（`flex_1 + p_6` + 虚线框），塞进 360px 浮层会撑出一片空白。这里只是「这一屏没有行」，用图标 + 一行灰字。该取舍在代码注释里写明了，避免后续被当作违规改回去。
- **图标来自 `gpui_kit::assets::IconName`**（完整 Lucide 目录），不是 `component::IconName` 的子集。
- **`Separator::horizontal()` 显式 `h(px(1.))`**：它的横线是 absolute 子元素、不贡献高度，在 flex 列里会被压成 0 高。

### 交互约定

| 动作 | 行为 |
|------|------|
| 点某一行 | 该条 `read = true`，刷新面板与铃铛（已读行再点不产生变更、不唤醒落盘） |
| 点行尾 `×` | 删除该条（含磁盘）；删完列表即时刷新 |
| 全部已读 | 全部置已读，按钮在 0 未读时禁用 |
| 清空 | 清空历史（含磁盘），列表为空时禁用；按钮带「不可恢复」tooltip |
| 切到「未读」 | 只显示未读；空态「没有未读通知」 |
| 切到「问题」 | 只显示 Error + Warning；空态「没有错误或警告」 |
| 打开面板 | **不**自动标记已读（否则用户看不到未读差异），**也不**清理 toast |

### 为什么不做按类型分组

早期设计按类型分组（错误 2 / 警告 1 …）。在浮层里，分组标题 + 分隔线会吃掉近一半高度，实际可扫读条数反而下降。最终用**扁平时间线 + 三档筛选**替代：时间线是通知最自然的排序，而「问题」档把用户真正要处理的两类（错误 + 警告）单独捞出来，覆盖了分组 80% 的用途，不再牺牲垂直空间。

## 7. 接线点

| 位置 | 内容 |
|------|------|
| `crates/app/src/main.rs` | `cx.set_app_identity("com.rdatastation.rds", "RdataStation")`，紧跟 `gpui_kit::init(cx)`，早于任何窗口；`on_app_quit` 里 `notification::flush_now()` |
| `crates/workbench/src/view/actions.rs` | `render_status_bar(&self, window, cx)`，先 `notification::bell(window, cx)` 再取 `cx.theme()` |
| `crates/workbench/src/view/render.rs` | `self.render_status_bar(window, cx)`；根节点挂 `ensure_notifications(window, cx)` 得到 toast 层 |
| `crates/settings/src/registry/entries/notifications.rs` | 11 个登记项，顺序 = 设置页顺序：`mute`（两个静音）→ `popup`（四个消失时长）→ `system`（四个类型开关）→ `merge`（合并窗口） |
| `crates/settings/src/service/mod.rs` | 11 个 `Slot::Notification*` 统一走 `notifications::apply_slot`（`skeleton.rs` 的节描述同步为「自动消失、同类合并与弹出 / 系统通知开关」） |

### 系统通知的前提（踩过的坑）

`gpui::App::set_app_identity` 必须满足三个条件，缺一个就**静默丢弃**：

1. 在**开窗之前**、**首次推送之前**调用；
2. Windows 上没有它，`in_app_and_system()` 的系统通知不会出现，也不报错；
3. macOS 还需要打成 `.app` bundle；Linux 还需要 notification daemon。

所以「系统通知开关打开了但收不到」在 Windows 上几乎都是第 2 条。全仓此前没有这处调用，已补。

### 旧配置迁移

拆 `do_not_disturb` 时，配置文件里可能还留着旧键。处理口径：

- `silent_in_app` 加 `#[serde(default, alias = "do_not_disturb")]`，旧值 `true` 直接落成「静音弹出」。这是当时必有的效果（开了勿扰就是不想被弹），不能丢；
- `silent_system` 无法从同一个键读第二次（serde 的键只会匹配到一个字段），只能回到默认 `false`。所以**旧勿扰用户升级后会重新收到系统通知**——方向是「变吵」，不会漏消息，属于安全方向；
- `success_autohide_secs` 的语义收窄为「成功类」；旧的「成功 + 信息」共用值保留给 Success，Info 取默认 3 秒。

两条迁移都有测试：`legacy_do_not_disturb_migrates_to_silent_in_app`（settings model）。

## 8. 已知边界（明确未做）

| 项 | 现状 | 影响 |
|----|------|------|
| 多窗口 | `active_window()` 优先，无聚焦窗口退回第一个 | 设置页/对话框在前台时 toast 也可见 |
| 只发系统通知 | 走 `Notification::system()`，toast 实体不创建 | 通知中心仍有记录、铃铛计数照涨；但系统通知点击回跳要 `window.window_handle()` 登记进 `SystemNotificationRegistry`，**无窗口时 toast 与系统通知都发不出**（vendor 架构约束，实现在 `push_system` 内） |
| 点击系统通知回跳 | vendor 已完整处理，本项目不设 `on_click` | `SystemNotificationRegistry::handle_response` 里依次做 `cx.activate(true)` → `window.activate_window()` → `list.close(id)` 关掉对应 toast → 最后若 `on_click` 存在才执行。所以点 Windows 通知就能把应用带回前台并收起那条 toast，不需要项目侧补回调；`on_click` 留给「点通知后要跳到具体页面」的场景（当前没有调用方需要） |
| 静音不撤回已弹出的 toast | 静音是「以后不弹」，不回溯 | 静音前弹出且未消失的 toast（错误默认常驻）会继续留在屏上，其 `×N` 可能落后于历史。要撤只能 `NotificationList::clear()`，那会连带 dismiss 全部系统通知，副作用比收益大，故不做 |
| 系统通知的停留时长 | 由操作系统决定 | 设置页的四个「自动消失」只作用于 in-app toast；四条 `to_system` 的 hint 已写明，避免用户以为「错误不消失」对系统通知也生效 |
| macOS 打包 | 未做 | 系统通知需要 bundle 才生效；开发态看不到 |
| 通知内联操作按钮 | 未做 | gpui-kit 的 `Notification::action()` 可用，但目前没有调用方需要 |
| 点 toast 打开通知中心 | 未做 | 需要把 `Popover` 的 open 状态从 `NotificationBell` 外提并双向同步；改坏「点铃铛打不开」的风险大于收益 |
| 历史检索 | 未做 | 50 条上限内靠滚动浏览，规模不足以需要搜索 |
| `mark_read` 等无 `cx` | 设计如此 | 这些是纯数据操作；UI 刷新由调用方 `cx.notify()` 负责，`view.rs` 已照做 |
| 行的键盘导航 | 未做 | 列表是 `div` + `ListItem`，没有接 `List` 的焦点模型；鼠标可点，键盘只到头部按钮 |
| 打开面板滚回顶部 | 副产物，非设计 | `ScrollHandle` 存在窗口的 element state 里，而帧交接只搬运**本帧访问过**的 state（`gpui-pre` `window.rs` 的 `finish()`：遍历 `accessed_element_states` 从上一帧搬过来）。Popover 关闭期间 content 一帧不渲染，state 没有被访问，随着上一帧一起销毁，重开就是新的 `ScrollHandle::default()`。效果上「打开总能看到最新」，但这是机制带来的，不是主动实现 |
| 面板筛选不重置 | 设计如此 | 关掉再开保留上次选中的档（TabBar 高亮着，不会让人以为通知丢了） |
| 未读档点一行会立刻消失 | 设计如此 | 点行即标记已读，该行不再属于「未读」，从当前筛选里移除。「全部」档点行只去掉圆点、行不动；「问题」档只看类型、不看已读，点行也不会消失——要看完整时间线就切「全部」 |
| 面板高度不随窗口自适应 | 未做 | 面板高约 430px（头部 + 分隔 + 三档筛选 + 列表 360 上限）。`gpui-base` 的 `Positioner` 只把 popup 位置 clamp 进视口，不缩尺寸，所以窗口高度小于面板时底部会被裁。窗口矮到这个程度时整个工作台本身已经不可用，做「按可用高度反算列表上限」收益极低且会引入新的布局耦合 |

## 9. 测试

`cargo test -p rds-notification` → 23 个用例，覆盖：

- **通道决策**：`Delivery::plan` 对 in-app / 系统两条开关的四种组合一一对应（`Both` / `SystemOnly` / `InAppOnly` / `HistoryOnly`），且只有「两条都关」才是只留历史

- 相对时间四档边界（含**整 60 秒必须落「1 分钟前」**、30 秒落「刚刚」）
- 时钟回拨不输出负值
- 非 RFC 3339 字符串原样透传
- 合并命中时只更新头部记录（不新增条目、`count` 递增）
- 合并时本次无正文 → 历史正文一并清空（不留上一条，避免与 toast 两套口径）
- A → B → A 合并后，被刷新的那条回到列表顶部（时间线顺序仍然代表时间）
- 超过 `MAX_HISTORY` 时截断
- 未读计数与单条/全部已读的账目一致
- 单条已读只在真变更时返回 true（避免白唤醒落盘线程）
- 删除单条只删指定那条、重复删除不算变更
- 筛选档与 `TabBar` 下标双向可还原、越界下标退回「全部」
- 三档筛选的匹配边界（「问题」= 错误 + 警告，不含成功/信息）
- 历史文件逐条容错：中间一条缺 `title` 时保留其余两条
- 缺字段记录按默认值补齐（`type_`→Info、`count`→1、`read`→false、`message`→None、`timestamp`→空）
- 非 JSON 数组输入返回空历史
- 批量操作只在真变更时报变更（空历史点「全部已读 / 清空」不唤醒落盘）
- **真写盘往返**：`write_history` → `load_history`，验证内容、顺序、正文与已读状态原样往返，且 rename 后不留 `.tmp`
- **并发写不产坏文件**：4 线程 × 40 轮各写 200 条到同一路径（`write_history_at` 入口），结束后文件必须是能整体解析的 JSON 数组、只含其中一个线程的记录、不留 `.tmp`。这条用独立子目录而非 `history_path()`，避免与「真写盘往返」那条并行时互踩；它守的是 `WRITE_LOCK` 本身——摘掉锁后测试立即红。

最后一条会真写文件，所以它开头先断言 `paths::is_isolated_root()`：`test-support` 一旦没生效，这个测试就会往**真实配置目录**写 `notification_history.json` 把用户历史盖掉——宁可测试红，也不许悄悄污染。已实测验证：真实目录（`%APPDATA%/RdataStation`、仓库 `.rds/config`）下不存在该文件，只出现在 `.rds/tmp/rds-test-root-<pid>/config/`。

`cargo test -p rds-settings` → 47 个用例。与本域直接相关的是登记表契约：模型叶子 ↔ 登记项双向一致、每个登记项都有 `Slot` 且形态（Bool/Number）一致、分组连续且同名分组显示名一致。新增三个开关/秒数只要漏改任一处，这三条会立刻红。另有两条迁移用例：`legacy_do_not_disturb_migrates_to_silent_in_app`、`unknown_keys_are_ignored`。

UI 部分（浮层位置、滚动、行点击、行尾 ×、铃铛在静音时的图标切换）没有自动化测试，依赖手动验证。

历史上的限提示（「更早的通知已移除，仅保留最近 50 条」）只在「全部」档出现：未读/问题档是筛选，条数少不代表历史被删，在那里提示会误导。

滚动行为分两种情况，都来自 element state 的帧机制，不是我们自己实现的：**面板打开期间**（点某条已读、切筛选）滚动位置保持——`ScrollableElement` 的 id 是 `ElementId::CodeLocation(Location::caller())` 且 `#[track_caller]`，每帧都能取回同一个 `ScrollHandle`；**关闭再打开**则回到顶部——关闭期间 content 不渲染，该 state 没被本帧访问，`finish()` 不会把它从上一帧搬过来，重开时 `use_keyed_state` 新建的是 default。
