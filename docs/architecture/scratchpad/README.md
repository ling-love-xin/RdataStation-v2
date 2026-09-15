# 草稿箱模块（M5）· 模块入口

> **一句话**：草稿箱是**单个项目私有的临时探索工作区**——面板根 = 项目下的可见目录 `{项目}/scratchpad/`，随项目迁移、不进任何数据库、不跨项目共享。它回答的是「**我正在做什么**」（M4 是"我能看到什么数据"，M6 是"我留下了什么"）。
>
> 本文只提炼**特点 / 边界 / 代码地图 / 硬约束**；细节指向本目录内文档，**不复制设计**。
> 状态：**面板与存储闭环已落地**（2026-09-15）——Phase A/B 全部完成，Phase C（编辑器联动）与 Phase D（提升为分析资源）待续。

## 1. 模块特点

### 产品行为

| 特点 | 含义 | 出处 |
| --- | --- | --- |
| **根 = 可见模块目录** | 草稿箱根是 `{项目}/scratchpad/`，用户可直接用系统文件管理器取走；「草稿」因此是**目录语义**而非视图过滤，不需要忽略规则、不扫描整个项目 | 架构 §0 决策 D1、§3.1 |
| **导入 ≠ 引用** | 导入 = **复制**进 `scratchpad/`（计体积、随项目迁移）；引用 = 只记路径 + 别名（不计体积、可能失效） | 架构 §6.6、原型 §2.2 |
| **失效必须可处理** | 引用目标被移走 → 置灰 + 「（丢失）」+ `⟲` 重新引用（只改路径，别名不动） | 架构 §6.6、§9 |
| **新建落点跟随选中** | 选中文件夹 → 内联行插在该文件夹首行（并自动展开）；未选中 → 模块根。粘贴同规则 | 架构 §6.3 |
| **删除先进回收站** | 删除 = 移入**项目级**回收站，底部撤销栏 5 s 自动消失；回收站可逐条还原、可清空 | 架构 §4.2、原型 §4.4 |
| **重结果落中央编辑区** | 内容搜索的命中列表与替换栏由**中央编辑区**渲染，侧栏只承载输入与开关（侧栏 240 px 装不下结果） | 架构 §0 决策 D7、§6.7–6.8 |
| **模板起步** | 新建文件可选 空白/SQL/Python/Markdown/JSON，自动补后缀 + 填充占位内容 | 架构 §6.3、原型 §4.1 |

### 语义与数据

| 特点 | 含义 | 出处 |
| --- | --- | --- |
| **内容与内部态分离** | 内容在可见目录 `scratchpad/`；内部态（外部引用 + 文件元数据）在 `{项目}/.RSmeta/scratchpad/config.json`，面板永不需要过滤 | 架构 §3 |
| **回收站为项目级** | `.RSmeta/trash/<id>/{payload, manifest.json}`，条目自带 `origin` + `original_rel_path`，草稿与将来的资源删除共用；草稿箱只还原 `origin = "scratchpad"` 的条目 | 架构 §4 |
| **元数据只存 ID / 路径** | `file_meta` 记 `last_connection_id` / `last_executed_at` / `bound_connections`（**均为连接 ID**），凭据始终在 `auth_store` | 架构 §3.2 |
| **路径防护** | `resolve_path` 拒绝 `..` 穿越与点前缀（`.RSmeta` 等）路径；列表跳过点前缀条目 | 架构 §5 |
| **旧布局一次性迁移** | 旧 `{项目}/.scratchpad/` → 新布局（配置进 `.RSmeta/scratchpad/`、内容进 `scratchpad/`、旧回收站并入项目级回收站）；幂等、非破坏、同名不覆盖 | 架构 §3.3 |
| **惰性加载 + 虚拟化** | 树只取模块根，展开时按目录拉取；渲染走 `v_virtual_list`（只画可视区行）；`MAX_DEPTH=4` 只约束搜索/递归复制 | 架构 §6.2 |

### 架构与约束

| 特点 | 含义 | 出处 |
| --- | --- | --- |
| **依赖只向下** | `scratchpad → shared`；**crate 内不含 gpui**，视图全部在 `workbench` | 架构 §7.1 |
| **render 是纯读路径** | 渲染期不做 I/O：加载与重操作（导入/粘贴/清空回收站/搜索/替换）均走 `services/scratchpad_jobs.rs` 工作线程 + 轮询回填；仅元数据级操作保持同步（架构 K1c） | 架构 §6.1、§13.1 |
| **双击打开到编辑器** | 行双击 / `Enter` / 右键「打开」→ 中央编辑器（同路径已打开只激活，不重读）；模式与只读等级由编辑器按路径判定 | 架构 §6.12 |
| **外部改动自动刷新** | 监听模块目录（`notify`），1.2 s 去抖后重拉列表，并给已打开的结果面板重跑一次搜索；监控不可用则降级为手动 `↻` | 架构 §6.11 |
| **窗口 = 项目** | 项目态**不得放进程单例**：workbench 由窗口的 `Shared::project` 按需构造 `ScratchpadStore`；`ScratchpadState`（长生命周期 watcher 场景）接入时**必须按窗口持有** | 架构 §7.2 |
| **两道护栏** | 同项目二次打开由 `project` crate 的 `ProjectLock` 拦截（只读/仍要打开/取消）；只读打开时草稿箱全面禁写并给状态栏提示 | 架构 §7.3 |
| **不做 multi-root** | 多根会把会话 / 监控 / 文件元数据的复杂度抬高一个量级，与本模块「应用实例即项目」的定位不符 | 架构 §7.4 |
| **树不承担编辑态** | 脏点、冲突 Diff、多文件 Tab 属编辑器宿主（Phase C）；树只发 `OpenFile(绝对路径)` 语义 | 原型 §9.4 |
| **零裸值 / 组件不手搓** | 颜色一律 `cx.theme()`（含产品 token `search.match.background`）；尺寸进 `crates/workbench/src/ui.rs`；列表/按钮/菜单用 gpui-kit 组件 | 原型 §6–§7 |

### 工程与文档

| 特点 | 含义 | 出处 |
| --- | --- | --- |
| **业务在 crate、视图在 workbench** | 递归复制 / 搜索 / 替换 / 引用重定位等语义都在 `crates/scratchpad`，可单测（14 项基线）；workbench 只做编排与渲染 | 架构 §7.1 |
| **命令要限并发** | 并发链接 DuckDB 静态库会耗尽内存：`cargo check/test` 必须带 `-j 2` | 本文 §5 |
| **设计先于实现** | 本轮补模块入口 + 架构 + 使用手册；原型/开发方案的进度记录逐轮追加 | 本文 §6 |

## 2. 边界

- 本模块拥有：**草稿文件读写 / 导入 / 外部引用 / 项目级回收站入口 / 内容搜索与替换 / 文件元数据（连接绑定）**。
- 不属于本模块：
  - **中央编辑区**（打开、编辑、`Ctrl+S` 回存、脏点、冲突 Diff、多文件 Tab）→ `editor`（Phase C 接线）；
  - **归档与取回**（提升为分析资源、只读存档、版本）→ `analytics_resource`（Phase D，经 command/event 协作）；
  - **数据源连接与内省** → `database` / `connection`（草稿只保存连接 **ID**）；
  - **项目 CRUD / 锁 / 回收站基础设施** → `project`（回收站由 `project` 定义位置与清单格式，草稿箱只是使用方）。
- **不做**：跨项目共享草稿、multi-root 工作区、在草稿箱内做数据计算（计算属 `engine`/DuckDB）。

## 3. 代码地图

| 位置 | 职责（括号内为现状） |
| --- | --- |
| `crates/scratchpad/src/watch.rs` | 模块目录文件监控：`ScratchpadWatcher`（`notify` 递归监听）+ `ChangeFlag`（外部改动 → 变更标记） |
| `crates/scratchpad/src/store.rs` | `ScratchpadStore`：根/元数据路径、`ensure_dir`、旧布局迁移、列表（懒加载按目录）、CRUD、`move_entry`/`copy_entry`（递归）、内容搜索（子串/正则 + 大小写，带命中区间）、`replace_in_file`、`diff_with_content`、导入、外部引用 CRUD + 可用性探测 + 重定位、`file_meta` 绑定、路径防护 |
| `crates/scratchpad/src/trash.rs` | `ProjectTrash` / `TrashManifest` / `TrashEntry`：项目级回收站（来源 + 原相对路径 + 还原/清除） |
| `crates/scratchpad/src/models.rs` | 域模型：`ScratchpadEntry` / `SearchMatch`（含 `match_spans`）/ `ExternalReference(Status)` / `FileMeta` / `AnalyzableFile` / `ReplaceResult` / `DiffResult` 等 |
| `crates/scratchpad/src/state.rs` | `ScratchpadState`：按项目初始化 store + watcher 标志（**当前无生产调用方**，接入时必须按窗口持有） |
| `crates/workbench/src/services/scratchpad_jobs.rs` | 草稿箱后台任务：单工作线程 + tokio 运行时执行加载与重操作（导入/粘贴/清空回收站/搜索/替换），结果队列 + 请求序号防过期 |
| `crates/workbench/src/panels.rs`（`SidebarPanel`） | 草稿箱面板：`render_scratchpad`（工具栏 / 搜索 / 树 / 引用 / 回收站 / 撤销栏 / 状态行）、`scratchpad_row`、`render_scratchpad_edit_row`、`render_scratchpad_empty_state`、`request_scratchpad_load` / `ensure_scratchpad_pump`、`ensure_scratchpad_watch`（外部改动监控）、剪贴板与多选、键盘导航 |
| `crates/workbench/src/panels.rs`（`EditorPanel`） | 内容搜索结果面板与替换栏：`render_scratchpad_search_pane`、`replace_scratchpad_all` |
| `crates/workbench/src/{commands,ui}.rs` | `scratchpad` key context 动作；`SCRATCHPAD_GROUP_MAX_HEIGHT` / `SCRATCHPAD_EMPTY_ICON_SIZE` 等尺寸常量 |
| `crates/workbench/src/view.rs`（`WorkbenchView`） | 宿主：消费 `Shared::open_file_request` → `open_in_editor`（把草稿打开到中央编辑器，同路径只激活） |
| `crates/app/src/main.rs` | 快捷键绑定（`ctrl-a` / `f2` / `delete` / `escape` / `↑↓` / `enter` / `ctrl-n`，context = `scratchpad`） |
| `crates/scratchpad/README.md` | crate 入口（特点提炼，不复述设计） |

## 4. 改这个模块前必须遵守

1. **依赖方向**：`scratchpad` 不得依赖 gpui / workbench；workbench 通过 workspace 依赖使用它。
2. **零裸 hex / 零裸 px**：颜色走 `cx.theme()`（含产品 token）；尺寸先进 `ui.rs` 再引用。
3. **业务逻辑不进视图**：可单测的语义（复制、搜索、替换、路径防护）放 crate；视图只编排与渲染。
4. **`render` 是纯读路径**（已落实于加载路径）：I/O 与 `Shared` 写入放事件路径或后台任务（`services/scratchpad_jobs.rs` 模式），不要在 render 里 `block_on` / 读盘。
5. **`cx.theme()` 借用**：同一函数里既要 theme 又要 `cx` 可变借用时，把可变操作放在 `let theme = cx.theme();` **之前**。
6. **重复元素的 `ElementId` 用业务键**（条目相对路径），不要用下标。
7. **只读项目**（`Shared.project_ui.read_only`）下所有写操作必须拒绝并给出提示。
8. **路径安全**：任何新建/删除/复制都必须走 `resolve_path` / `validate_name` 校验，禁止绕过。
9. **不做进程级单例**：项目态只挂在窗口的 `Shared` / 面板实体上。
10. **改完必须更新**：`crates/scratchpad/README.md` 能力表 + `scratchpad-dev-plan.md` 进度记录。

## 5. 测试与验证

```bash
# 类型检查（必须 -j 2：并发链接 DuckDB 静态库会耗尽内存）
env RUSTC="<toolchain>/bin/rustc.exe" "<toolchain>/bin/cargo.exe" \
  check -p rds-scratchpad -p rds-workbench -p rds-app --all-targets -j 2

# 后端单测（14 项基线：模块根隔离 / 路径防护 / 回收站来源 / 跨模块还原拒绝 /
# 旧布局迁移 / 引用状态与重定位 / 绑定往返 / 搜索区间 / 正则大小写 / 递归复制 / 替换）
env RUSTC="<toolchain>/bin/rustc.exe" "<toolchain>/bin/cargo.exe" test -p rds-scratchpad -j 2
```

- 单测**不覆盖**视图：面板交互（多选、剪贴板、模板、虚拟列表、替换栏）目前靠人工验收，见 `scratchpad-user-guide.md` §9。
- 已知环境限制：本机 `rustdoc` 执行报 Windows `os error 448`（doctest 阶段），与本模块无关。

## 6. 文档地图

| 文档 | 作用 |
| --- | --- |
| `README.md`（本文） | 模块入口：**从哪开始读** |
| `scratchpad-architecture.md` | **为什么这样设计 / 怎么运转**：不变式 / 概念模型 / 存储布局 / 回收站与元数据 / 数据流 / 决策表 / 降级 / 测试策略 / 已知问题（权威） |
| `scratchpad-prototype-design.md` | **长什么样**：面板布局 / 树与分组 / 交互规格 / 主题映射 / 落点映射 |
| `scratchpad-prototype.html` | 可交互原型（RDS Light/Dark，可切主题） |
| `scratchpad-dev-plan.md` | **做什么、做到哪**：Phase 划分 / 逐轮进度记录 / 测试场景 / 风险 / 映射 |
| `scratchpad-user-guide.md` | **怎么用**：入口 / 界面导览 / 典型流程 / 快捷键 / FAQ / 验收清单 |
| [`../../../crates/scratchpad/README.md`](../../../crates/scratchpad/README.md) | crate 级特点提炼（不含设计细节） |

## 7. 下一步

| 项 | 归属 | 说明 |
| --- | --- | --- |
| 双击打开 → 编辑器草稿模式（`Ctrl+S` 回存、`file_meta.last_connection_id` 恢复、脏点、冲突 Diff） | Phase C（依赖 `editor`） | 见 `scratchpad-dev-plan.md` §2 Phase C |
| 拖放（外部文件导入 / 树节点拖入编辑区） | Phase C | 同上 |
| 文件监控（`notify`）：外部修改/删除感知 | Phase A 余项 | `state.rs` 的 `watcher_active` 已预留 |
| 提升 / 存档只读 / 取回 / 版本；资源侧回收站统一展示 | Phase D（依赖 `analytics_resource`） | 架构 §9.3 |
| 引用目录可展开浏览（当前只作入口） | 待拍板 | 依赖「引用目录是否计入树」的产品决定 |
