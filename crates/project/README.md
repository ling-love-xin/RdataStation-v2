# rds-project — M1 项目管理

> 设计文档在 `docs/architecture/project/`；本文件是模块入口，只提炼**特点**与落点，不复述设计。

## 定位

「一个应用实例 = 一个项目」的本地优先项目工作区。负责项目的注册与生命周期（增删改查 / 固定 / 归档 / 软删找回 / 重定位 / 版本台账）、磁盘元数据（`.RSmeta/`）、实例锁与未保存草稿保护，以及选择器、标题栏菜单、项目设置、语义对话框全套视图。

## 模块特点

### 一个实例一个项目

- 会话只有一处：`OpenProject { root, name }`，宿主与视图共享同一个 `Rc`（`host.session` / `Shared.project`）。
- 刻意**不支持多窗口打开同一个项目**；打开时被拒的项目给「只读打开 / 仍要打开」逃生口。
- 「切换项目」= 关闭当前 + 回选择器；无项目时选择器覆盖中央区（保留五段外壳）。

### 数据落点：名册与元数据分离，双写一致

| 层 | 位置 | 内容 |
| --- | --- | --- |
| 名册（跨项目） | 全局系统库 `project_info` | 名称 / 路径 / 状态 / `is_pinned` / `removed_at` / `last_opened_at` |
| 元数据（项目内） | 项目根 `.RSmeta/` | `project.db`（SQLite）、`analytics.duckdb`、`project.json`、`project.lock` |

目录名常量单一来源 `store::RS_META_DIR_NAME`。删除磁盘数据只删 `.RSmeta`，**用户文件保留**。

### 并发与破坏性操作都设了护栏

- **实例锁**用 OS 文件锁（进程退出 / 崩溃自动释放），`project.lock.owner` 记 pid 与获取时间供展示；「仍要打开」用于确认对方已退出后接管。
- **只读态两道防线**：写命令**可见但置灰**（卡片 `⋯` 菜单、标题栏项目菜单、设置面板写控件）；同时保留拦截层（`read_only_blocked` 写 notice）——禁用态是「点不动」，拦截是「点了也没用」。只读只禁写，不禁看 / 不禁走：设置面板、切项目、关闭项目始终可用。
- **未保存草稿拦截**：`request_open` / `request_close` / `request_create_project` / `request_open_folder` 统一进拦截对话框；确认后由 `PendingAction`（携带输入实体）**直达**目标对话框，不用回选择器再点一次。
- **不可逆操作**（删磁盘数据）要求输入项目名二次确认，且按钮用 `danger` 变体；设置、项目菜单、卡片 `⋯` 三处入口共用同一批动作函数。

### 视图与 model / service 同 crate

按 GPUI-kit 指南，视图不再留在 workbench：`ui.rs` 自带选择器 / 菜单内容 / 设置 / 对话框，外部依赖经 `ProjectUiHost` 注入——重绘桥（`ProjectUiNotifier`）、编辑区桥（`ProjectEditorBridge`）、排序持久化、打开后刷新。workbench 侧只剩 `components/project_host.rs` 做桥接，因此本 crate **不依赖 workbench 与 settings**。

配套的既有约定：

- 模态一律用语义组件：`Dialog` / `AlertDialog`；菜单一律 `Button::dropdown_menu` + `PopupMenu`（方向键导航 / Escape / 焦点恢复由组件负责，不自绘弹层）。
- 菜单「长什么样」与「点了做什么」分开：`project_menu_entries(read_only)` 是顺序 / 文案 / 图标 / 可用性的唯一权威来源（纯函数，逐项可断言），渲染只按 id 挂事件。
- 可点控件一律用语义 `Button`（自带 `track_focus` + `tab_stop`，Enter/Space 可激活）；需要键盘焦点的元素**不用会变的 `ElementId`**。
- 对话框按**栈语义**管理关闭时机（`on_ok` 返回 `false`，成功路径显式 `close_dialog`），避免被拦截动作另开对话框时 pop 错对象；校验错误走 `ProjectUiState::dialog_error` 槽。
- 「位置」字段 = 系统目录选择器（仅目录 / 单选）+ 目标路径预览，不用手抄路径。

### 状态可恢复、偏好可持久

- 软删（`removed_at`）+「已移除」Tab + 恢复入口；失效路径可「重新定位」，也可只移出列表。
- 名称与描述就地编辑（名册 + 项目本体双写，磁盘目录名不变）；描述同时出现在**卡片元信息**与设置·概览（空 / 纯空白不占位）；空目录打开时**询问式创建**。
- 设置·存储段是 `.RSmeta` **结构树**（目录在前 / 同级按名 / 深度优先，带大小与「复制路径」），只读排障用；「打开 `.RSmeta`」用系统文件管理器打开该目录（会暴露内部结构，已在界面注明）。
- 设置「默认项」记项目**默认连接**（U3）：写项目本体 `ProjectConfig.default_connection_id`（`.RSmeta/config/settings.json`），空值 = 不设默认；候选由宿主注入（全局 + 当前项目连接），记录值指向已删连接时如实显示 id。
- 固定置顶（`project_info.is_pinned`）与排序方式（`settings.projects.sort_mode`）跨会话保留；状态筛选（R4）为临时视图状态不落库。
- 内置「从示例项目开始」：无网络即可得到一个可用项目。

### 磁盘与名册快照：render 不做 I/O

设置面板要显示的东西（`.RSmeta` 目录遍历、名册里的描述与缺失驱动）全部在**事件路径**取一次，存 `ProjectUiState::settings`（`SettingsSnapshot`），render 只读快照。这与 crate 的既定约定一致：`render` 是纯读路径，副作用回事件路径。

### 明确的范围外

提升 / 引用（promote / snapshot）、移动或另存项目目录、DuckLake 远程项目（`ProjectPath::Remote` 仅模型层预留，UI 已明示范围）。

### 尚未开发（待办）

剩余项集中在 `docs/architecture/project/project-dev-plan.md` §8：卡片右键菜单（C3）、搜索 facet 语法（C5）、菜单快捷键（C2，组件无 shortcut 槽位，暂缓）。

## 代码落点

| 文件 | 职责 |
| --- | --- |
| `src/models.rs` | 域模型：`Project` / `ProjectInfo` / `ProjectPath` / `Versioned<T>` |
| `src/store.rs` | `.RSmeta` 创建 / 加载 / 迁移；`ProjectManager`；配置就地读写（`update_config`）；依赖自检 |
| `src/lock.rs` | 实例锁：OS 文件锁 + `project.lock.owner` 占用者信息 |
| `src/service.rs` | 编排：列表 / 创建 / 打开（含只读）/ 关闭 / 重命名 / 固定 / 归档 / 软删 / 恢复 / 硬删 / 移出 / 重定位 / 默认连接读写 / 版本台账 |
| `src/ui.rs` | 视图：选择器、标题栏菜单内容、项目设置、语义对话框、`ProjectUiHost` 宿主桥 |
| `src/ui/tests.rs` | 24 项 `ui` 测试（19 项 GPUI headless 窗口测试 + 5 项纯函数测试：选择器 / 状态筛选 / 设置快照与结构树 / 默认连接 / 对话框 / 拦截 / 排序 / 浏览目录 / 空目录询问 / 卡片与描述 / 菜单规格 / 只读拦截 / 键盘激活） |
| `tests/project_registry.rs` | 名册端到端集成：创建登记 → 固定置顶 → 软删隐藏（磁盘保留）→ 已移除找回（注入临时全局库） |
| `tests/project_store.rs` | 磁盘 `.RSmeta` / 实例锁 / 默认连接（U3）集成 |

## 文档

| 文档 | 内容 |
| --- | --- |
| `docs/architecture/project/README.md` | 模块入口（一句话定位 / 文档索引 / 特点速览） |
| `docs/architecture/project/project-prototype-design.md` | 交互语义与原型（决策表、场景清单、主题映射） |
| `docs/architecture/project/project-prototype.html` | 可交互原型（明暗双主题） |
| `docs/architecture/project/project-dev-plan.md` | 任务划分 / 进度记录 / 测试场景 / 风险 / 映射表 |
| `docs/architecture/project/project-view-architecture.md` | 宿主桥契约、对话框栈语义、窗口测试方案与坑 |
| `docs/architecture/project/project-user-guide.md` | 使用手册（入口 / 导览 / 流程 / FAQ / 验收清单） |

## 约定

```bash
cargo test -p rds-project -j 2          # lib（39，含 19 项窗口测试）+ 集成（1 名册 + 4 存储）
```

- 全量测试**必须** `cargo test --workspace -j 2`：并行链接重型 crate 会耗尽内存（DuckDB 已改动态链接）。
- 视图测试不要用 `use gpui_kit::*` / `use super::*` 通配导入（会让 `#[test]` 解析到 gpui 的 `test` 宏自身 → 无限递归）；细则见 `.agents/skills/gpui-kit-dev/SKILL.md`「窗口测试」。
- 依赖方向：`project → engine / shared / gpui-kit`，不得依赖 `workbench` 或 `settings`。
