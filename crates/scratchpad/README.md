# rds-scratchpad — M5 草稿箱（项目工作区）

> 本文件是 crate 的 **README 级入口**：只提炼模块特点与代码结构，完整设计以 `docs/architecture/scratchpad/` 为准（架构约定：crate 内不复制设计文档）。

## 一句话定位

草稿箱是**单个项目私有**的临时探索工作区：面板根 = 项目下的模块目录，内容跟随项目走，不进 `project.db`/`global.db`，**不跨项目共享**。

## 模块特点

### 1. 根 = 模块目录（不是项目根，也不是隐藏目录）

```
{项目}/
├── scratchpad/                  ← 草稿箱根（模块内容，可见、可编辑）
├── resources/                   ← M6 分析资源（后续；提升目标）
├── mock/                        ← M7 Mock 产物（后续）
└── .RSmeta/
    ├── scratchpad/config.json   ← 模块内部态（外部引用 + file_meta）
    └── trash/                   ← 项目级回收站（草稿 + 资源共用）
```

- 内容在可见目录里，用户可直接用系统文件管理器取走；**内部态全部在 `.RSmeta/`**，面板永不用过滤。
- 「草稿」因此是一个**目录语义**而非视图过滤：在 `scratchpad/` 里的就是草稿。不需要忽略规则、不需要扫描整个项目。
- 旧布局 `{项目}/.scratchpad/` 一次性迁移（配置 → `.RSmeta/scratchpad/`、内容 → `scratchpad/`、旧回收站 → 项目级回收站）；幂等、非破坏、同名不覆盖。

### 2. 回收站是项目级的，且自包含来源

- 位置 `.RSmeta/trash/`；条目为 `trash/<id>/{payload, manifest.json}`。
- `manifest` 记录 `origin`（来源模块）+ `original_rel_path`（原相对路径）+ `kind/size/deleted_at`，因此还原能回到原模块、原路径（同名自动改名，不覆盖）。
- 草稿箱的还原入口**只接受 `origin == "scratchpad"`**，其他来源报错提示在其模块中还原。
- API 按模块无关设计：待 M6 落地后若确认为稳定共用能力，再按「≥2 使用方」规则上提到 `shared`/`engine`。

### 3. 隔离边界：窗口 = 项目

- 项目态**不得放进程单例**：workbench 由窗口的 `Shared::project` 按需构造 `ScratchpadStore`（无进程级缓存）。`ScratchpadState` 保留给需要长生命周期 watcher/缓存 的场景，**接入时必须按窗口持有**。
- 同一项目根二次打开由 `project` crate 的 **`ProjectLock`** 兜底（已接入项目打开流程：`{.RSmeta}/project.lock`，占用时弹「只读 / 仍要打开 / 取消」）。
- **只读模式联动**：项目以只读打开时（`Shared.project_ui.read_only`），草稿箱禁止新建 / 重命名 / 删除 / 粘贴，并在状态栏给出提示。
- 不引入 multi-root：它是 Zed 的能力，但会把会话/监控/文件元数据的复杂度抬高一个量级，与本模块「应用实例即项目」的定位不匹配。

### 4. 与编辑器、资源模块的边界

- **树只做导航与文件操作**，不承担编辑态：脏点、冲突、Diff 属编辑器宿主（替换已落地：结果栏逐文件写回，不动编辑器状态）。
- 树发出 `OpenFile(绝对路径)` 语义（编辑器无根，按路径所属模块决定只读/可编辑）；重结果（内容搜索、Diff、替换）落**中央编辑区**，侧栏只承载输入与摘要。
- 内容搜索结果经 `Shared::scratchpad_search` 交给中央编辑区渲染；侧栏不自己画结果。
- 提升为分析资源 = **存档（move + 只读锁定）**，要改只能取回（检出）；属 Phase D，经 command/event 协作，不直接依赖 `analytics_resource` 视图。

### 5. 元数据：只存 ID / 路径，绝不存凭据

- `config.json` 的 `files` 键 = **相对模块根的路径**（项目迁移不失效），记录 `last_connection_id` / `last_executed_at` / `bound_connections`（均为连接 ID）。
- 凭据始终在 `auth_store`（AES-256-GCM）；文件元数据不落明文。
- 外部引用（链接）：项目外的目标只能存**绝对路径** + 别名，**不复制**；加载时探测可用性，失效项置灰并提示「丢失」；可改名 / 在文件管理器中打开 / 重新引用（`update_external_reference_path`，只改路径） / 移除。
  - 与**导入**的本质差异：导入是**复制**进 `scratchpad/`（计体积、随项目迁移）；引用只记路径（不计体积、可能失效）。
- 路径安全：`resolve_path` 拒绝 `..` 穿越与点前缀（`.RSmeta` 等）内部路径；列表隐藏点前缀条目。

## 代码结构

| 文件 | 职责 |
| --- | --- |
| `src/models.rs` | 域模型：`ScratchpadEntry` / `FileMeta`（+ `preferred_connection()`：显式绑定优先、其次最近执行） / `ExternalReference` / `ExternalReferenceStatus` / 搜索 / Diff / 替换 |
| `src/store.rs` | `ScratchpadStore`：列表（懒加载按目录）、CRUD、**递归复制**、回收站入口、内容搜索（子串/正则/大小写 + **命中区间**）、替换（正则/字面量 + 大小写）、Diff、外部引用（含**重新定位**）、可分析文件、路径防护（含反向的 `relative_path_of`）、旧布局迁移、文件元数据读写（`file_meta` / `bind_connections` / `update_file_meta`） |
| `src/trash.rs` | `ProjectTrash`：项目级回收站（`TrashManifest` / `TrashEntry`，含来源与原路径） |
| `src/watch.rs` | `ScratchpadWatcher` / `ChangeFlag`：模块目录文件监控（**外部改动 → 变更标记**，视图侧去抖重拉；只监听内容目录，`.RSmeta` 不在范围内） |
| `src/state.rs` | `ScratchpadState`：按项目初始化 store + watcher 标志（**当前无生产调用方**；监控器已自持生命周期，接入时须按窗口持有） |
| `src/jobs.rs` | 后台任务：单工作线程 + tokio 运行时执行加载与重操作（导入 / 粘贴 / 清空回收站 / 搜索 / 替换 / **冲突 Diff**），结果队列 + 请求序号防过期（视图只入队 / 轮询 / 回填） |
| `src/scratchpad_view.rs` | 面板视图（`ScratchpadView`）：工具栏 / 搜索 / 树 / 引用 / 回收站 / 撤销栏 / 状态行、剪贴板与多选、键盘导航、脏点回显、**冲突条**；另提供中央编辑区的两套只读投影（搜索结果面板 / 冲突 Diff 面板）；含纯函数辅助与其单测 |
| `src/host.rs` | `ScratchpadHost` 端口（**本 crate 定义、宿主实现**）：项目根 / 只读判定 / 状态栏提示 / 宿主重绘 / 搜索结果投递 / 在编辑器打开文件 / 脏文档集合（`dirty_files`）/ 缓冲区读写（`draft_content` / `reload_draft`）/ 冲突 Diff 投递（`show_diff`） |
| `src/commands.rs` | 面板键盘动作（`Scratchpad*` 系列 Action） |

- 依赖方向：`scratchpad → workbench_shell / gpui-kit / shared`（**不依赖 workbench**；宿主能力经 `ScratchpadHost` 端口注入，实现在 `workbench/src/components/scratchpad_host.rs`）。模板：`[dev-dependencies]` 必须打开 `paths/test-support`（测试数据根隔离，见 `docs/architecture/runtime/data-paths.md` §5）。

## 能力状态

| 已实现 | 待补 |
| --- | --- |
| 模块根 + 内部态隔离 + 旧布局迁移 | 拖放导入 / 拖入编辑器（Phase C） |
| 项目级回收站（来源/原路径/还原/清空） | 拖放导入 / 拖入编辑器（Phase C） |
| 列表/新建（含模板）/重命名/删除/移动/复制（文件与**文件夹递归**） | — |
| 打开草稿到编辑器 + 按 `file_meta` 预选连接 + **执行后回写**最近连接 | — |
| **冲突 Diff**（外部改动 + 编辑器有未保存修改 → 冲突条：差异 / 重载 / 忽略；差异行级落中央编辑区） | — |
| 懒加载、排序（名称/大小/时间）、文件名过滤、**虚拟列表**（只渲染可视区）、空态大图标 + 按钮 | 提升/存档只读/取回/版本（Phase D，依赖 `analytics_resource`） |
| 内容搜索（正则/大小写）+ **命中高亮** + **全部替换**（结果与替换栏落中央编辑区） | 点击命中跳转文件（依赖编辑器打开） |
| 外部引用（添加文件或目录 / 自定义别名 / 改名 / 打开 / 移除 / 可用性探测 / **失效后重新引用**）、文件元数据（数据源绑定：读 `file_meta` + 写 `bind_connections` / `update_file_meta`） | 引用目录的展开浏览（当前只作入口） |
| **文件监控**（`notify` 递归监听模块根 + 1.2 s 去抖重拉；启动失败降级为手动 `↻`） | 已打开搜索结果的自动重搜（外部改动只重拉树，不重跑搜索） |
| 面板侧：多选（Ctrl/Shift/Ctrl+A）、剪切/复制/粘贴、撤销栏（5s 自动消失）、右键菜单、F2/Del/Esc/↑↓/Enter/Ctrl+N、导入、打开所在位置、**脏点**（编辑器未保存修改）、只读模式禁写 | Phase D 回收站与资源删除的合并展示 |

> 连接绑定的**回写**发生在宿主：`crates/workbench/src/services/scratchpad_meta.rs`（编辑器回执 → 只回写模块内草稿，见 `docs/architecture/scratchpad/scratchpad-architecture.md` §6.13）。

## 设计与验证

- 设计（权威）：`docs/architecture/scratchpad/` 五件套——`README.md`（模块入口）· `scratchpad-architecture.md`（设计理念与架构 + 已知问题）· `scratchpad-prototype-design.md` + `scratchpad-prototype.html`（原型）· `scratchpad-dev-plan.md`（进度）· `scratchpad-user-guide.md`（使用手册）。
- 验证：`cargo check -p rds-scratchpad -j 2`；单测 `cargo test -p rds-scratchpad -j 2 --lib`（**36 项**：域逻辑 + 面板纯函数 + 后台任务）。
- **命令约定**：全量编译/测试必须限制并发（`cargo check-all` / `cargo test-all` 别名，含 `-j 2` 与 `RUST_MIN_STACK`）——并发链接重型 crate 会耗尽内存（DuckDB 已改动态链接）。
