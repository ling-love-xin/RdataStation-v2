# 项目管理模块（M1）· 模块入口

> **一句话**：「一个实例一个项目」的本地优先项目工作区——项目注册与生命周期、`.RSmeta` 元数据、实例锁与未保存草稿保护，加选择器 / 标题栏菜单 / 项目设置 / 语义对话框全套视图。
>
> 本文只做**文档索引 + 特点速览**：特点的完整提炼与代码落点见 crate 入口 [`crates/project/README.md`](../../../crates/project/README.md)，设计细节见下列文档，本目录不重复设计。

## 文档索引

| 文档 | 内容 | 什么时候看 |
| --- | --- | --- |
| `project-prototype-design.md` | 交互语义与原型：9 条决策、CRUD 场景、状态机、主题映射、范围外清单 | 改交互、评估范围 |
| `project-prototype.html` | 可交互原型（RDS 明暗双主题） | 视觉 / 文案对齐 |
| `project-dev-plan.md` | 任务划分（P0/A/B/C）、进度记录、测试场景清单、风险与对策、实现映射表 | 查历史决策与待办 |
| `project-view-architecture.md` | 宿主桥 `ProjectUiHost` 契约、状态所有权、对话框栈语义、窗口测试方案与坑 | 改 `ui.rs`、写视图测试 |
| `project-user-guide.md` | 使用手册：入口 / 界面导览 / 典型流程 / 状态速览 / 快捷键与键盘操作 / 数据与安全 / FAQ / 验收清单 | 自己用或写验收用例 |
| `project-showcase.md` | **一页看懂（可贴版）**：一句话 + 四个剧本 + 流程 / 三道门 / 只读两道防线 / 架构 / 质量与证据 / 边界；纯文本 + 表格 + mermaid | 宣传、贴 PR / wiki / 聊天 |
| `project-showcase.html` | **一页看懂（视觉版）**：首屏可切「选择器 / 项目设置 / 逃生口」线框，可点着试 facet 收窄与只读置灰；明暗双主题、离线可开 | 演示、截图、给外部人看 |
| `project-wiring-matrix.md` | **接线矩阵**：逐方法标 已接线 / 预留 / 待删；死代码清理与归属决策台账 | 改 `store.rs` / 排查死代码 |
| `project-data-dictionary.md` | **数据字典**：`.RSmeta` 目录树、`project.db` / `global.db` 表结构、`project.json` schema、权威约定 | 改表结构 / 排查数据 |

## 特点速览

- **一实例一项目**：会话唯一（`OpenProject`），不支持多窗口打开同一项目；切换 = 在**项目中心**（覆盖层）里选目标，先释放当前项目再打开——不再要求先关项目回选择器。
- **名册与元数据分离**：全局库 `project_info`（固定 / 软删 / 最后打开）+ 项目内 `.RSmeta/`（`project.db` / `analytics.duckdb` / `project.json`）；删除只动 `.RSmeta`，用户文件保留。
- **两道护栏**：OS 文件锁（退出自动释放，带「只读打开 / 仍要打开」逃生口）+ 未保存草稿拦截（确认后直达目标对话框）。
- **视图与 model / service 同 crate**：`ui.rs` 经 `ProjectUiHost` 注入宿主能力，workbench 只剩 `components/project_host.rs` 桥接。
- **状态可恢复、偏好可持久**：软删找回、失效路径重定位、固定置顶、排序方式跨会话保留；内置示例项目。

## 代码落点

`crates/project/src/`：`models.rs`（域模型）、`store.rs`（`.RSmeta` / 配置）、`lock.rs`（实例锁）、`service.rs`（编排）、`ui.rs`（公共面：状态 / 宿主 / 输入实体 / `PendingAction`）+ `ui/{actions,dialogs,host,hub,picker,settings}.rs`（实现）+ `ui/tests.rs`（视图测试，50 项窗口/纯函数：31 窗口 + 19 纯函数；本 crate lib 共 63 项）。职责表见 crate README。

## 状态

- 主线完成（2026-09-14）：CRUD / 生命周期 / 实例锁 / 选择器（搜索 ∩ 状态筛选）/ 项目设置（名称 + 描述）/ 系统目录选择器 / 空目录询问。
- 规范补齐（2026-09-19）：只读禁用态（写命令可见但置灰，拦截层兜底）/ 选择器键盘可达（Tab + Enter 真能操作）/ 设置·危险区三件套（归档 / 移出列表 / 删除数据）/ 标题栏菜单改 `Button::dropdown_menu`（规格抽纯函数可断言）。
- 信息补齐（2026-09-19 二）：`.RSmeta` 结构树（目录在前 + 大小 + 复制路径 + 合计）/ 描述上卡片与概览 / 设置面板的磁盘与名册读取改**快照**（事件路径取一次，render 不做 I/O）。
- 功能补齐（2026-09-19 三）：**默认连接（U3）**——后端 `store::update_config` + `service::{load_default_connection, save_default_connection}`，UI 设置·默认项下拉（宿主注入候选，空 = 不设默认，记录值失效如实显示）。
- 体验补齐（2026-09-19 四）：卡片**右键菜单**（与 `⋯` 下拉同源规格）、搜索 **facet 语法**（`状态:` / `固定:` / `锁:`，与自由文本 / 筛选按钮叠加 AND）、逃生口补陈旧锁提示、只读拦截补两条路径的测试。**2026-11-22 实测**：lib 50 + 名册集成 1 + 生命周期集成 5 + 存储集成 3（全部通过）。
- 文档与实现对齐（2026-11-22）：①新建对话框改为**字段级行内校验**（名称走 `validate_project_name`、位置走 `location_hint`，判据与提交同源；按钮不置灰）；②新建**位置记忆**（`settings.projects.last_create_location`，登记为 `SettingEntry::Module`，不上设置页）；③**构建标识落项目**（非只读打开把 `version::info().tag()` 写进 `.RSmeta/config/settings.json` 的 `extensions.rds.build_tag`，只写不展示）。同批：gpui-kit 升级 **0.10.0**、持久化抽取到 `crates/store`（`engine::persistence` 仅重导出），`project-prototype-design.md` §13 记了 18 条与设计稿的差异。④（同轮打磨）新建对话框的行内提示不再每帧探测目录——`open_dialog` 的 builder 是**渲染闭包**（vendor 契约：cheap and idempotent），改为按「位置 / 名称」变化的局部 memo，只有输入变化才跑一次 `exists()` + `read_dir`；同名非法（含 `\ / :` 等）时不判目录，避免 `join` 拼出子路径判错。并补两组测试锚定：`location_hint` 六分支纯函数单测、`create_dialog_inline_hint_updates_while_typing` 窗口测试（键入后提示**同帧**出现，`debug_selector` + `debug_bounds` 断言）。`ui/tests.rs`：**24 窗口 + 14 纯函数**。⑤（同轮打磨 二）「打开现有项目」对话框补齐同源行内提示（`folder_hint`，文案逐字复用 `submit_open_folder`；空目录**不报错**，提交时才问是否在此创建），同样按路径 memo；测试：`folder_hint_matches_open_preconditions`（五分支）+ `open_folder_dialog_hint_flags_non_project_dir`（窗口，换路径后提示出现 / 消失）。⑥（同轮打磨 三）两处对话框的 memo 抽成 `ui::memo_hint`（去 32 行重复），单测 `memo_hint_recomputes_only_when_key_changes` 锁住"同 key 不重算 / 换 key 必重算 / None 也缓存"；同时把本 crate 3 条 `【探针】` 日志从 `info` 降为 `debug`（浏览时不再刷日志，排查时 `RUST_LOG=debug` 仍可见）。⑦（同轮打磨 四）**clippy 清零**：本 crate 告警 22 → 2，全部为无行为变化的小改——两处行内提示 memo 的 `Rc<RefCell<Option<(K, Option<String>)>>>` 收成 `ui::HintCache<K>`，`ProjectUiHost` 的 6 个闭包桥字段抽成 `pub type`（`SortSaver` / `HostAction` / `ConnectionProvider` / `BuildTagProvider` / `CreateLocationGetter` / `CreateLocationSaver`，宿主侧 `Rc::new(闭包)` 的写法不用改），`open_path` / `open_read_only` / `request_open` 由 `&PathBuf` 改 `&Path`，`lock.rs` 与 `service.rs` 的 4 处嵌套 `if` 折叠成 `let … && let …`，`ProjectUiState` 改 `#[derive(Default)]` 删掉手写 `impl`。余下 2 条有意保留（`OpenOutcome` 的 `large_enum_variant` 会把装箱成本推给 workbench、`dialog_footer_three` 的 8 参数是三个按钮的平行三元组），理由见 `project-view-architecture.md` §8。
- **项目管理入口补齐（2026-11-24，三刀）**：①**标题栏菜单 6 → 8 项（只读 9 项）**——新增「新建项目…」「打开现有目录…」（作用于别的目录、不写当前项目，只读下照样可用），「切换项目…」改名**「项目中心…」**；切换语义也真正落地：`request_switch` / `switch_now` 先释放当前项目（锁 + 宿主连接作用域）再打开目标，脏草稿走未保存拦截后直达 `SwitchTo`（不再把新锁盖到旧会话上）。②**项目中心覆盖层**（`ui/hub.rs`：`open_project_hub` / `close_project_hub` / `render_project_hub`）——有当前项目时也能浏览 / 管理别的项目，覆盖层复用 `render_picker`，当前项目卡片打「当前」`Tag`，选中别的项目即切换；`Ctrl+Shift+P`（`SwitchProject`）由 `request_close` 改为 `open_project_hub`，与 `Ctrl+Shift+W`（`CloseProject`）彻底分工。③测试 **42 → 48**（30 窗口 + 18 纯函数；lib 共 61），`ProjectUiState` 8 → 9 字段（加 `hub_open`）。
- **只读判据收窄（2026-11-24）**：只读锁保护的是**当前项目**自己的 `.RSmeta`，项目中心里别的项目不在锁的范围——`ui/host.rs::read_only_blocked` 拆出 `write_blocked(host, target, …)`（原 6 处调用语义不变），卡片侧新增 `item_write_locked`（只读**且**这张就是当前项目）供给 `card_menu_entries(item, tab, locked)` 置灰；`toggle_pin` / `soft_remove` / `restore` / `forget` / `open_delete_dialog` / `confirm_delete` 补目标项目兜底守卫（此前只有菜单置灰这一道）。测试 **48 → 50**（31 窗口 + 19 纯函数；lib 共 63）。
- 范围外（原型 §12）：提升 / 引用（promote / snapshot）、移动或另存项目目录、DuckLake 远程项目（`ProjectPath::Remote` 仅模型层预留）。
- 待办见 `project-dev-plan.md` **§8 未开发清单**：执行 SQL 的只读拦截测试（E5）、菜单快捷键（C2：0.10.0 的 `PopupMenuItem` 已有 `action` 槽位、`render_key_binding` 会按 `action_context` 解析键位，但 project crate 不依赖 workbench，需宿主先注入 action 工厂，暂缓）。窗口退出路径的草稿兜底（E3）已于 2026-09-19 完成（`app` 注册 `on_window_should_close` → 会话批量落库，见 `editor_session_real::leaving_the_window_stashes_every_open_document`）。
