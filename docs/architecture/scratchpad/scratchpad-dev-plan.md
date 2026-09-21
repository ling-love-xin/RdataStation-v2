# 草稿箱模块 · 开发方案（P0 + Phase A/B/C）

> 状态：**Phase A/B 全部落地 + K1/K1b（加载与重操作全面后台化，render 零 I/O）已修** · Phase C 进度：打开✅ / 连接预选✅（C-2 前半）/ 执行回写✅（C-2 后半）/ 脏点✅（C-3）/ **冲突 Diff✅（C-4）** / 拖放待接 · 待续：K1c（元数据级操作保持同步，**有意保留**）、Phase D（提升/存档/取回，依赖 `analytics_resource`）
> 关联文件：`scratchpad-prototype-design.md`（原型与已确认决策）、`scratchpad-prototype.html`（可交互原型）、`crates/scratchpad/README.md`（crate 入口与特点提炼）
> 前置：v1 后端/前端为行为蓝本（`v1/backend/src/core/scratchpad`、`v1/frontend/extensions/builtin/scratchpad`）；v2 后端已迁移（`crates/scratchpad`，`models`/`state`/`store` 47 个方法）
> 本方案核心变更：草稿箱根 = **模块目录 `{project}/scratchpad/`**（可见），内部元数据 `.RSmeta/scratchpad/`，回收站为**项目级** `.RSmeta/trash/`（草稿 + 资源共用）
> 复用 `connection-dev-plan.md` 的推进方式：Phase 划分 → 文件落点 → 验收 → 测试场景 → 风险

## 0. 进度记录（最近在前）

> 路径注：2026-09-16 起草稿箱后台任务已从 workbench 移入 crate（`crates/scratchpad/src/jobs.rs`），面板按面板拆模块（`crates/workbench/src/panels/*`），尺寸常量落到 `crates/workbench_shell/src/ui.rs`；**2026-09-17 起面板视图本身也下沉进 crate**（`crates/scratchpad/src/scratchpad_view.rs` + `host.rs`，见“十四次”）。**下列历史条目保留当时的路径**，读时按此换算。

### 2026-09-21（USIT 第 1 轮）— 「草稿 → 编辑器」这颗接口钉住 + 三处过期状态纠正

起因：真机 USIT 反馈“编辑器与草稿箱文件好像没接线”。核对下来**链路是通的**（双击 / `Enter` / 右键「打开」→ `ScratchpadHost::open_in_editor` → `Shared::request_open_in_editor` → 宿主 `render` 消费 → `editor::persist::open_file`；脏点 / 冲突 Diff / 拖入插入同样已接），真正欠的是两件事：**这颗接口没有用例钉住**（两端分属两个 crate，任一端换名都不会让另一端的单测变红），以及**三份文档还写着 `Phase C` 未接**——验收人照文档判定，只能得出“没接线”。

| # | 改动 | 位置 |
| --- | --- | --- |
| 1 | 新增接口用例：`open_in_editor(草稿绝对路径)` 必须入队一份**绝对路径 + 可写**的打开请求，且取出即清空 | `crates/workbench/src/components/scratchpad_host.rs::tests::opening_a_draft_hands_the_absolute_path_to_the_host`（新） |
| 2 | 状态纠正：头注“Phase C 尚未接线” → 已接（只剩系统文件拖入导入与命中跳行） | `README.md`（状态行 / §1 特点表 / §2 边界） |
| 3 | 状态纠正：§1“编辑文件内容（`Phase C` 接中央编辑器）”从「不能做」移到「能做 + 去编辑器改」；§9 清单指路 | `scratchpad-user-guide.md`（头注 + §1 表） |
| 4 | 状态纠正：§6.12「待接」里已完成的冲突 Diff / 拖入插入划掉；§6.14 的 `Enter` 行改成“在中央编辑器打开”（不再是“回落打开所在位置”） | `scratchpad-architecture.md` |

**未盖住的**：宿主 `WorkbenchView::render` 里那段消费分支（`take_open_in_editor` → `open_in_editor_with`）仍无自动化——与 §13.4 的 K10 同一条账（窗口级交互要真 `WorkbenchView`，见 `connection-dialog-architecture.md` §14 #9）。本次只在**接口**这一层钉住，“双击没反应”这类症状仍要人工看一眼（面板行 `on_click` 的 `click_count >= 2` 分支属窗口级）。

**验证**：`cargo test -p rds-workbench --lib -j 2` → **125 passed**（+1）；`cargo check -p rds-engine -p rds-editor -p rds-workbench --all-targets -j 2` 零告警。

### 2026-09-17（十九次）— Phase C-5 前半：拖草稿文件到编辑器插入

**已完成**

| # | 改动 | 位置 |
| --- | --- | --- |
| 1 | 跨 crate 拖放载荷 `InsertFileDrag { label, path }`（**只带路径**） | `crates/shared/src/drag.rs`（新）+ `lib.rs` 重导 |
| 2 | 草稿树行拖起（仅文件）+ 拖拽幽灵 `ScratchpadDragGhost` | `crates/scratchpad/src/scratchpad_view.rs` |
| 3 | 编辑器文本区接落点：读盘 → `InputState::replace`（光标处插入）→ 同步服务层 → 提示；只读 / 空文件 / 读失败均在消息里说清 | `crates/editor/src/view/host.rs::{on_drop, insert_file_contents}` |
| 4 | `editor` 显式声明对 `shared` 的依赖（与其 `lib.rs` 分层描述一致；`engine` 已传递携带，不增编译面） | `crates/editor/Cargo.toml`、`Cargo.lock` |

**关键取舍**：载荷住 `shared` 而不是拖起方 crate——拖放类型要被 `scratchpad` 与 `editor` 同时看见，
而两个特性 crate 不互相依赖（`database::NavDragPayload` 能住在拖起方，是因为它的消费方在 workbench）。

**验证**：`cargo check -p rds-shared -p rds-scratchpad -p rds-editor --all-targets -j 2` 与 `-p rds-workbench --lib -j 2` 零告警；`cargo test -p rds-editor --lib` → 217 passed；`cargo test -p rds-scratchpad --lib` → 36 passed。

**未接**：系统文件管理器拖文件进树 = 导入。仓库里目前无 OS 拖放入口（全仓无 `FileDropEvent` / `ExternalPaths`），
需先确认 gpui-kit 是否转发平台事件；目前导入入口是工具栏 `⬇`（系统文件对话框）。

### 2026-09-17（十八次）— 原型对齐巡检：脏点位置 + 点命中打开

**背景**：按用户要求逐条对照 `scratchpad-prototype-design.md` / `scratchpad-prototype.html`，
本轮先收两处差异（其余见回话中的巡检清单）。

| # | 差异 | 处理 |
| --- | --- | --- |
| 1 | 原型 HTML：名字 · **●** · 大小时间（脏点在名字之后）；实现当时画在名字**前** | 改为名字之后（`scratchpad_row`），文档同步 |
| 2 | 原型 §4.3「点击命中跳转文件」仍列待补 | 已接：点命中标题 → `request_open_in_editor`（相对路径 → 模块内绝对路径） |

**验证**：`cargo check -p rds-scratchpad -p rds-workbench --lib -j 2` 零告警；`cargo test -p rds-scratchpad --lib` → **36 passed**。

**尚未对齐（明确记录）**：命中**跳行**（需编辑器跳行端口）、提升为分析资源（Phase D）、
拖放（C-5，下一轮）、原型 §4.5 写的是「冲突对话框」，实现为**冲突条 + 中央 Diff 面板**
（差异内容一致，形式不同：对话框层需宿主 `open_dialog`，已记入待拍板）。

### 2026-09-17（十七次）— Phase C-4：冲突 Diff（外部改动 vs 编辑器未保存修改）

**已完成**

| # | 改动 | 位置 |
| --- | --- | --- |
| 1 | 后台任务新增 `Job::Diff` + `OpResult::Diff` + `enqueue_diff`（读盘在工线程） | `crates/scratchpad/src/jobs.rs` |
| 2 | 宿主端口新增 `draft_content`（读缓冲）/ `reload_draft`（照磁盘重载并清脏）/ `show_diff`（差异投中央编辑区） | `crates/scratchpad/src/host.rs`、`workbench/src/components/scratchpad_host.rs` |
| 3 | 检测：外部改动同拍取「脏文档 ∩ 模块内」→ 入队算差异；面板出冲突条（差异 / 重载 / 忽略 3 个动作） | `scratchpad_view.rs::{detect_scratchpad_conflicts, ScratchpadConflict}` |
| 4 | 中央编辑区的 Diff 面板（左=磁盘 / 右=编辑器，行号 + 红/绿） + `EditorBridge::show_diff` 通路 | `scratchpad_view.rs::render_scratchpad_diff_pane`、`workbench/src/panels/{shared,mod,editor}.rs` |

**关键取舍**

- **判据是内容而不是 mtime**：`diff_with_content` 全 `Unchanged` 就不算冲突——外部改动可能已被写回，或者就是我们自己写的；mtime 分不清这两件事。
- **消解动作在侧栅冲突条上**（不是中央面板）：它们要同时改编辑器与草稿箱自己的状态；面板只负责把差异看清楚。
- 「忽略」= 只移除本地条目：下次磁盘再变才重新报冲突（不靠“静默忽略列表”）。

**验证**：`cargo check -p rds-scratchpad -p rds-workbench --lib -j 2` 零告警；`cargo test -p rds-scratchpad -j 2 --lib` → **36 passed**（新增 `diff_markers_follow_unified_diff`）。

### 2026-09-17（十六次）— Phase C-2 后半：执行后回写 `last_connection_id`

**已完成**

| # | 改动 | 位置 |
| --- | --- | --- |
| 1 | 编辑器新增**执行回执**（`ExecReceipt{document, connection, succeeded}` + `drain_exec_receipts()`）；`drain_exec` 在交回结果的同时留一份，队列封顶 64 条 | `crates/editor/src/shared.rs`（纯加法，不改编辑器行为） |
| 2 | 宿主新增回写服务：1 s 一拍取回执 → 只算成功 → 文档→绝对路径→`relative_path_of` → `update_file_meta`；失败进状态栏 | `crates/workbench/src/services/scratchpad_meta.rs`（新）+ `services/mod.rs` |
| 3 | 装配：泵挂在 `WorkbenchView`（与草稿箱面板是否渲染无关），`init_workspace` 起一次 | `crates/workbench/src/view.rs`（`ensure_scratchpad_meta_pump`） |

**关键取舍**

- 泵**不挂在草稿箱面板**：侧栅切到别的工具时草稿箱不渲染，而执行照样发生；挂在宿主才不漏。
- 回执是**加法**：结果仍归编辑区，编辑器不认识草稿箱；「这路径是不是我的地盘」由消费方（`draft_targets`）判定。
- 只记**成功**的执行；同一草稿多条回执**后到者胜**（最后一次执行的那个连接才是“上次用的”）。
- 写的是元数据（`config.json`），按 K1c 保持同步（一次小 JSON 写），不开后台任务。

**验证**：`cargo check -p rds-editor -p rds-workbench --lib -j 2` 零告警；新增 2 项宿主编测（`only_files_inside_the_module_are_written_back` / `the_last_receipt_of_a_draft_wins`）。

### 2026-09-17（十五次）— Phase C-3：脏点回显（编辑器未保存修改 → 草稿树打点）

**已完成**

| # | 改动 | 位置 |
| --- | --- | --- |
| 1 | 宿主端口新增 `dirty_files()`（脏文档**绝对路径**集合；默认空集，不接编辑器的宿主不用实现） | `crates/scratchpad/src/host.rs` |
| 2 | 宿主实现：从 `EditorService::dirty_ids` + 文档路径取集合；`build_host` 多收一份 `EditorShared` | `workbench/src/components/scratchpad_host.rs` |
| 3 | 装配：`SidebarPanel::new` 多收 `&EditorShared`（前导 `::` 消歧——`panels/mod.rs` 自带 `mod editor;`） | `workbench/src/panels/mod.rs`、`workbench/src/view.rs` |
| 4 | 视图：1.2 s 轮询（与目录监控同拍）比对 `dirty_seen` 缓存，有变化才重绘；行渲染按绝对路径命中在文件名前画实心圆点（**只有文件**） | `crates/scratchpad/src/scratchpad_view.rs`（`refresh_dirty_cache` / `scratchpad_shows_dirty_dot`） |

**设计取舍**：脏文档集合是**外部状态**（编辑器拥有），所以走「轮询取回 + 缓存比对」而不是在 `render` 里问宿主——与「render 是纯读路径」一致；也不让 `scratchpad` 依赖 `editor` crate（本仓硬约束）。

**验证**：`cargo check -p rds-scratchpad --all-targets -j 2` 与 `cargo check -p rds-workbench --lib -j 2` 零告警；`cargo test -p rds-scratchpad -j 2 --lib` → **33 passed**（新增 `dirty_dot_marks_dirty_files_only`）。

**下一步**：C-2 后半（执行后回写 `file_meta`，需编辑器侧完成通知）、C-4 冲突 Diff（`diff_with_content` 已在手，缺 UI 与冲突触发点）、C-3 拖放。

### 2026-09-17（十四次）— 面板解耦跟随（A'4 视图下沉）+ Phase C-2 前半（打开预选连接）

**协作完成（panels-coupling A'4：视图下沉，宿主走端口）**

| 变化 | 新位置 |
| --- | --- |
| 面板视图（含纯函数与测试） | `crates/scratchpad/src/scratchpad_view.rs`（`ScratchpadView` / `ScratchpadSearchView`） |
| 面板键盘动作 | `crates/scratchpad/src/commands.rs` |
| 宿主端口（本 crate 定义） | `crates/scratchpad/src/host.rs`（`pub trait ScratchpadHost`） |
| 端口实现 + 装配 | `workbench/src/components/scratchpad_host.rs`（`WorkbenchScratchpadHost`）+ `panels/mod.rs`（`SidebarPanel` 持 `Entity<ScratchpadView>`） |
| 旧文件 | `workbench/src/panels/scratchpad_panel.rs` 已删 |

依赖方向随之更新：`scratchpad → workbench_shell / gpui-kit / shared`（仍**不依赖 workbench**）；本模块文档（入口 §3/§4/§5、架构 §6.12/§7.1/§7.2/§12）已按新落点对齐。

**本次已完成（Phase C-2 前半：草稿 → 连接预选）**

| # | 改动 | 位置 |
| --- | --- | --- |
| 1 | `FileMeta::preferred_connection()`：预选口径 = **显式绑定优先，其次最近执行**；都没有则不预选 | `crates/scratchpad/src/models.rs` |
| 2 | `ScratchpadStore::file_meta(rel)`：元数据读侧（无记录返回默认值，不报错） | `crates/scratchpad/src/store.rs` |
| 3 | `ScratchpadStore::relative_path_of(abs)`：绝对路径 → 模块内相对路径（反向；模块外与点前缀路径返回 `None`） | 同上 |
| 4 | 打开草稿即预选连接：只对**新建**文档生效（同路径只激活不覆盖用户选择），且连接必须在编辑器下拉里（已删除的不绑） | `crates/workbench/src/view.rs::open_in_editor` |

`DiffLineKind` 补派生 `Copy / PartialEq / Eq`（冲突 Diff 断言需要，纯加法）。

**验证**：`cargo check -p rds-scratchpad --all-targets -j 2` 与 `cargo check -p rds-workbench --lib -j 2` 零告警；`cargo test -p rds-scratchpad -j 2 --lib` → **32 passed**（新增 3 项：`preferred_connection_prefers_explicit_binding` / `file_meta_roundtrip_covers_binding_and_last_execution` / `relative_path_of_maps_editor_paths_back_into_the_module`）。

**下一步（C-2 后半，未做）**：执行完成后回写 `last_connection_id` / `last_executed_at`。需编辑器侧给一个执行完成通知——建议照本仓既有端口风格加 `EditorShared::attach_document_sink`，在 `editor/src/view/host.rs::drain_exec_results` 回调；workbench 侧收到后调 `update_file_meta`。缓做原因：该文件正被并行任务修改，避免冲突。

### 2026-09-16（十三次）— 结构解耦跟随：文档路径与新 API 对齐

**背景**（协作完成的结构调整）：

| 变化 | 新位置 |
| --- | --- |
| 面板按面板拆模块 | `workbench/src/panels/{scratchpad_panel,editor,shared,resources,right,mod}.rs` |
| 草稿箱后台任务移入 crate | `crates/scratchpad/src/jobs.rs`（`pub mod jobs`）——crate 现在自带异步编排，`cargo test -p rds-scratchpad` 直接覆盖任务层 |
| UI 尺寸常量落外壳 crate | `crates/workbench_shell/src/ui.rs`（经 `crate::ui` 重导） |
| «在编辑器中打开» 请求封装 | `Shared::request_open_in_editor(path)` / `Shared::take_open_in_editor()`（字段私有） |

**本次已完成**：模块入口 §3 代码地图、架构 §6.1/§6.6/§6.7/§6.8/§6.11–6.12 数据流标签、§7.1/§7.2/§10/§12/§13 的路径与 API 名全部按新结构对齐；原型页脚路径同步。

**新规矩已满足**：依赖 `paths`/`engine`/`shared` 的成员必须在 `[dev-dependencies]` 打开 `paths/test-support`（原因见 `docs/architecture/runtime/data-paths.md` §5）——`crates/scratchpad` 已具备该行，静态契约 `cargo test -p rds-paths` 已验证（1 passed）。

**验证**：`cargo test -p rds-paths -j 2` 通过；本轮仅文档改动，编译状态以当时工作区为准。

### 2026-09-16（十二次）— Phase C-1：双击 / Enter / 右键「打开」→ 中央编辑器

**已完成**

| 层 | 内容 | 落点 |
| --- | --- | --- |
| 共享态 | `Shared::open_file_request`（只传**绝对路径**）+ `take_open_file_request()`（取出即清空，与 `take_project_action_request` 同口径） | `crates/workbench/src/panels/` |
| 侧栏 | 行双击（`click_count >= 2`，仅文件）/ `Enter` / 右键菜单新增「打开」→ 置位请求；`request_open_scratchpad_file` 统一入口；不再回落到「打开所在位置」（`↗` 仍保留该能力） | 同上 |
| 宿主 | `WorkbenchView::render` 消费请求 → `open_in_editor(path)`（复用已有 `editor::persist::open_file` + `show_document`：同路径只激活不重读）；失败写通知栏 | `crates/workbench/src/view.rs` |

**边界**：草稿箱只发「要打开这个路径」的意图；模式与只读等级由编辑器按路径判定（`editor::mode::resolve_mode`）。**仍余**：SQL 草稿模式的连接回填（`file_meta.last_connection_id`）、脏点回显、冲突 Diff、拖放。

**验证**：`cargo check -p rds-workbench -p rds-app --all-targets -j 2` 零告警。**未验证**：GUI 实机（双击打开/同路径激活/打开失败提示）。

### 2026-09-16（十一次）— Phase A5：草稿箱文件监控（外部改动自动刷新）

**已完成**

| 层 | 内容 | 落点 |
| --- | --- | --- |
| crate | `watch` 模块：`ScratchpadWatcher`（`notify` 递归监听模块根，OS 事件只置位 `ChangeFlag`）+ `ChangeFlag`（`mark` / `take`）；drop 即停止监听（事件收敛线程自然退出）；新增 2 项单测 | `crates/scratchpad/src/watch.rs`（新）、`lib.rs`、`Cargo.toml`、根 `Cargo.toml`（workspace 加 `notify`） |
| 面板 | `ensure_scratchpad_watch`（项目根变化时换监控点；失败降级为手动 `↻`）+ `ensure_scratchpad_watch_poll`（常驻 1.2 s 去抖轮询：有变更且未在内联编辑/无加载在途 → 置 `loaded = false` 触发常规重载）；项目关闭时停掉监控；发起重载时先清一次标记，避免自己写文件后多刷一次 | `crates/workbench/src/panels/` |

**设计取舍**：只做「变更标记 + 去抖重拉」，不做增量同步（避免复刻 `scan_dir_tree` 的排序/过滤/懒加载/行号语义）；只监听内容目录不监听 `.RSmeta`，因此配置写入不会自激刷新。

**验证**：`cargo check -p rds-scratchpad -p rds-workbench -p rds-app --all-targets -j 2` 零告警；`cargo test -p rds-scratchpad -j 2 --lib` **16 passed**（新增 `watch::tests::{change_flag_marks_and_clears, external_write_is_observed}`）。**未验证**：GUI 实机（在外部编辑器改文件后面板 ~1.2 s 内自动刷新）。

**收尾（同日）**：监控拍发现外部改动时，若内容搜索结果面板还开着，用同一套查询/开关**重跑一次搜索**（K4 余项）——否则替换与外部改动都会让结果成为快照。

### 2026-09-16（十次）— K1b：重操作全部后台化（导入 / 粘贴 / 清空回收站 / 搜索 / 替换）

**背景**：K1 只把「加载」搬离了 render；真正会长时间卡 UI 的是搬运字节与遍历全树的操作（导入 GB 级文件、复制大目录、清空大回收站、全树搜索、批量替换）。

**已完成**

| 层 | 内容 | 落点 |
| --- | --- | --- |
| 任务层 | `scratchpad_jobs` 新增 `Import` / `Paste` / `EmptyTrash` / `Search` / `ReplaceAll` 任务与 `OpResult`（含 `SearchPayload`、替换汇总）；新增 `enqueue_import` / `enqueue_paste` / `enqueue_empty_trash` / `enqueue_search` / `enqueue_replace_all` / `drain_ops` | `crates/workbench/src/services/scratchpad_jobs.rs` |
| 侧栏 | 四个重操作改为「只入队 + 起轮询」；新增 `apply_scratchpad_ops` 统一处理文案 / 刷新 / 通知（含剪切成功后收起剪贴板、清空后收起回收站分组） | `crates/workbench/src/panels/` |
| 编辑区 | 「全部替换」只入队（任务内部：先搜→去重文件→逐文件写回→重搜）；新增 `Shared::scratchpad_pump_request`，由 `SidebarPanel::render` 在**所有左侧面板模式下**消费（仅“完全隐藏”时留到恢复那一帧） | 同上 |
| 清理 | 删除已无调用方的 `copy_scratchpad_entry`（复制已走 `store.copy_entry`） | 同上 |

**有意保留同步**（判定标准：是否可能搬运字节或遍历全树）：新建 / 重命名 / 删除入回收站 / 回收站还原 / 引用增删改 / 打开所在位置——均为单次系统调用（微秒~毫秒级），迁后台反而增加状态同步成本。已记入架构 §13.1 K1c。

**验证**：`cargo check -p rds-workbench -p rds-app --all-targets -j 2` 零告警（仅 `rds-mock` 有一条并行任务的 warning）。**未验证**：GUI 实机（大文件导入/大目录复制/替换进行时的界面响应、通知文案）。

### 2026-09-16（九次）— K1：草稿箱加载全面后台化（render 零 I/O）

**背景**：`render_scratchpad` 首次进入、操作后重载、展开文件夹都同步 `block_on` 读盘（大目录/网络盘会冻结界面），违反自定约束「render 是纯读路径」。

**已完成**

| 层 | 内容 | 落点 |
| --- | --- | --- |
| 新增 | `scratchpad_jobs`：单工作线程 + tokio 运行时，任务（`LoadRoot` / `LoadDir`）→ 结果队列；`enqueue_*` / `drain_*` / `has_pending` / `invalidate_loads`；模块根加载带自增 `seq` 防过期（沿用 `nav_jobs` 模式） | `crates/workbench/src/services/scratchpad_jobs.rs`（新）+ `services/mod.rs` |
| 面板 | `load_scratchpad`（同步）→ `request_scratchpad_load`（只入队 + 起轮询）；新增 `ensure_scratchpad_pump`（60 ms 轮询）/ `apply_scratchpad_loads` / `apply_scratchpad_dirs`；`load_scratchpad_dir` → `request_scratchpad_dir` | `crates/workbench/src/panels/` |
| 视图 | `ScratchpadView` 新增 `loading` / `load_seq`；在途期间状态行显示「加载中…」，且**不再用空态占位闪现**（保留旧条目到新结果到达） | 同上 |
| 防护 | 项目已关闭/切换时 `invalidate_loads()` 推进序号，在途的旧项目结果一律丢弃；轮询印空闲自退，面板销毁后 `weak.update` 失败即结束 | 同上 |

**验证**：`cargo check -p rds-workbench -p rds-app --all-targets -j 2` 零告警。**未验证**：GUI 实际手感（首帧不再卡顿、加载中文案）需人工过一遍《使用手册》§9 验收清单的「基础与空态」。

**仍余（K1b）**：事件路径的写操作（删除/粘贴/导入/提交编辑/搜索/替换/引用增删改）仍是同步 `block_on`；方向是按同一模式逐类迁为任务（每类需自己的结果类型与回填方法）。

### 2026-09-15（八次）— Phase B 收尾：递归复制 / 命中高亮 / 模板 / 虚拟列表 / 键盘导航 / 替换

**已完成**

| 层 | 内容 | 落点 |
| --- | --- | --- |
| 后端 | `copy_entry`（文件夹递归、`_copy`/`_copy_N` 避让、二进制安全 `fs::copy`、拒绝复制到自身子树）+ `copy_dir_contents` | `crates/scratchpad/src/store.rs` |
| 后端 | `SearchMatch::match_spans`（行内命中字节区间，单行最多 16 段）+ `literal_match_spans`（大小写不敏感时校验字节长度，Unicode 变宽则放弃高亮） | 同上 / `models.rs` |
| 后端 | `replace_in_file` 新增 `case_sensitive`；字面量模式转义后仍走 regex（共享大小写开关），并用 `NoExpand` 保证替换串里的 `$` 不被当作分组引用 | `store.rs` |
| 后端 | `update_external_reference_path`（失效引用重新定位：只改路径，别名/创建时间不变） | `store.rs` |
| 侧栏 | 文件夹递归复制接入粘贴（`copy_entry`）；模板 chip 行（空白/SQL/Python/Markdown/JSON：自动补后缀 + 占位内容）；空态改为大图标 + 标题 + 说明 + 新建/文件夹/导入按钮；草稿树改为**唯一滚动区**（`v_virtual_list` + `track_scroll`，行高逐行给出）；引用/回收站底部限高可滚 | `crates/workbench/src/panels/` |
| 侧栏 | 键盘：`↑↓` 移动选中并滚入视口、`Enter` 文件夹展开折叠（文件暂回落「打开所在位置」）、`Ctrl+N` 新建文件 | `panels/` / `commands.rs` / `crates/app/src/main.rs` |
| 侧栏 | 失效引用行新增 `⟲` 重新引用（文件/目录选择器 → `update_external_reference_path`） | `panels/` |
| 中央区 | 命中文本高亮（消费 `search.match.background` 产品 token，按 `match_spans` 切段）；替换栏（「替换为」输入 + 「全部替换」+ 预览计数），逐文件写回后自动刷新结果 | `panels/`（`EditorPanel`） |
| 共享态 | `Shared::scratchpad_store()`（侧栏与编辑区共用）、`run_scratchpad_search(...)`（搜索构建结果视图单点） | `panels/` |
| 清理 | 删掉占位文件 `crates/scratchpad/src/{model,commands,scratchpad_view}.rs` | `crates/scratchpad/src/` |
| 原型 | `scratchpad-prototype.html` 补：模板 chip 行、替换栏、失效引用 `⟲`、空态文案/三按钮、token 文案；`scratchpad-prototype-design.md` §2.1/§2.3/§3/§4.3/§4.4/§4.5/§4.7/§6.4/§7 按实现重写 | `docs/architecture/scratchpad/*` |
| 文档 | 补齐模块五件套缺口：`README.md`（模块入口：特点/边界/代码地图/硬约束 10 条/测试命令）· `scratchpad-architecture.md`（设计理念与架构：不变式/概念模型/存储布局/回收站/路径安全/十条数据流/D1–D13/降级矩阵/测试策略/实现映射/**§13 已知问题 K1–K12**）· `scratchpad-user-guide.md`（使用手册：导览/典型流程/操作落到哪/快捷键/FAQ/验收清单），并在 `docs/architecture/README.md` 登记 | `docs/architecture/scratchpad/*` |
| 侧栏 | 新建落点对齐原型：选中文件夹时内联行插在该文件夹首行（并自动展开），未选中则建在模块根；重载时保留并**刷新**已展开子目录缓存（修掉“操作后展开态看起来空了”） | `crates/workbench/src/panels/` |

**验证**：`cargo check -p rds-scratchpad -p rds-workbench -p rds-app --all-targets -j 2` 零告警；`cargo test -p rds-scratchpad -j 2` **14 passed**（含新增 `copy_entry_recurses_and_avoids_name_collisions`、`search_reports_match_spans`、`replace_in_file_handles_case_regex_and_literal_dollar`、`external_reference_relink_updates_path_only`）。`cargo test` 的 doctest 阶段在 Windows 报 `os error 448`（rustdoc 无法执行，环境限制，与本模块无关）。

**仍余（Phase C/D，依赖其它模块）**：双击打开 → 中央编辑器草稿模式（SQL 草稿 + `Ctrl+S` 回存 + `file_meta.last_connection_id` 恢复）、脏点、冲突 Diff（`diff_with_content` 尚未消费）、拖放导入/插入、多文件 Tab；Phase D 提升/存档只读/取回/版本（依赖 `analytics_resource`）。

### 2026-09-11（七次）— 引用能力补齐（别名 / 文件或目录 / 改名 / 打开）+ 原型对齐

> 澄清：文中的「引用」指草稿箱的**外部引用功能**（链接外部路径），不是代码引用。

**导入 vs 引用**：导入 = 复制进 `{项目}/scratchpad/`（计体积、随项目迁移）；引用 = 只记路径 + 别名（不计体积、可能失效，需探测）。

**已完成**

| 层 | 内容 | 落点 |
| --- | --- | --- |
| 后端 | 新增 `rename_external_reference(old, new)`（别名校验：非空 / 无分隔符 / 无 `..` / 不重名）；新增单测 `external_reference_rename_and_validation` | `crates/scratchpad/src/store.rs` |
| 侧栏 | 引用选择器改为**文件或目录**；选定后内联输入**别名**（默认取名称）再提交；引用行新增 `↗` 打开 / `✎` 改别名 / `✕` 移除；移除旧的自动别名路径 | `crates/workbench/src/panels/` |
| 原型对齐 | 原型设计 §2 改为两行工具栏 + 模式 chip 搜索 + 行尾「大小 · 相对时间」+ 行操作、新增 §2.2「导入 vs 引用」；§3/§4.3/§4.4/§4.7 按实现重写；`scratchpad-prototype.html` 同步（两行工具栏、模式 chip、引用行操作、空态/底部文案） | `docs/architecture/scratchpad/*` |

**未验证**：本轮改动（`store.rs` 新方法 + 单测、`panels/` 引用 UI）**尚未编译验证**——执行 `cargo check` 时环境报 `cargo: Permission denied`（`cargo` 为 `rustup.exe` 符号链接，疑似并行进程/杀软锁定），待环境恢复后用 `cargo check -p rds-scratchpad -p rds-workbench --all-targets -j 2` 补验。

### 2026-09-11（六次）— 内容搜索（正则/大小写）+ 结果落中央编辑区

**已完成**

| 层 | 内容 | 落点 |
| --- | --- | --- |
| 后端 | `search_file_content(query, case_sensitive, context_lines, is_regex)` 新增 `is_regex`；正则循环外编译一次（`RegexBuilder::case_insensitive`）；单文件匹配改为 `regex.is_match` 分支；新增单测 `search_content_regex_and_case` | `crates/scratchpad/src/store.rs` |
| 侧栏 | 搜索行新增模式 chip（`文件名` / `内容`）；内容模式下 `.*` 正则、`Aa` 区分大小写、`⏎` 运行；输入框 `Enter` 在内容模式下直接搜索 | `crates/workbench/src/panels/` |
| 共享态 | `Shared::scratchpad_search`（`Rc<RefCell<Option<ScratchpadSearchView>>>`）；侧栏写入 + `notify_host`，编辑区读取 | 同上 |
| 中央编辑区 | 内容结果面板（头部：查询/命中数/扫描数/开关标记 + 「关闭」；命中项：文件 · 行号 + 上下文行） | `panels/scratchpad_panel.rs::render_scratchpad_search_pane` + `EditorPanel::render` |

**验证**：`cargo check --workspace --all-targets -j 2` 零告警；`cargo check -p rds-workbench -p rds-app --all-targets -j 2` 零告警。

**未完成**：命中文本高亮（主题侧已有 `search.match.background` 产品 token，草稿箱未消费）；点击命中跳转到文件（依赖编辑器打开，Phase C）。后端新单测已加入，但 `cargo test -p rds-scratchpad -j 2` 两次卡在 `libduckdb-sys` 原生库构建（项目已警告并发链接耗内存），**本轮未执行测试**。

### 2026-09-11（五次）— 右键菜单 + 键盘快捷键

**已完成**（`crates/workbench/src/panels/`、`crates/workbench/src/commands.rs`、`crates/app/src/main.rs`）

| 能力 | 实现 |
| --- | --- |
| 右键菜单 | 行 `.context_menu(...)`（gpui-kit `ContextMenuExt` + `PopupMenuItem`）：打开位置 / 重命名 / 剪切 / 复制 / 删除（带分隔线） |
| F2 重命名 | `ScratchpadRename` action → 唯一选中项行内重命名 |
| Delete 删除 | `ScratchpadDelete` action → 批量删除（含撤销栏） |
| Esc 取消编辑 | `ScratchpadCancelEdit` action → 关闭内联编辑 |
| 绑定与 context | `actions!(scratchpad, …)` + app 层 `scratchpad` context 绑定（Ctrl+A / F2 / Delete / Escape） |

**验证**：`cargo check --workspace --all-targets -j 2` 与 `cargo check -p rds-app -j 2` 均零告警。

**仍余**：新建模板；文件夹递归复制；内容搜索（正则/大小写）；虚拟列表（>50）；空态大图标+双按钮；脏点/双击打开（Phase C）；提升/存档/取回（Phase D）。

### 2026-09-11（四次）— 导入/引用/打开位置/全选/撤销自动消失/行尾元信息

**已完成**（`crates/workbench/src/panels/`、`commands.rs`、`crates/app/src/main.rs`）

| 能力 | 实现 |
| --- | --- |
| 导入文件 | 工具栏 `⬇` → 系统文件对话框（多选）→ `import_external_file` |
| 外部引用添加 | 工具栏 `🔗` → 选目录 → `add_external_reference`（别名默认取目录名） |
| 打开所在位置 | 选中行 `↗` → `open_in_system_explorer` |
| Ctrl+A 全选 | `actions!(scratchpad,[ScratchpadSelectAll])` + app 层绑定（`scratchpad` context）+ 面板 `key_context`/`track_focus` + 行点击聚焦 |
| 撤销栏 5s 自动消失 | `background_executor` 定时器；仅当撤销栏仍指向同一次删除才清空 |
| 行尾元信息 | 相对时间（刚刚/N分钟/N小时/N天/日期）+ 文件可读大小 |
| 选中左侧珊瑚色条 | 行 `relative()` + 绝对定位 2px `list.active.border`（原型 §3） |
| 工具栏两行 | 第一行新建/导入/引用/排序/刷新；第二行剪切/复制/粘贴/删除（选择非空或剪贴板非空时显示） |

**验证**：`cargo check --workspace --all-targets -j 2` 零告警。本轮未改 `rds-scratchpad`，后端测试不受影响（上轮 7 项全绿）；`cargo test` 因并行构建占用构建目录锁未重跑。

**仍余**：右键菜单；键盘（F2/Del/Esc/↑↓）；新建模板；文件夹递归复制；内容搜索（正则/大小写，结果落中央区）；虚拟列表（>50）；脏点/双击打开（Phase C）；提升/存档/取回（Phase D）。

### 2026-09-11（三次）— 草稿箱面板自身闭环

**已完成**（`crates/workbench/src/panels/`）

| 能力 | 实现 |
| --- | --- |
| 新建（内联） | 工具栏 ＋ / 🗀 → 顶部内联输入（`Input` + ✓/✕，Enter 提交）；`create_entry` |
| 重命名（内联） | 选中行 ✎ → 行内输入；`rename_entry`；初始值回填文件名 |
| 删除 → 回收站 | 选中行 ✕ → `delete_entry`（项目级回收站）+ 底部**撤销栏**（`restore_from_trash`） |
| 回收站分组 | 头部展开/折叠、逐条「还原」、头部「清空」；条目带来源模块标签 |
| 文件名过滤 | 搜索框（`InputState`，placeholder）+ 递归匹配（自身或子树命中，命中时自动展开） |
| 外部引用 | 列表展示（丢失项置灰 + “（丢失）”）+ 逐条移除 |
| 输入框管理 | 懒创建 + `subscribe_in`：`PressEnter` 提交、`Change` 重绘（`InputEvent`） |

**验证**：`cargo check --workspace --all-targets` 零告警（仅 `rds-project` 有一条非本任务 warning）；`cargo test -p rds-scratchpad` 7 passed。

**未完成**：右键菜单（当前用选中行内联操作替代）、多选/批量、剪切/复制/移动、导入（系统文件对话框）、外部引用添加、虚拟列表（>50）、排序、懒加载（当前 depth=4 全量）；撤销栏目前常驻到下一次操作（5s 自动消失待接定时器）；脏点依赖编辑器宿主（Phase C）。

### 2026-09-11（二次）— 模块根 + 项目级回收站 + 元数据/引用

> 取代上一条中的 “根 = 项目目录” 与 “回收站落 meta” 两项（已按最终模型重写）。

**已完成**

| 项 | 内容 | 落点 | 验证 |
| --- | --- | --- | --- |
| 根语义定稿 | `ScratchpadStore::new` 改为 `root = {project}/scratchpad`（可见模块目录），`meta = {project}/.RSmeta/scratchpad`；`ensure_dir` 建模块根/meta/回收站 | `crates/scratchpad/src/store.rs` | 单测 `module_root_and_meta_isolated` |
| 项目级回收站 | 新增 `ProjectTrash`：`.RSmeta/trash/<id>/{payload,manifest.json}`，条目携带 `origin` + `original_rel_path` + `kind/size/deleted_at`；restore 按原相对路径重建、同名自动改名；跨模块条目拒绝在草稿箱还原。**P0.8（2026-09-18）已上提到 `crates/engine/src/persistence/trash.rs`**（中性化：`TrashKind` / `TrashRestoreOutcome` / `empty_origin`），草稿箱侧的列表与清空按 `origin` 过滤 | `crates/engine/src/persistence/trash.rs`（原 `crates/scratchpad/src/trash.rs`） | 单测 `trash_is_project_level_with_origin`、`restore_refuses_other_module_entries`、`empty_origin_leaves_other_modules_alone` |
| 旧数据迁移 | `.scratchpad/` 内容 → `scratchpad/`；旧 `.scratchpad.json` → `config.json`；旧 `.trash/` 与上一版 `{meta}/.trash` 均并入项目级回收站；空壳清理；幂等 | `store.rs`（`migrate_legacy_layout`） | 单测 `legacy_layout_is_migrated` |
| 数据源引用 | `FileMeta` 新增 `bound_connections`；新增 `ScratchpadStore::bind_connections`（只存连接 ID） | `models.rs` / `store.rs` | 单测 `bound_connections_roundtrip` |
| 外部引用可用性 | 新增 `ExternalReferenceStatus` + `external_reference_status()`（加载时探测路径是否存在） | `models.rs` / `store.rs` | 单测 `external_reference_persists_and_reports_status` |
| 面板 | 外部引用显示“丢失”态（置灰 + （丢失）后缀） | `crates/workbench/src/panels/` | `cargo check` 零告警 |

**下一步**：A5 文件监控；B4–B9（内联新建/模板、导入/引用对话框、搜索、右键菜单/重命名/删除/移动、回收站管理 + 撤销栏、多选、虚拟列表/排序、懒加载）；新增 **Phase D 提升/存档/取回**（§1.2）。

### 2026-09-11 — P0 + Phase A + Phase B 首切片

**已完成**

| 项 | 内容 | 落点 | 验证 |
| --- | --- | --- | --- |
| P0.1/P0.2 | 当前项目会话：环境变量 `RDS_PROJECT_PATH` → 全局库「最近打开项目」（取路径仍存在者）→ 空态；会话写入 `Shared::project` | `crates/workbench/src/services/project_session.rs`、`panels/`（`Shared`）、`view.rs`（`WorkbenchView::new` + 标题栏项目名） | `cargo check` 零告警 |
| A1/A2 | `ScratchpadStore` 根 = 项目目录；内部元数据 `{project}/.RSmeta/scratchpad/`（`config.json` + `.trash/`）；`ensure_dir` 只建 meta 目录 | `crates/scratchpad/src/store.rs` | 单测 `root_is_project_dir_and_meta_isolated` |
| A3 | 隐藏与防护：树跳过点前缀条目（`.RSmeta` 天然不可见）；`resolve_path_impl` 拒绝首段为点的路径 | 同上 | 单测 `internal_paths_are_blocked` |
| A4 | 旧布局迁移：`.scratchpad/.scratchpad.json` → 新配置；用户文件搬到项目根（**同名保留不覆盖**）；旧 `.trash` 并入新回收站；空目录清理；幂等 | 同上（`migrate_legacy_layout`） | 单测 `legacy_layout_is_migrated` |
| A6 | 清理语义：`CONFIG_FILE` 改为 `config.json`；回收站统一走 `trash_dir()` | 同上 | 编译零告警 |
| A7 | 测试：根语义/元数据隔离/路径防护/回收站落位/引用持久化/旧布局迁移 5 项 | `store.rs` `mod tests` | `cargo test -p rds-scratchpad` 5 passed |
| B1 | 依赖接线：workspace `scratchpad` 别名 + workbench 依赖 | `Cargo.toml`、`crates/workbench/Cargo.toml` | `cargo check` |
| B2/B3（部分） | 草稿箱面板首个切片：工具栏（＋文件 / 🗀文件夹 / ↻刷新）、只读树（展开/折叠/选中）、外部引用分组、回收站计数、空态、底部统计；`LeftPanel::Draft` 由占位接管 | `crates/workbench/src/panels/`（`ScratchpadView` / `render_scratchpad`） | `cargo check` 零告警 |
| A5 | 文件监控：**未做**（`notify` 已是构建图内传递依赖，接入时仍按 workspace.dependencies 统一声明） | `crates/scratchpad/src/state.rs` | — |

**Phase B 未完成（下一步）**：B4 内联新建输入与模板、导入/引用对话框；B5 搜索（文件名过滤 + 内容搜索结果落中央区）；B6 右键菜单/重命名/删除/移动/提升 + 键盘；B7 回收站管理 + 撤销栏；B8 多选/批量；B9 虚拟列表（>50）与排序；懒加载（当前 depth=4 全量，大项目需改按需加载）。

**Phase C 未开始**：编辑器草稿文件模式、`file_meta` 连接恢复、拖放、冲突 Diff、搜索替换。

**已知取舍**：
- 面板加载用 `Runtime::new().block_on(...)`（与 workbench 现有服务调用模式一致）；后续可改 `cx.spawn`。
- 新建文件默认 `未命名.sql`（冲突自动 `_1` 递补），未做模板选择。
- 树为 depth=4 全量加载，未做懒加载；项目根很大时有 IO 开销。

**关键文件**：`crates/scratchpad/src/store.rs`、`crates/scratchpad/src/state.rs`、`crates/workbench/src/panels/`、`crates/workbench/src/services/project_session.rs`、`crates/workbench/src/view.rs`、`Cargo.toml`、`crates/workbench/Cargo.toml`。

## 1. 现状结论（盘点摘要）

| 层 | 状态 |
| --- | --- |
| 域模型（`crates/scratchpad/src/models.rs`：`ScratchpadEntry`/`SearchMatch`/`ExternalReference`/`AnalyzableFile`/`FileMeta`/`DiffResult`/`ReplaceResult`） | ✅ 已迁移 |
| 存储（`crates/scratchpad/src/store.rs`：列表/CRUD/回收站/搜索/替换/Diff/引用/可分析文件/路径防护） | ✅ 已迁移（根目录仍为 `{project}/.scratchpad/`，需改造） |
| 状态（`crates/scratchpad/src/state.rs`：`ScratchpadState` + `watcher_active` 标志） | ⚠️ 有骨架，**未接入任何调用方**；文件监控未真正启动 |
| 占位文件（`model.rs` / `commands.rs` / `scratchpad_view.rs`） | ✅ 已删除（2026-09-15；视图落在 `workbench/src/panels/`） |
| 视图（GPUI） | ✅ 已实现（`SidebarPanel::render_scratchpad` + `EditorPanel` 搜索结果/替换栏） |
| workbench 依赖 | ✅ 已依赖 `rds-scratchpad`（workspace 依赖） |
| 当前项目会话（项目根路径来源） | ✅ 已接（P0：`Shared::project`（`OpenProject`），连接对话框与草稿箱共用） |

**关键缺口**：① 根目录语义切换（A）✅；② 面板视图与交互（B）✅；③ 编辑器联动与生态（C）；④ 提升/存档/取回（D）。前置依赖 P0 项目会话 ✅。

## 2. 阶段划分

### P0 — 当前项目会话（前置，与连接模块共用）

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| P0.1 | 启动确定「当前项目根」：优先启动参数，其次 `GlobalDatabaseManager::get_recent_projects` 首项，最后默认工作区目录 ✅ | `crates/app/src/main.rs` + `crates/workbench/src/services/`（新增 `project_session.rs`） | 启动后可拿到 `project_root: Option<PathBuf>` |
| P0.2 | 会话状态入 `Shared`（`Rc<RefCell<Option<ProjectSession>>>`），标题栏项目名 / 连接对话框 / 草稿箱共用 ✅ | `crates/workbench/src/panels/`（`Shared`）、`view.rs` | 三处读到同一项目根 |
| P0.3 | 无项目时降级：草稿箱空态、连接项目作用域禁用（消除「手填项目路径」）⬜ | workbench | UI 有明确空态 |

> P0 是草稿箱能显示内容的硬前提；若暂缓，草稿箱只能停留在空态。

### Phase A — 后端根语义切换（目标：根 = 项目目录，元数据隔离）✅ 已实现（A5 文件监控除外）

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| A1 | `ScratchpadStore::new` 改为：`root = project_path`，`meta_dir = project_path/.RSmeta/scratchpad`，`config = meta_dir/config.json`，`trash = meta_dir/trash/` | `crates/scratchpad/src/store.rs` | 列出的是项目文件，不再有 `.scratchpad/` |
| A2 | `ensure_dir` 只创建 meta 目录（config 父目录 + trash），**不创建**项目根 | 同上 | 空项目首次调用后出现 `.RSmeta/scratchpad/` |
| A3 | 隐藏与防护：`scan_dir_tree` 跳过所有点开头条目（已有）+ 显式跳过 `.RSmeta`；`resolve_path_impl` 拒绝首段为 `.RSmeta` 或点开头的相对路径（防越权读写内部目录） | 同上 | `.RSmeta` 不在列表、不可被 API 访问 |
| A4 | 旧数据迁移：若 `{project}/.scratchpad/` 存在 → 迁移 `config.json`（原 `.scratchpad.json`）与用户文件到新语义（文件本就在根下则不移动），迁移后清理空目录（策略见原型 §8.2 待确认） | 同上（`migrate_legacy_layout`） | 迁移幂等；重复启动不报错 |
| A5 | 文件监控接入：用 `notify` 监听项目根（忽略 `.RSmeta`），变更经事件推送刷新树；`ScratchpadState::set_watching` 落地 ✅ 2026-09-16（改为监听模块目录 `scratchpad/`，变更标记 + 1.2 s 去抖重拉；`ScratchpadState::set_watching` 仍未接，监控器自持生命周期） | `crates/scratchpad/src/watch.rs`（+ 依赖 `notify`） | 外部新建/修改文件，面板自动刷新 |
| A6 | 清理占位死文件（`model.rs` / `commands.rs` / `scratchpad_view.rs`）✅ 2026-09-15 已删 | `crates/scratchpad/src/` | `cargo check -p rds-scratchpad` 零告警 |
| A7 | 单元/集成测试：根列表隐藏内部目录、路径穿越防护、回收站落位、引用/`file_meta` 读写、`get_analyzable_files` 相对路径 | `crates/scratchpad/tests/` | 测试全绿 |

### Phase B — 面板视图接入（目标：替换 `LeftPanel::Draft` 占位）✅ 已全部落地

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| B1 | 依赖接线：`Cargo.toml` workspace 增 `scratchpad` 别名；workbench 依赖 `scratchpad` ✅ | `Cargo.toml`、`crates/workbench/Cargo.toml` | 编译通过，依赖方向向下 |
| B2 | 面板实体：面板头 / 工具栏 / 搜索 / 分组树 / 底部状态；状态存于 `ScratchpadView` ✅ | `crates/workbench/src/panels/`（`SidebarPanel`） | 面板渲染，切换活动栏可见 |
| B3 | 树渲染：递归行、类型色点、选中/悬停、相对时间、懒加载（`depth=0` → 展开加载）✅ | 同上 | 深目录展开正确 |
| B4 | 工具栏与空态：新建文件/文件夹（内联 + 模板）、导入、引用、排序、刷新；空态大图标 + 三按钮 ✅ | 同上 | 各按钮闭环 |
| B5 | 搜索：文件名实时过滤；内容模式（正则/大小写）→ 结果与**命中高亮**落中央编辑区 ✅（点击跳转待 Phase C） | 同上 + `EditorPanel` | 结果带上下文、高亮正确 |
| B6 | 右键菜单 + 键盘：重命名/删除/剪切/复制/粘贴/打开位置；F2/Delete/Ctrl+A/↑↓/Enter/Ctrl+N/Esc ✅（提升属 Phase D） | `panels/` + `commands.rs`（`scratchpad` context） | 全操作可用 |
| B7 | 回收站与撤销栏：折叠区列表/恢复/清空；删除后 5 s 撤销（自动消失 + 逐条还原）✅ | 同上 | 误删可恢复 |
| B8 | 多选（Ctrl/Shift）与批量删除 ✅ | 同上 | 菜单按单/多选自适应 |
| B9 | 草稿树虚拟列表（`v_virtual_list`，只渲染可视区）+ 排序（名称/大小/时间）；引用/回收站底部限高可滚 ✅ | 同上 | 大目录流畅、面板整体可达 |

> 落点修正：早期方案里的 `components/scratchpad_panel.rs` 不是实际落点；面板先落在 `panels/`（`SidebarPanel` / `EditorPanel`），**2026-09-16 再下沉至 `crates/scratchpad/src/scratchpad_view.rs`**（见“十四次”）。

### Phase C — 编辑器联动与生态

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| C1 | 中央编辑区「草稿箱文件模式」：`.sql` 打开 → 执行引擎 + 连接选择 + `Ctrl+S` 回存；`.py`/`.json`/`.md` 代码编辑器；防重复 Tab ✅ **首片已接（2026-09-16）**：双击/Enter/右键「打开」→ 编辑器（同路径只激活） | `crates/scratchpad/src/scratchpad_view.rs`（发请求）+ `workbench/src/view.rs::open_in_editor` | 双击打开、编辑回存正确 |
| C2 | `file_meta` 联动：**打开时自动选连接** ✅（2026-09-17）+ **执行后写 `last_connection_id`/`last_executed_at`** ✅（2026-09-17，编辑器回执 + 宿主 1 s 泵） | `scratchpad` store（`file_meta` / `preferred_connection` / `update_file_meta`）+ `editor::shared::ExecReceipt` + `workbench/src/services/scratchpad_meta.rs` | 连接自动恢复 ✅ |
| C3 | 脏点回显：编辑器未保存修改 → 草稿树文件名前实心圆点（**只有文件**）✅ **已接（2026-09-17）**：经 `ScratchpadHost::dirty_files` 取绝对路径集合，1.2 s 一拍比对缓存 | `crates/scratchpad/src/{host,scratchpad_view}.rs` + `workbench/src/components/scratchpad_host.rs` | 改一处出现、`Ctrl+S` 后消失 |
| C5 | 拖草稿文件到编辑区 → 插入内容到光标处 ✅ **已接（2026-09-17）**；系统文件拖入树导入 ⬜（无 OS 拖放入口，暂用工具栏 `⬇`） | `shared/src/drag.rs` + `scratchpad_view.rs` + `editor/src/view/host.rs` | 拖放生效 ✅ |
| C4 | 冲突处理：外部修改 → 冲突条 → `diff_with_content` Diff 面板 → 重载（照磁盘）/ 忽略 ✅ **已接（2026-09-17）**：内容判据（非 mtime）；差异落中央编辑区，消解动作在侧栅 | `crates/scratchpad/src/{jobs,host,scratchpad_view}.rs` + `workbench/src/panels/{shared,mod,editor}.rs` | 冲突可消解 ✅ |
| C5 | 搜索替换：预览计数 → `replace_in_file`（正则/大小写）→ 原子写回 → 刷新 ✅ 已落地（结果栏内嵌替换栏；Diff 预览仍未接） | 同上 | 替换后结果自动刷新 |
| C6 | 提升为分析资源：经 command/event 调 `analytics_resource`，**移动 + 归档锁定**（详见 Phase D） | `scratchpad` 命令 + 分析资源服务 | 提升后事件刷新 |
| C7 | 迁移文档验收：`cargo check --workspace` 零告警；无 `unwrap/expect` 新增；架构红线复核 | 全仓 | 全绿 |

### Phase D — 提升 / 存档 / 取回（分析资源）

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| D1 | `resources/` 模块根约定（`{project}/resources/`）与资源登记模型（`resource_id / version / hash / promoted_from / readonly`） | `crates/analytics_resource` + `project.db` | 登记可查询、可回读 |
| D2 | 提升：`scratchpad/ → resources/` **move + 归档**，随文件归档数据源绑定/来源；完成后发事件刷新两侧 | `rds-scratchpad` 命令 + 资源服务 | 提升后草稿消失、资源只读 |
| D3 | 锁定：文件系统只读属性 + 应用守卫（禁重命名/移动；编辑器只读打开） | 存储 + 编辑器 | 资源不可写 |
| D4 | 取回（检出）：资源**复制**回 `scratchpad/`（新名 + 派生关系），资源本体不动 | 资源服务 + 草稿箱 | 可取回并编辑 |
| D5 | 版本：再次提升生成**新版本**（稳定 `resource_id`），旧版保留 | 资源服务 + `project.db` | 版本递增、引用不断 |
| D6 | 资源删除 → 项目级回收站（`origin = "resources"`；在资源模块还原） | `ProjectTrash` + 资源服务 | 条目来源正确、不在草稿箱误还原 |

## 3. 测试场景清单（参照 v1 §四，Phase A 覆盖 1–6）

1. **根语义**：项目根有 `a.sql` / `data/report.sql`，面板只列用户文件，`.RSmeta` 不出现
2. **新建/重命名**：根与子目录内联创建、重命名（重名/空值被拒）
3. **软删除/恢复**：删除 → 回收站计数 +1 → 撤销栏恢复 → 文件回到原位
4. **移动/复制**：剪切粘贴跨目录；复制生成 `_copy`
5. **外部引用**：添加别名引用 → 列表显示；非法路径置灰；移除生效
6. **路径防护**：传入 `../`、`.RSmeta/x` 被拒
7. **可分析文件**：`.csv`/`.parquet` 被 `get_analyzable_files` 识别，相对路径正确（Phase B）
8. **搜索**：文件名过滤；内容模式正则/大小写；结果上下文与跳行；>500 条截断（Phase B）
9. **编辑器**：`.sql` 草稿模式打开、执行、`Ctrl+S` 回存；连接自动恢复（Phase C）
10. **冲突/Diff**：外部修改触发冲突 → Diff 红绿 → 接受（Phase C）
11. **监控**：外部新建文件面板自动刷新；`.RSmeta` 变更不触发（Phase A）
12. **大目录**：>50 条启用虚拟滚动，滚动/排序无卡顿（Phase B）
13. **主题**：明暗切换核对面板底/选中条/搜索框/菜单/撤销栏（`theme-preview.html` 为基准）
14. **旧数据迁移**：存在 `.scratchpad/` 的旧项目启动后迁移且幂等（Phase A）

## 4. 风险与对策

| 风险 | 对策 |
| --- | --- |
| 缺少项目会话（P0）导致草稿箱无内容 | P0 优先；未完成时明确空态，不阻塞后端 A 阶段 |
| 模块根内若误放内部态，泄露到 UI 或被 API 访问 | 内部态统一 `.RSmeta/<模块>/`（天然在模块根之外）+ `resolve_path` 拒绝点前缀路径；测试覆盖 |
| 回收站被跨模块误还原 | 条目带 `origin`；`ScratchpadStore::restore_from_trash` 只接受 `scratchpad` 来源 |
| 提升后仍能改一份，与存档分叉 | 提升 = move + 只读锁定；修改只能取回（检出）后提升新版本（Phase D） |
| 文件监控把 `.RSmeta`（SQLite/DuckDB）变更当作刷新信号，造成抖动 | watcher 过滤 `.RSmeta` 与临时文件（`-wal`/`-shm`/`~`） |
| 240px 面板塞入搜索/替换/Diff 过挤 | 导航留侧栏，重结果/弹窗落中央编辑区（原型 §4.3/§4.5） |
| 根目录可能很大（用户整个项目） | 懒加载 + 虚拟列表 + 内容搜索流式/超时/截断 |
| 脏状态与外部冲突误判 | 脏点集合与 watcher 事件联动；冲突弹窗人工裁决 |
| 模板/图标颜色对比度不足 | 先用标准 token；不足再补产品语义 token（原型 §6.4） |
| `.RSmeta` 与 `.scratchpad` 命名/大小写不一致 | 统一走 `project` crate 的 `RS_META_DIR_NAME`（`.RSmeta`），不各写各的 |

## 5. 实现位置映射（设计决策 → 代码文件）

| 设计决策 | 代码文件 |
| --- | --- |
| 模块根 = `{project}/scratchpad/`；内部元数据 `.RSmeta/scratchpad/` | `crates/scratchpad/src/store.rs`（`ScratchpadStore::new` / `ensure_dir` / `scan_dir_tree` / `resolve_path_impl`） |
| 项目级回收站（含来源/原路径） | `crates/engine/src/persistence/trash.rs`（`ProjectTrash` / `TrashEntry` / `TrashManifest` / `TrashKind` / `TrashRestoreOutcome`；**P0.8 已从 `crates/scratchpad/src/trash.rs` 上提中性化**，草稿箱侧只重导出） |
| 文件元数据 / 数据源绑定 / 外部引用可用性 | `crates/scratchpad/src/models.rs`（`FileMeta::bound_connections` / `FileMeta::preferred_connection` / `ExternalReferenceStatus`）、`store.rs`（`file_meta` / `bind_connections` / `update_file_meta` / `external_reference_status`） |
| 旧 `.scratchpad/` 迁移（→ 模块根/元数据/项目回收站） | `crates/scratchpad/src/store.rs`（`migrate_legacy_layout` / `move_dir_contents` / `ingest_trash_dir`） |
| 文件监控 | `crates/scratchpad/src/state.rs`（`notify`）+ 事件推送 |
| 域模型 / 存储 API | `crates/scratchpad/src/{models,state,store}.rs` |
| 递归复制 / 移动 / 替换 / 引用重定位 | `crates/scratchpad/src/store.rs`（`copy_entry` / `copy_dir_contents` / `move_entry` / `replace_in_file` / `update_external_reference_path`） |
| 内容搜索（含命中区间） | `crates/scratchpad/src/store.rs`（`search_file_content` / `literal_match_spans`）+ `models.rs::SearchMatch::match_spans` |
| 面板视图（工具栏/树/虚拟列表/空态/引用/回收站/撤销栏） | `crates/scratchpad/src/scratchpad_view.rs`（`ScratchpadView`：`render_scratchpad` / `scratchpad_row` / `render_scratchpad_edit_row` / `render_scratchpad_empty_state` / `scratchpad_move`） |
| 内容搜索结果 + 替换栏 | `crates/scratchpad/src/scratchpad_view.rs`（`render_scratchpad_search_pane` / `run_scratchpad_search`）+ `crates/workbench/src/panels/editor.rs::replace_scratchpad_all` |
| 宿主端口 / 端口实现 | `crates/scratchpad/src/host.rs`（`ScratchpadHost`）+ `crates/workbench/src/components/scratchpad_host.rs` |
| 快捷键 / 尺寸常量 | `crates/scratchpad/src/commands.rs`、`crates/workbench/src/commands.rs`、`crates/workbench_shell/src/ui.rs`、`crates/app/src/main.rs` |
| 左 Dock 装配（`LeftPanel::Draft`） | `crates/workbench/src/panels/`（`SidebarPanel` 持 `Entity<ScratchpadView>`） |
| 当前项目会话 | `crates/workbench/src/services/project_session.rs`、`crates/app/src/main.rs` |
| 中央编辑区草稿文件模式（Phase C） | 打开：`crates/workbench/src/view.rs::open_in_editor`（含 C-2 连接预选）；后续（脏点/冲突）待定 |
| 依赖接线 | 根 `Cargo.toml`（`scratchpad` 别名）、`crates/workbench/Cargo.toml` |
| 搜索高亮 token | `assets/themes/product-tokens.json` + `crates/workbench_shell/src/product_tokens.rs`（`search.match.background`） |

> 早期方案中的 `crates/workbench/src/components/scratchpad_panel.rs` **未创建**；面板实现 2026-09-16 前在 `panels/`，**2026-09-17 起在 `crates/scratchpad/src/scratchpad_view.rs`**。

## 6. 验证方式

- 每阶段：`cargo check -p rds-scratchpad -p rds-workbench -p rds-app --all-targets -j 2` 零告警 + 对应测试（`crypto` 之外的单测均在 `store.rs` 内联模块）
- 后端单测基线：`cargo test -p rds-scratchpad -j 2 --lib`（**37 项**；含临时项目目录的端到端文件操作 + 面板纯函数 + 后台任务）
- UI：`cargo run -p rds-app` 手动走通 §3 场景清单与 `scratchpad-user-guide.md` §9 验收清单
- 主题：明暗切换核对 token（`docs/architecture/theme/theme-preview.html` 为基准）
- 阶段完成后回填本文件「进度记录」、`crates/scratchpad/README.md` 能力表与原型文档同步
