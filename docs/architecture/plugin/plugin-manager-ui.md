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
| 筛选条 | 搜索 `Input` + 四档分段 `TabBar` + 「共 N 个」 | `render.rs`；筛选/搜索纯函数在 `widgets.rs` |
| 主体左 | 两列卡片网格，奇数行补空位 | `cards.rs` |
| 主体右 | 340px 详情栏：基本信息 / 贡献点 / 运行状态 / 管理动作 | `detail.rs` |

筛选四档为 `全部 / 已启用 / 已停用 / 出错`——**没有**原型里的「可更新」：
RDS 当前没有市场或远程索引，保留该档会变成永远空空如也的死入口。

详情栏「项目引用」一段目前是**如实占位**：Rust 侧只有 `uninstall_plugin`
拒绝卸载时返回引用者清单，没有只读的「谁引用了该插件」查询接口。

---

## 4. 入口与中央常驻 tab

```text
左 Dock 插件面板页头「管理」按钮（plugin-open-manager）
  → Shared::show_plugin_manager(window, cx)
  → Shared::open_plugin_manager 回调（WorkbenchView::init_workspace 装配）
  → DockArea::add_panel_view(panel_handle(panel), DockPlacement::Center, …)
  → focus_plugin_manager_tab(&panel, window, cx)
```

- **单实例**：面板实体在 `WorkbenchView::init_workspace` 创建一次，由
  `WorkbenchView::plugin_manager: Option<Entity<PluginManagerPanel>>` 强持有。
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

---

## 7. 已知缺口（不当成已完成）

| 项 | 状态 |
| --- | --- |
| 只读的「谁引用了某插件」查询 | 查询接口已有（`DependencyManager::check_dependents`）；详情栏尚未接线，当前为如实占位 |
| 市场 / 远程索引 / 可更新档 | 未引入；筛选四档不含「可更新」 |
| 面板徽标区分「DB 停用」与「未加载」 | 未做；当前沿用左栏状态语义 |
| `schema_version` / `plugins.updated_at` 索引 | 未拍板 |
| 左 Dock 三个薄壳（`DatabaseNavPanel` / `ScratchPanel` / `ResourcePanelTab`）拆分 | 已完成（2026-11-10）：`panels/left_tabs/` 目录（`mod` / `database_nav` / `scratch` / `resources` / `tests` + `plugin_tab/` 五文件子目录） |
