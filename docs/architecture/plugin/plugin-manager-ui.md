# 插件管理页（Rust / GPUI）· 目录与接线

> 对应原型：`prototype/plugin-manager.html`（双承载：左 Dock 紧凑管理行 + 中央整页）。
> 本文只记 Rust 落地：**文件职责、数据所有权、入口与中央 tab 接线、尺寸/契约、测试与已知缺口**。
> 视觉与尺寸权威仍是 `../ui/`、`../theme/`；组件选型与 API 以本地 vendor `0.9.0` 为准。

---

## 1. 为什么是目录模块

`panels/left_tabs/` 目录已承载草稿箱 / 数据库导航 / 资源 / 插件四个左栏面板（每个面板一个文件或子目录，
共约 1500 行；拆分前是单文件 `panels/left_tabs.rs`）。其中插件面板再按职责拆成
`left_tabs/plugin_tab/`（`mod` / `delegate` / `panel` / `pure` / `render`，机械切分、零行为变更）。
插件管理页是**中央整页**，包含页头、统计、反馈、筛选、两列卡片、右侧详情栏，
若继续追加入 `left_tabs/` 的任一左栏文件，会把一个「左 Dock 面板文件」变成跨承载的混合体。

因此按 `panels/property_panel/` 的既有范式落成目录模块：

```text
crates/workbench/src/panels/plugin_manager/
├── mod.rs      面板协议 + 状态字段 + 动作转发 + tab 聚焦
├── render.rs   整页骨架（页头 / 统计 / 反馈 / 重载横幅 / 筛选条 / 滚动主体）
├── cards.rs    两列卡片网格与单卡
├── detail.rs   右侧固定详情栏
├── widgets.rs  筛选 / 统计 / 状态文案等纯函数 + 动作按钮
└── tests.rs    纯函数单测（不渲染窗口）
```

新增页内区块时按职责落到上述文件；**不要**再往 `left_tabs/plugin_tab/` 加管理页内容。

---

## 2. 数据所有权（只有一份）

管理页**不复制清单**。数据面仍是左 Dock 的 `PluginPanel`：

| 方向 | 通道 | 说明 |
| --- | --- | --- |
| 读 | `PluginPanel::{plugins_snapshot, contributions_snapshot, progress_text, error_text, notice_text}` | 一次 `read` 克隆为 `PluginSnapshot`，渲染子树不再反复 lease 面板实体 |
| 写 / 动作 | `PluginPanel::{refresh_plugins, pick_package_file, pick_package_dir, retry_plugin, set_plugin_enabled, uninstall_plugin, reload_plugins}` | 管理页只转发，后台任务、互斥忙标记、事件订阅全部复用左栏既有实现 |
| 刷新通知 | `cx.observe(&plugin_panel, …)` | 左栏刷新 / 动作回执 / 生命周期事件写入后自动重绘管理页 |
| 重载提示 | `plugin::get_plugin_manager().pending_reloads()` | 只读查询；`>0` 才渲染「立即重载」横幅 |

这条复用是刻意的：管理页与左栏不会各刷一份、各自过期；`PluginPanel` 的
「同一时间只允许一个操作」互斥也自然覆盖管理页。

---

## 3. 界面骨架

| 区 | 内容 | 落点 |
| --- | --- | --- |
| 页头 | 「插件管理」+「本地已安装 · 所有操作立即生效」+ 安装插件 / 从目录导入 / 垂直分隔 / 刷新 | `render.rs`（`Toolbar`） |
| 统计行 | 全部 / 已启用 / 已停用 / 出错 | `render.rs` + `widgets::plugin_stats` |
| 反馈条 | 错误 > 进行中 > 成功回执（三选一） | `render.rs` |
| 重载横幅 | 待重载数量 +「立即重载」 | `render.rs` |
| 筛选条 | 搜索 `Input` + 五档分段 `TabBar` + 「共 N 个」 | `render.rs`；筛选/搜索纯函数在 `widgets.rs` |
| 主体左 | 两列卡片网格，奇数行补空位 | `cards.rs` |
| 主体右 | 340px 详情栏：基本信息 / 贡献点 / 驱动 / 依赖关系 / 权限 / 运行状态 / 管理动作 | `detail.rs` |

筛选五档为 `全部 / 已启用 / 已停用 / 出错 / 含驱动`——**没有**原型里的「可更新」：
RDS 当前没有市场或远程索引，保留该档会变成永远空空如也的死入口。
「含驱动」不按启用状态筛，排障时先定位驱动、再看它为什么没进下拉；搜索词同时匹配
驱动 id / 显示名 / 分类（用户记不住 `rds.driver.oracle-rust` 这类插件包名）。

**依赖关系（2026-11-10）**：详情栏「依赖关系」段分两向——「本插件依赖」来自清单
`dependencies`；「被其他插件依赖」来自 `DependencyManager::all_dependents`
（遍历 `global.db` 插件表的 `manifest_json`，一次算全量反向索引）。
`PluginInfo` 新增 `dependencies: Vec<String>` 与 `dependents: Option<Vec<String>>`；
缓存与 `registered_drivers` 同口径（`None` = 全局库未初始化，`Some(空)` = 确实没有），
在 `resync_sidecar_driver_rows` 里刷新，`list_plugins` 只读缓存、不碰异步库。
卸载时的拒绝判断仍走实时 `check_dependents`，与这里展示的缓存是同一条实现
（`check_dependents` 现在是 `all_dependents` 的一行查表）。
反向依赖已知非空时，卡片与详情栏的「卸载」按钮直接禁用并在 tooltip 列出依赖者；
「停用」按钮同样置灰并在 tooltip 说明「停用会让谁失效」，不让用户点下去才被拒绝；
缓存未知时不前置拦截，仍由卸载时的实时检查兜底（不把「没读到」当成「没人依赖」）。

**停用依赖保护（2026-11-11）**：停用同样会让依赖方立即失去驱动。左栏卡片、详情栏
与管理页冲突卡点的单插件停用，以及「停用全部已启用」，都先读同一份 `dependents`
缓存：单插件被依赖时拒绝并写清「被谁依赖、先处理依赖方」；批量停用时只要选中集合里
有任意一个被依赖就整批拒绝，并列出被拦截的 id，避免出现「停了一半、状态更难判断」。
`dependents = None` 时不拦（与卸载同口径）。注意：拦截目前只在面板动作入口，
`PluginManager::disable_plugin` 本身仍未查依赖，非面板调用仍可能绕过；根治需在该方法
里接 `check_dependents`。

**手动刷新（2026-11-10）**：页头与左栏的刷新按钮改为先 `resync_sidecar_driver_rows`
再 `list_plugins`，不再只重读清单。否则登记状态只在启停 / 加载等生命周期动作里刷新，
用户手动刷新后仍会长期停在「登记状态未知」；同步失败只记日志，不拦清单刷新。

**批量动作（2026-11-11）**：统计行右侧给「启用全部已停用（N）」「停用全部已启用（N）」，
出错数大于 0 时再出现「重试全部出错（N）」，插件一多或冲突告警需要一次处理多个时不用逐个点。
归批的状态由纯函数判定：启用只碰 `Inactive`，停用只碰 `Active` / `Loaded`，重试只碰 `Error`；
`Error` 不参与启停（没有可停的运行时，且启用前得先修清单或解冲突）。
停用全部走确认框（会一次性改变所有驱动的下拉可见性），插件目录与 `plugin-data` 保留。
整批只占一个后台任务并按顺序执行，单个失败不中断，结束后统一报
「已启用 X 个 / 停用 X 个 / 已重试并加载 X 个，失败 Y 个：<id>: <原因>」；失败文案写在 `error` 位，
随后的清单刷新传 `clear_error=false`，不会被成功刷新擦掉。
批量重试针对的是「冲突方停掉后另一个仍停在 Error」：冲突拒绝发生在清单未变时，
热插拔扫描不会自动重试，必须显式再解析一次清单。

**驱动可见性（2026-11-10）**：`plugin::PluginInfo` 新增 `drivers: Vec<PluginDriverSummary>`，
由清单 `[[contributes.drivers]]` 直接派生（id / display_name / default_port / category），
管理页在卡片上加驱动 chips（每个 chip 直接标「已登记 / 已登记 · 已停用 / 未登记」），
详情栏加「驱动」段。这样不用点进详情就能看出「插件在、下拉里有没有」。驱动段只做展示，不改登记链路；
登记仍由 `driver_decl::resync_driver_rows` 写 `global.db` 的 `drivers` 表。
另：`backend.runtime = "rust"` 的原生 sidecar 在部署期**不再解析解释器**
（可执行文件就是插件自带的 agent），因此不会再被
`RuntimeResolver::resolve` 判成 `COMMON_NOT_SUPPORTED` 而清掉驱动行。

**贡献点明细（2026-11-10）**：`PluginInfo` 新增 `contribution_items: Vec<PluginContributionSummary>`
（`kind` / `id` / `label` / `detail`），由清单派生：命令取 `category` + `shortcut`，面板取 `location`，
设置取 `type` + `default`（JSON 串）。详情栏在「贡献点」摘要行下逐条列出；驱动不进这个列表，
仍由「驱动」段单独展示——`ContributionRegistry` 本身也不收录驱动。

**权限段（2026-11-10）**：详情栏五组 chips 分别对应清单 `[permissions]` 的
`sidecar`（进程）/ `driver`（数据面）/ `frontend`（前端）/ `wasm`（沙箱）/ `host`（Host API）。
`credentials = "database"` 时用 `warning` 色显式提示「会接收数据库密码（仅用于建立连接）」；
`host` 非空时补一行「默认拒绝：声明只表示申请，不等于已放行」。
按 `manifest.rs` 的口径，`sidecar` / `driver` 是**展示与告警轨、不门控**；`host` 默认拒绝，
需要用户逐项放行。空清单显示「清单未声明权限。」

**驱动登记状态（2026-11-10）**：清单声明 ≠ 下拉可见。`PluginInfo` 再增
`registered_drivers: Option<Vec<PluginDriverRegistration{ id, enabled }>>`：
`None` 表示本次没读到全局库（文案「登记状态未知」），`Some(空)` 才是确实一行都没登记。
快照由 `PluginManager` 在 `resync_sidecar_driver_rows` 成功后从 `global.db.get_all_drivers()`
筛 `driver_kind='sidecar'` 缓存，`list_plugins` 纯读缓存，不在同步路径碰异步库。
详情栏每条驱动显示「已登记 / 已登记 · 已停用 / 未登记」，颜色依次 success / warning / danger；
这样「插件显示已启用、下拉却没有」时，可直接定位是登记缺失而不是驱动实现缺失。
页头统计行另加一格「驱动 N/M 已登记」（`plugin_driver_stats`）：
声明了驱动却一个都没登记、或本次读不到登记表时用 `warning` 色，下拉为空时第一眼就能看到。
这一格只读缓存，不做额外查询。

统计行下新增**驱动链路**（`plugin_driver_chain`，纯函数）：
`清单声明 → 插件启用 → 写入 drivers 表 → 连接下拉` 四段各自带状态点与计数，
并在末尾给出当前卡点（无驱动声明 / 插件未启用 / 登记表未读取 / 驱动行未写入 /
行已写入但全部停用（激活失败，看插件错误与 `sidecar.log`） / 部分不可用）。
第三段会分开数「行已写入」与「行可用」：`enabled = false` 的行不算可用，也不报成「没写入」。
四段全部由 `PluginInfo` 快照推导，不查库；「读不到登记表」只降为警告，不判成失败。
预览页的链路条同步改成按 `PLUGINS` 现算，切换插件启停后四段计数会跟着变。

**驱动 id 冲突（2026-11-10）**：同一驱动 id 被多个**启用中**插件声明时，加载期会拒绝，
两个插件都不可用（实例：JDBC 版与 Rust 版 Oracle 都贡献 `oracle`）。管理页把这个静默失败
显式摊开：`driver_id_conflicts` 按驱动 id 聚合启用插件的贡献点，统计行文案追加
「· N 个驱动 id 冲突」，链路第二段标 `Bad` 并在卡点里写出冲突 id 与两个插件 id，
下一步动作是停用或卸载其中一个插件。冲突卡点旁直接给「停用 `<插件 id>`」按钮，
点一下就走 `PluginPanel::set_plugin_enabled(.., false)`，不用用户再去列表里找。停用的插件
不参与判定，避免「看到冲突却不知道该停谁」。

**驱动登记总表（2026-11-11）**：链路行尾新增「登记明细（N）」按钮，默认收起；
展开后按「驱动 id → 插件 id」列出每个插件声明的每个驱动：驱动 id、显示名、
插件 id、登记状态（已登记 / 已登记 · 已停用 / 未登记 / 登记状态未知），
被多个启用插件重复声明的行尾追加「· id 冲突」。数据全部来自 `PluginInfo`
的快照（`driver_registry_rows` 纯函数），不额外查库；`registered_drivers = None`
仍显示「登记状态未知」，不降级成「未登记」。复用同一行解决三个问题：
哪个驱动由谁声明、登记行是否启用、是否撞车。每行插件 id 右侧带「查看」，
点一下直接在右侧打开该插件详情，省去回卡片区逐个查找。
插件未启用时插件 id 用弱化色，与卡片区状态口径一致。

**文件与日志入口（2026-11-10）**：`PluginInfo` 增 `install_path: Option<PathBuf>`
（`plugin.toml` 所在目录；wasm 运行时列表不回报磁盘路径 → `None`；坏目录按
`paths::plugin_dir(id)` 兜底，路径不存在也给 `None`）。详情栏「管理动作」下补两枚按钮：
「目录」用 `opener::open` 在文件管理器打开安装目录，「日志」打开
`paths::plugin_cache_dir(id)/sidecar.log`。日志文件不存在（驱动从未启动）时按钮禁用并
在 tooltip 写明原因，不做点了没反应的死按钮；打开失败写进顶部 notice，不占用 error 段。
日志路径由 `plugin::sidecar_log_path(id)` 提供，与 sidecar 启动时 stderr 落点同一口径。
同一行还有「复制诊断」：`plugin_diagnostics` 把插件 id / 状态 / 错误 / 运行形态 / 安装目录 /
日志路径 / 每个驱动的登记结果 / 本插件依赖与被依赖拼成纯文本写入剪贴板，
省去让用户逐段截图；依赖未知时明确写「未知（全局库未初始化）」，不冒充「无」。

独立预览页：`prototype/plugin-manager-preview.html`（单文件、无网络、双击即开），
在管理页形态上额外画了「驱动链路」条与连接对话框下拉演示；详情栏已与 GPUI 同步
「运行形态 / 贡献点 / 依赖关系 / 权限 / 驱动登记状态」五段，状态栏按
`已登记/声明` 汇总驱动，用于先评审前端。

---

## 4. 入口与中央常驻 tab

```text
左 Dock 插件面板页头「管理」按钮（plugin-open-manager）
  → Shared::show_plugin_manager(window, cx)
  → Shared::open_plugin_manager 回调（WorkbenchView::init_workspace 装配，现于 `crates/workbench/src/view/workspace.rs`）
  → DockArea::add_panel_view(panel_handle(panel), DockPlacement::Center, …)
  → focus_plugin_manager_tab(&panel, window, cx)
```

- **单实例**：面板实体在 `WorkbenchView::init_workspace` 创建一次，由
  `WorkbenchView::plugin_manager: Option<Entity<PluginManagerPanel>>` 强持有（字段仍在 `view/mod.rs`）。
  打开回调先按 `PanelId` 在 Center / Left / Right / Bottom 四个区域查 `contains_panel`：
  已存在则 `DockArea::select_panel` 只聚焦，**不再** `add_panel_view`——否则
  `add_panel_inner` 会在 tab 组里再插一个指向同一实体的节点。
  因此重复点击只切回原 tab，搜索词 / 筛选 / 选中项不丢。
- **不新增 `LeftPanel` 变体**：`LeftPanel` 是硬编码 `Copy` 枚举，新增变体会连锁改
  5 处穷尽 `match`；插件图标也不存在（`IconName::Plugin` 全仓无定义），
  入口按钮用 `IconName::Settings`，tab 标题用 `LeftPanel::Plugin.icon()`。
- **聚焦顺序是硬约束**：`focus_plugin_manager_tab` 必须在 `area.update` 闭包外调用。
  `TabGroup::select_tab` 会回读面板实体，在实体更新闭包内调用会 double lease panic
  （与 `focus_property_tab` / `focus_search_tab` 同口径）。

---

## 5. 尺寸与契约

新增结构尺寸先登 `crates/workbench_shell/src/ui.rs`（已登记）：

| 常量 | 值 | 用途 |
| --- | --- | --- |
| `PLUGIN_MANAGER_DETAIL_WIDTH` | `21.25`（340px） | 右侧详情栏 |
| `PLUGIN_MANAGER_SEARCH_WIDTH` | `18.75`（300px） | 搜索框 |
| `PLUGIN_CARD_MIN_HEIGHT` | `10.5`（168px） | 卡片最小高度 |

`ui_contract.rs` 三处登记：

1. 契约 2a 尺寸清单：6 个 `.rs` 的 `include_str!`；
2. 契约 2b 颜色清单：同样 6 条；
3. 契约 2c `every_panel_module_is_registered_in_the_manifests`：每个文件带引号路径
   在 `ui_contract.rs` 文本里出现 ≥2 次（上述两处各一次）；
4. 契约 4 `shared_fields_are_whitelisted`：`open_plugin_manager` 进白名单。

颜色一律 `cx.theme().colors.*`，零 hex；滚动主体
`flex_1() + min_h_0() + overflow_y_scrollbar()`，列容器外层 `items_stretch()`。

---

## 6. 验证

```powershell
$env:CARGO_TARGET_DIR='D:\RdataStation\RDS\RdataStation-v2\.rds\target-gf'
$env:RUST_MIN_STACK='16777216'

cargo check -p rds-workbench --lib
cargo test  -p rds-workbench --lib plugin_manager
cargo test  -p rds-workbench --test ui_contract
```

2026-11-10 实测：`check` 0 error；`plugin_manager` 7 个纯函数单测全绿；
`ui_contract` 10 个契约测试全绿。本模块 0 warning（余下 2 条为
`property_panel` 既存未用变量）。

2026-11-11 复测（批量动作）：`cargo check -p rds-workbench --lib` 0 error；
`cargo test -p rds-workbench --lib` 62 passed（`bulk_actions_skip_states_they_cannot_change` 同时覆盖启停与重试的状态面）；
`cargo test -p rds-workbench --test ui_contract` 10 passed。

2026-11-11 复测（驱动登记总表）：`cargo check -p rds-workbench --lib` 0 error；
`cargo test -p rds-workbench --lib` 63 passed
（新增 `driver_registry_rows_join_declarations_with_registration_state`：四态 × 冲突 + 排序）；
`cargo test -p rds-workbench --test ui_contract` 10 passed。

---

## 7. 已知缺口（不当成已完成）

| 项 | 状态 |
| --- | --- |
| 只读的「谁引用了某插件」查询 | 已完成（2026-11-10）：详情栏「依赖关系」段双向接线（`dependencies` / `dependents`），`Some(空)` 显示「无」、`None` 显示「未知」 |
| 市场 / 远程索引 / 可更新档 | 未引入；筛选五档不含「可更新」 |
| 面板徽标区分「DB 停用」与「未加载」 | 未做；根因是 `PluginInfo` 没有 `is_enabled` 字段，DB 权威启停状态不进会话快照 |
| 停用被依赖插件 | 面板侧已拦截（2026-11-11）：单插件与批量停用都会按 `dependents` 缓存拒绝；`PluginManager::disable_plugin` 本身仍未查依赖，非面板调用可绕过 |
| `schema_version` / `plugins.updated_at` 索引 | 未拍板 |
| 左 Dock 三个薄壳（`DatabaseNavPanel` / `ScratchPanel` / `ResourcePanelTab`）拆分 | 已完成（2026-11-10）：`panels/left_tabs/` 目录（`mod` / `database_nav` / `scratch` / `resources` / `tests` + `plugin_tab/` 五文件子目录） |
