# 数据源连接模块 · 设计架构与数据流（M3）

> 状态：Phase A/B + Phase C（C1–C3、C5–C18）已实现；C4（模板导入导出）**能力就绪、UI 入口暂缓** ；已知缺口见 §14 · 2026-09-12
> 关联：`connection-prototype-design.md`（原型设计，含 §2.2 暂存列表规格）、`connection-dialog-prototype.html`（交互稿）、`connection-dev-plan.md`（开发方案与逐轮记录）、`../../theme/ui-constraints.md`（尺寸与控件约束）
> 读者：参与本模块维护 / 扩展的开发者；也用于排查「入口没反应」「保存后列表没刷新」这类跨层问题

---

## 1. 定位与边界

| 维度 | 结论 |
| --- | --- |
| 负责 | 连接的**新增 / 编辑 / 删除 / 测试**、作用域路由（G_/P_/GP_）、DuckDB Secret 加速、运行态连接与隧道、连接组织元数据（标签 / 分组）的读写入口 |
| 不负责 | 数据源**导航树与对象浏览**（属 database-nav）、元数据内省实现（属 database crate + engine 缓存）、分析资产 |
| 核心交互载体 | 「新建 / 编辑数据源连接」模态对话框（一个对话框内可连续编辑多个连接） |
| 关键约束 | 视图层零裸色值（`cx.theme()`）、I/O 不在 render 中、`connection` crate 不得依赖 `engine`（依赖方向见 §2.1） |

---

## 2. 分层架构

### 2.1 依赖方向（硬约束）

```mermaid
flowchart TB
    APP["app（App Shell）"]
    WB["workbench（视图 + 服务编排）"]
    CONN["connection（传输层：URL / 协议链 / 连接器 / Secret）"]
    ENG["engine（驱动 / 持久化 / 缓存 / 迁移）"]
    SHARED["shared（错误 / 加密 / 基础类型）"]
    OTHERS["project / database / insight / mock / settings / scratchpad"]

    APP --> WB
    APP --> OTHERS
    WB --> CONN
    WB --> ENG
    WB --> OTHERS
    ENG --> CONN
    ENG --> SHARED
    CONN --> SHARED
```

> 规则：**Feature crates → engine, shared, gpui-kit**；`connection` 只允许依赖 `shared`。
> 因此「连接服务」拆成了两半：不依赖 engine 的传输/协议层下沉 `connection`；依赖 engine（持久化、驱动路由）的编排层留在 `workbench`。这就是 C3 分批收敛的由来。

### 2.2 文件落点

| 层 | 文件 | 职责 |
| --- | --- | --- |
| 视图 | `crates/workbench/src/components/connection_dialog/` | 对话框（五 Tab / Header / 侧栏暂存列表 / 三管理器覆盖层）——按职责拆分： |
| 视图 | ├ `mod.rs` | 模块声明与 re-export、常量、`ConnectionDialogState` 字段、输入控件工厂 |
| 视图 | ├ `state.rs` | 构造 / 元数据刷新 / 编辑回读 / `ClonedDialogState`（保存按钮的轻量视图） |
| 视图 | ├ `staging.rs` | `ConnectionDraft` / `Hop`、暂存状态机、持久化映射（`draft_to_row` / `row_to_draft`）、来源短码 |
| 视图 | ├ `render.rs` | `open()`：对话框元素树（Header / Tab / 侧栏 / footer / 快捷键 handler） |
| 视图 | ├ `managers.rs` | 三管理器覆盖层（认证 / 网络 / 环境）CRUD |
| 视图 | └ `helpers.rs` | 图标、Select 赋值、原型卡片/只读行、URL 重拼、作用域标签、尺寸常量（`GAP_*` / `LABEL_W` / `BADGE_*` / `PROJECT_W` / `DRIVER_W` / `TAB_BODY_H` / `STAGING_H` / `ROW_H`） |
| 视图 | └ `project_picker.rs` | 项目下拉项 `ProjectItem`（项目名 + 路径双列、搜索、末项「＋ 新增项目」） |
| 视图 | `crates/workbench/src/panels.rs`（`EditorPanel` / `Shared`） | 入口（`request_new_connection` / `request_edit_connection`）、项目下拉订阅（`ensure_dialog_subscription`）、宿主重绘桥、面板级共享状态（含 `project_new_request`） |
| 视图 | `crates/workbench/src/view.rs`（`WorkbenchView`） | 对话框层挂载（`Root::render_dialog_layer`）、观察编辑面板（通知级联） |
| 服务 | `crates/workbench/src/services/data_source_service.rs` | CRUD / 测试连接 / 同名检查 / 作用域路由 / 摘要解析（`parse_url_host_port_db`） |
| 服务 | `crates/workbench/src/services/workspace_loader.rs` | 列表可见性合并（全局 + 项目侧）、运行态 `connected` 填充、删除路由 |
| 服务 | `crates/workbench/src/services/connection_service.rs` | 会话生命周期（connect / close / 列表 / 元数据路径）、隧道登记与回收 |
| 服务 | `crates/workbench/src/services/secret_integration.rs` | DuckDB Secret 注册 / 注销（按 `use_duckdb_fed` 门控） |
| 传输 | `crates/connection/src/{url,url_params,chain,config,connector,factory,stream,known_hosts,secret,model}.rs` | URL 组装与脱敏、参数注入、协议链执行 + `TunnelRegistry`、连接器与流、Secret 语句 |
| 持久 | `crates/engine/src/persistence/{global_db,project_db,connection_store,project_connection_store,connection_org_store,connection_draft_store,driver_store,auth_store,network_store,env_store}.rs` | 全局库 / 项目库 / 最近连接 / 组织元数据 / **暂存草稿** / 驱动与类型目录 / 三类引用配置 |
| 持久 | `crates/engine/migrations/{global,project_meta}/*.sql` | 表结构演进（暂存草稿 = `global/020_add_connection_drafts.sql`） |
| 持久 | `crates/engine/src/migration/*` | 全局系统库初始化（`initialize_global_system` / `install_global_db_manager`）、DuckDB 迁移执行器 |

---

## 3. 概念模型

### 3.1 两层目录：数据库类型 vs 驱动实现

这是最容易误解的一组概念（曾出现「驱动类型下拉绑到了数据库类型」的实现错误）：

| 概念 | 表 | 例子 | 出现位置 |
| --- | --- | --- | --- |
| **数据库类型**（`DataSourceType`） | `data_source_types` | MySQL / PostgreSQL / SQLite / DuckDB / MariaDB / Oracle / ClickHouse…（按 `category`：relational / file-based / analytics / nosql） | 对话框**侧栏分类树**（行首显示类型 `icon` emoji） |
| **驱动实现**（`Driver`） | `drivers` | `MySQL (sqlx)` / `MySQL (Official)` / `SQLite (rusqlite)` / `DuckDB (duckdb-rs)` | 对话框 **Header「驱动」下拉**（只显示**实现短名**，如 `sqlx`） |

- 一个数据库类型下可有多个驱动实现（`drivers.type_id → data_source_types.id`）。
- **UI 分工（2026-09-11 去噪）**：类型只在左侧栏选（暂存条目最左侧显示缩小的类型 UI = 类型 `icon`）；右侧驱动下拉**不再重复类型名**，仅列当前类型的启用驱动并显示实现短名（`driver_short_name`：取 `name` 括号内部分，无括号时原值）。同一类型下短名唯一，跨类型同名（如两个 `Official`）不冲突——下拉选项按类型过滤。
- **落库语义**：连接记录的 `db_type` 与 `driver_id` 都存**驱动 id**（`drivers.id`，如 `mysql_native`）——与之对齐的是引擎运行时 `DriverRegistry` 的 key（`create_database(db_type)` → `DataSourceRouter::route` → registry），选错驱动即连不上。
- 文件型判定用驱动元数据 `drivers.is_file`（不再按名字硬编码）。
- 侧栏点击类型 → 自动选中该类型下第一个启用驱动（Header 联动）；草稿同时记录 `type_id` 与 `driver_id`（恢复时先定类型再定驱动）。

### 3.2 连接记录与作用域（G_/P_/GP_）

| 作用域 | ID 前缀 | 落库位置 | 语义 |
| --- | --- | --- | --- |
| 仅全局 | `G_` | `global.db` 的 `global_connections` | 系统级，所有项目可见 |
| 仅项目 | `P_` | 当前项目 `.RSmeta/project.db` 的 `connections` | 项目私有 |
| 全局 + 项目 | `G_` 定义 + `GP_` 快照 | 全局库 + 项目库各一条 | 项目引用全局快照；规则：项目可引用全局，全局不引用项目私有 |

> `GP_` 是**独立副本**（非活链接）：全局定义后续修改不会自动跟随；项目侧可用
> 「从全局定义同步」（对话框 footer，仅编辑 GP_ 时显示 → `DataSourceService::sync_snapshot_from_global`）
> 显式拉取最新配置与凭据密文；同步保留快照 ID / 创建时间 / 分组关系。

`update` 按 ID 前缀路由（`id_prefix::{is_project, is_snapshot}`）；`save` 按 UI 作用域选择落库；项目作用域必须提供项目根（未打开项目时由当前项目会话预填，可手改）。

### 3.3 复用引用（认证 / 网络 / 环境）

三类「可复用配置」贯穿连接配置：`auth_configs`（凭据，AES-256-GCM）、`network_configs`（协议链档案）、`environments`（含 5 类策略）。对话框内为「下拉引用 + 管理入口（覆盖层 CRUD）」；选中引用后字段只读（改档案一处全量生效）。

### 3.4 DuckDB Secret 本地加速

- 开启 `use_duckdb_fed` 的连接，保存后把源库凭据注册为 DuckDB `PERSISTENT SECRET`（`SET secret_directory` 指向应用目录，**不写用户主目录**；名称统一小写以匹配 DuckDB 标识符折叠）。
- 关闭开关时清理对应 Secret；Secret 注册失败只告警，不影响连接本身（加速是增强而非依赖）。

### 3.5 暂存草稿（`ConnectionDraft`）

一个对话框内可同时存在多个「正在编辑的连接」：

| 类型 | 特征 | 说明 |
| --- | --- | --- |
| **草稿（Draft）** | `saved_id = None` | 全部字段为内存快照；`✕` 可删除；保存成功后转正式；当前条目有未写回修改时名称旁显示`●` |
| **已保存（Saved）** | `saved_id = Some(id)` | 打开对话框时由 `DataSourceService::list()` 合并生成；点击走 `load_for_edit` 回读；**不在暂存列表删除**（删除入口归导航栏）；名称旁显示来源短码 `P/G/GP` |

- 快照字段 = Header + 五 Tab 的全部可编辑状态（`type_id` 数据库类型、`driver_id` 驱动 id、`driver_name` 实现短名、URL、凭据、作用域与项目路径、SSL、协议链 `Vec<Hop>`、驱动属性、策略覆盖、三类引用）。
- **内存 + 跨会话持久化**：关闭对话框不丢失（状态挂在 `EditorPanel` 的对话框句柄上）；变更与关闭时写入 global.db 的 `connection_drafts` 表（迁移 `020`），重启后首次打开自动恢复。
- **凭据安全边界**：持久化表**不含密码列**（`ConnectionDraftRow` 无 password 字段，恢复后密码框为空），只随正式保存写入连接库（AES-256-GCM）。

---

## 4. 状态所有权与生命周期

### 4.1 状态地图

| 状态 | 存放 | 生命周期 | 谁写 | 谁读 |
| --- | --- | --- | --- | --- |
| 连接列表 / 选中 / 通知文案 | `Shared`（`Rc`） | 应用 | `workspace_loader`、对话框保存 / 删除后 | 侧栏、编辑区、状态栏 |
| 宿主重绘桥 `host_redraw` | `Shared`（`Rc<dyn Fn(&mut App)>`） | 应用 | `WorkbenchView::new` 注入 | 对话框打开 / 关闭、暂存操作 |
| 对话框状态（含草稿列表） | `EditorPanel.dialog: Option<Rc<ConnectionDialogState>>` | 首次打开 → 应用退出 | 对话框自身 | 对话框渲染、侧栏回调 |
| 草稿列表 / 光标 | `ConnectionDialogState.{drafts, draft_cursor}` | 同上 | `staging_*` 方法 | 暂存列表渲染 |
| 三类引用 / 驱动目录 | `ConnectionDialogState.{auth_list, network_list, env_list, types, drivers}` | 对话框打开时刷新 | `refresh_meta` | 各 Tab / 侧栏 |
| 运行时连接 / 隧道守卫 | `ConnectionService`（`ConnectionManager` + `TunnelRegistry`） | 连接建立 → 断开 | `connect_with_type` / `close_connection` | 状态点、元数据服务 |

### 4.2 对话框层挂载与宿主重绘（关键机制）

gpui 的 `cx.notify()` **只重渲染该视图子树**；而 `Root::open_dialog` 只通知 `Root`。对话框层挂在 `WorkbenchView::render` 中，因此：

```mermaid
flowchart TB
    A["EditorPanel 按钮 / 侧栏编辑入口"] --> B["dialog.open(...)"]
    B --> C["Root::open_dialog（层状态入 Root）"]
    B --> D["Shared::notify_host（宿主重绘桥）"]
    D --> E["WorkbenchView::render 重跑"]
    E --> F["Root::render_dialog_layer 取到活动层"]
    F --> G["层真正进入元素树（可见）"]
    E --> H["cx.observe(editor) 级联：对话框内部状态刷新"]
```

- **打开 / 关闭**都必须通知宿主：打开漏通知 → 点了没反应；关闭漏通知 → 层残留。
- 对话框**内部**状态刷新（切 Tab、增删协议链跳、暂存切换、测试结果）走 `EditorPanel` 的 notify，由 `WorkbenchView` 的 `cx.observe` 级联到宿主。
- `WorkbenchView` 的事件回调（如侧边栏「编辑」）本身处于宿主 update 上下文，**不能**回调 `notify_host`（借用重入），依赖该回调末尾既有的 `cx.notify()`。

### 4.3 对话框打开 / 关闭语义

| 时机 | 行为 |
| --- | --- |
| 打开（新建） | `open(none)`：重入保护（先 `close_dialog`）→ 刷新引用 / 驱动目录 → 合并已保存连接入暂存列表 → 项目根预填（不覆盖已填值）→ 项目下拉按会话项目选中 |
| 打开（编辑） | `open(Some(id))`：额外 `load_for_edit(id)` 回读全部字段（项目下拉以回读路径为准） |
| 项目会话变更（对话框开着） | 每帧比较 `Shared::project` 快照 → 变则重建项目下拉选项并选中新项目（「＋ 新增项目」创建后即走此路径）；脏草稿不丢：项目根写回只取非空输入 |
| 保存成功 | **不关闭**：草稿转正式 + 自动补空草稿（连续编辑），结果区提示；连接列表即时刷新 |
| 取消 / Esc / 点遮罩 | `staging_flush`（写回当前草稿）→ `close_dialog` → 通知宿主移除层 |

---

## 5. 数据流

### 5.1 打开对话框

```mermaid
flowchart LR
    A["入口：编辑区按钮 / 侧栏编辑 / 项目会话"] --> B["EditorPanel::request_new_connection / request_edit_connection"]
    B --> C["ConnectionDialogState::open"]
    C --> D["refresh_meta：引用配置 + 类型 / 驱动目录 + 项目下拉选项"]
    C --> E["staging_merge_saved：已保存连接入列表"]
    C --> F["load_for_edit（编辑模式）"]
    C --> G["notify_host → 层渲染"]
    G --> H{"项目下拉：＋ 新增项目?"}
    H -->|是| I["置位 Shared::project_new_request"]
    I --> J["WorkbenchView::render 消费 → open_create_dialog（脏草稿先走未保存确认）"]
    J --> K["新项目成为会话 → 下一帧选项 / 选中项自动跟随"]
```

### 5.2 保存链路（新建 / 编辑）

```mermaid
sequenceDiagram
    participant U as 用户
    participant D as ConnectionDialogState
    participant S as DataSourceService
    participant O as ConnectionOrgStore
    participant K as DuckDB Secret
    participant L as 连接列表（Shared）
    U->>D: 点击「保存」
    D->>D: collect：校验（名称 / URL / 协议链≤4跳）
    D->>D: 驱动名反查 drivers 表 → 驱动 id（db_type / driver_id）
    alt 编辑模式（editing_id = Some）
        D->>S: update(id, input, project_path)
    else 新建（按作用域）
        D->>S: save(input, project_path)
    end
    S->>S: 同名检查（大小写不敏感）+ 项目路径预检
    S->>O: tags 同步 / 分组一致性清理
    S-->>D: conn_id
    D->>K: ensure_secret_registered（use_duckdb_fed 时）
    D->>L: load_connections_for_scope → 刷新列表
    D->>D: staging_after_save：草稿转正式 + 补空草稿
    D-->>U: 结果区「已保存：G_xxx（已加入暂存列表，可继续新建）」
```

### 5.3 暂存列表状态机（多连接连续编辑）

```mermaid
stateDiagram-v2
    [*] --> 空草稿: 打开对话框（列表恒非空）
    空草稿 --> 编辑中: 输入 / 选择
    编辑中 --> 编辑中: 切 Tab / 引用 / 协议链（面板 notify → 宿主级联）
    编辑中 --> 编辑中: 切换条目（capture 写回 → apply 载入）
    编辑中 --> 已保存: 保存成功（staging_after_save）
    已保存 --> 空草稿: 自动追加新草稿并选中
    编辑中 --> 空草稿: 删除（✕）
    空草稿 --> 已保存: 点击已保存条目（load_for_edit 回读）
    编辑中 --> 已保存: 关闭对话框（staging_flush 写回草稿）
```

capture / apply 的数据边界（切换条目的正确性依赖它）：

| 方向 | 覆盖内容 |
| --- | --- |
| `capture_draft`（表单 → 草稿） | 名称 / 类型与驱动（`type_id` + `driver_id` + 实现短名）/ URL / 用户名密码 / 备注 / 作用域与项目路径 / SSL 四项 / 缓存路径 / 联邦开关 / 当前 Tab / 协议链 / 驱动属性 / 策略覆盖 / 三类引用 |
| `apply_draft`（草稿 → 表单） | 同上；驱动先恢复类型再按驱动 id（回退短名）选中；字符串型 Select 为空时清空选中（`set_selected_index(None)`） |

持久化时机（`connection_drafts` 表；详见 §3.5）：

| 时机 | 行为 |
| --- | --- |
| 首次打开对话框 | `staging_restore`：仅当仍为初始「单个空草稿」时用库中条目替换（避免覆盖本会话已编辑内容），并 `apply_draft(0)` 载入表单 |
| 变更操作（添加 / 切换 / 删除 / 保存后） | `staging_persist`：全量替换写入（条目通常个位数） |
| 关闭对话框（保存并关闭 / 取消 / Esc / 遮罩） | `staging_flush`：先写回当前表单再落库 |

### 5.4 运行时连接与隧道

```mermaid
sequenceDiagram
    participant N as 导航 / 状态栏
    participant C as ConnectionService
    participant T as connection::chain
    participant M as ConnectionManager
    N->>C: connect_with_type(ConnectRequest)
    C->>C: 认证注入（auth_config）→ URL 参数化
    C->>T: apply_network_method（Direct / SSL / SSH / 代理 / 链）
    T-->>C: 改写后的 URL + TunnelGuard 列表
    C->>C: tunnels.insert(conn_id, guards)
    C->>M: create_database（驱动 id 路由）+ add_connection
    Note over C,M: 任一步失败 → release_tunnels 回收（本地端口 + 后台任务）
    N->>C: close_connection(conn_id)
    C->>C: tunnels.take → drop（关闭隧道）
    C->>M: remove_connection（保留元数据缓存）
```

### 5.5 键盘快捷键（Action + key context）

对话框层独立于「workbench」key context，快捷键在为对话框容器声明的 `"connection-dialog"` 上下文中生效（app 层 `bind_keys`，见 `crates/app/src/main.rs`）：

| 按键 | Action | 说明 |
| --- | --- | --- |
| `Ctrl+Enter` | `SaveConnection` | 保存；焦点在输入框内时由 Input 的 `Enter` action（`secondary = true`）冒泡到容器兜底 |
| `Ctrl+T` | `TestConnection` | 测试连接 |
| `↑` / `↓` | `DraftPrev` / `DraftNext` | 切换暂存条目（焦点在输入框内时 ↑↓ 仍归 Input 的光标移动） |
| `Esc` | gpui-base `Cancel` | 关闭对话框（组件内置，context "Dialog"；关闭路径与取消一致写回草稿） |

```mermaid
sequenceDiagram
    participant K as 键盘
    participant I as Input（焦点在输入框）
    participant C as 对话框容器(key_context)
    participant A as Action handler
    K->>I: Ctrl+Enter
    I->>I: 单行输入：emit PressEnter + cx.propagate()
    I->>C: Enter action 冒泡
    C->>A: secondary=true → do_save
    Note over K,C: 焦点不在输入框时：key binding 直接命中 SaveConnection
```

### 5.6 模板导入导出（C4）

走**剪贴板 JSON**（无文件对话框依赖），仅限**未保存且非空**草稿：

```mermaid
flowchart LR
    A[暂存列表草稿] -->|templates_export| B[ConnectionTemplate JSON]
    B -->|剪贴板| C[用户粘贴 / 存档]
    C -->|剪贴板| D[templates_import]
    D -->|kind/version/非空 校验| E[追加为新草稿]
    E --> F[选中首条 + 解析驱动短名]
    F --> G[apply_draft 载入表单 · 密码留空]
```

| 项 | 约定 |
| --- | --- |
| 格式标识 | `kind = "rds.connection.template"`（防误粘贴） |
| 版本 | `version = 1`；导入拒绝高于当前版本 |
| 字段 | 名称 / `type_id` / `driver_id` / URL / 用户名 / 备注 / 作用域与项目路径 / SSL 模式 / 标签 / 联邦开关 |
| 不含 | **密码**、分组（项目级 id 无跨库意义）、协议链内联值与缓存路径 |
| 失败 | 解析 / 校验失败返回中文消息且**不修改**暂存列表 |

---

## 6. 关键设计决策与取舍

| # | 决策 | 理由 / 代价 |
| --- | --- | --- |
| 1 | 传输/协议层下沉 `connection`，编排层留 `workbench` | 依赖方向硬约束；代价是「连接服务」被拆两处，需按职责定位 |
| 2 | `db_type` / `driver_id` 存**驱动 id** | 与引擎 `DriverRegistry` key 对齐；驱动目录可扩展（多实现同库） |
| 3 | 对话框状态用 `Rc<ConnectionDialogState>` | 侧栏 / 管理器的回调需要克隆状态句柄调用方法；代价是 `EditorPanel` 字段非 `Entity`（不参与 gpui 观察，靠 `host_redraw` + `observe` 显式驱动） |
| 4 | 打开 / 关闭显式通知宿主重绘 | 绕开「Root 的 notify 到不了子视图」；代价是每个新入口都要记得调用（已有 4 类测试兜底） |
| 5 | 保存后**不关闭**对话框，另提供「保存并关闭」 | 连续编辑多个连接的前提；「保存并关闭」兼容会话式操作（`do_save` 共用同一闭包） |
| 6 | 草稿**跨会话持久化**到 `connection_drafts`（无密码列） | 应用退出不丢草稿；凭据不落此表，恢复后密码框为空，安全边界不变 |
| 7 | 已保存连接在暂存列表**不提供删除** | 删除是不可逆操作，入口统一收敛到导航栏（有确认与作用域路由） |
| 8 | 引用配置「选中即只读」 | 复用语义（改一处全量生效）的强制体现；代价是不能局部覆盖引用字段 |
| 9 | 图标按资产路径加载（`lucide()`） | `IconName` 只含组件默认子集；`AllAssets` 注册全量 Lucide，按路径可用 |
| 10 | 测试/编译固定 `-j 2` | 并发链接 DuckDB 静态库会耗尽内存（rustc 崩溃 + 宿主卡顿，见 `.cargo/config.toml`） |
| 11 | 暂存条目直显来源短码（`P/G/GP`）与脏标记（`●`） | 连续编辑时快速辨识作用域与未写回修改；不引入自绘 tooltip（gpui-kit 0.6 需 TooltipOverlay 集成，收益不值） |
| 12 | 快捷键用 Action + `key_context("connection-dialog")` | 对话框层不在 workbench 元素子树内，需独立 context；焦点在输入框时靠 `Enter` action 冒泡兜底 |
| 13 | 对话框拆为 `connection_dialog/{mod,state,staging,render,managers,helpers}.rs`（后续新增 `project_picker.rs`，共 7 个） | 单文件降至 176 行（入口）；子模块 `use super::*` 共享导入，公开路径不变；同步删除废弃的 `collect`/`hops_valid` 副本 |
| 14 | 草稿存储双入口：生产需全局单例，测试 `set_db_path_override` | 未初始化全局系统的进程（测试 / 降级启动）拒绝写库，避免误写用户真实 `global.db` |
| 15 | 类型在左侧选、驱动只显示实现短名 | 消除重复噪音（每个驱动项不再携带类型名）；代价是驱动短名依赖 `name` 括号约定，靠 `driver_short_name` 纯函数 + 单测兜底（无括号时原值不丢信息） |
| 16 | 暂存条目类型徽标用类型 `icon`（emoji） | 与左侧类型树同一套视觉语言，零图标资产成本；无类型/旧草稿时回退状态点，Header 同步提示（“请先在左侧选择数据库类型”） |
| 17 | Tab 内容区固定高度 + 内部滚动 | 切换 Tab 不重排侧栏 / Header（布局稳定优先于“对话框自适应高度”） |
| 18 | 模板导入导出走**剪贴板 JSON**（无文件对话框） | gpui 0.6 无跨平台文件选择开箱能力；剪贴板天然支持测试注入（`write_to_clipboard` / `read_from_clipboard`）；代价是需先复制到文件才能存档 |
| 19 | 作用域分段按钮**自绘**（不用 `Button` 变体） | RDS 主题未覆盖 `button_secondary_foreground`，组件变体会出现“文字不可见仍可点击”；自绘完全走 `theme.colors` token |
| 20 | 类型回推改为“仅未选类型时补全” | 避免“用户选了类型但驱动仍是旧值”时侧栏高亮弹回旧类型（真机反馈的“类型会变动”） |
| 21 | 侧栏两区（暂存 / 类型树）与 Tab 内容均**固定高度 + 内部滚动** | 条目 / 类型 / 内容再多也不拉长对话框（布局恒定优先）；代价是空间利用率略低 |
| 22 | 项目栏放**备注行右侧固定位置**，`仅全局` 时置灰不可编辑 | 作用域与项目语义相邻（备注行空间充裕）；三态切换不引起布局移动 |
| 23 | Header 定宽元素：名称 10.5rem / 驱动 11rem / 作用域分段短标签 | 切换类型时后续元素位置不移动；类型名过长用 `text_ellipsis`（固定布局优先于完整展示） |
| 24 | 模板导入导出**能力先就绪、UI 入口暂缓** | 避免当前阶段界面变动；方法 `pub` + 测试覆盖，后期只需接两个按钮 |
| 25 | Header 收敛为 **3 行 + 统一标签列（2.75rem）**，行序按用户建议布局 | 降低信息密度与视觉噪音：① 类型徽标（仅图标）· 名称（弹性）· 作用域分段；② 备注（弹性）· 项目（定宽 17rem）；③ 驱动（定宽 11rem）· URI（弹性）；类型徽标常驻行首，未辨识时显示 `?` |
| 26 | 项目栏为**单下拉**（项目名 + 路径左右结构），**无编辑 / 列表模式切换**（路径输入仍是单一数据源） | 真机反馈：两态切换与 ✎/列表按钮徒增噪音；单下拉即可含尽“选项目”语义；选中项由 render 写回路径输入，保存与作用域预检统一读路径 |
| 27 | UI 尺寸**常量化 + 规范文档**（间距五档 / 字体三档 / 图标三档 / 区域固定尺寸） | 之前只有颜色有硬约束，尺寸靠口头约定 → 反复微调；现由 `theme/ui-constraints.md` + `helpers.rs` 常量约束，新代码一律引用 |
| 28 | 项目下拉**末项固定为 `＋ 新增项目`**，选中 → 置位 `Shared::project_new_request` → 宿主开「新建项目」入口 | 下拉项与路径映射存在 `project_options`（label → 路径，新增项无路径）；用“确认事件 → 置位共享状态 → 宿主 render 消费”解开“事件回调不能直接重建面板”的借用冲突（与 `open_edit` 同模式）；有未保存草稿时先走未保存确认，避免静默丢弃 |
| 29 | 项目会话变更检测在**每帧比较**（不再写在 `meta_refreshed` 一次性守卫内） | 「＋ 新增项目」会在对话框打开期间创建并切换项目，下拉选项与选中项必须即时跟上；比较仅一次 `Option<(String,String)>` 借用，开销可忽；也避免仅因会话变更就重跑建 runtime + 查库的元数据拉取 |
| 30 | 对话框订阅在**面板入口**（`EditorPanel::request_*`）建立，**不得**在 `ConnectionDialogState::open` 内建 | `open` 是在面板 `update` 上下文里被调用的，在那里再 `entity.update(cx, …)` 会 panic（`cannot update … while it is already being updated`）；`ensure_dialog_subscription` 用面板自身的 `cx.subscribe_in` 直接建，句柄由面板持有、释放即取消 |
| 31 | 项目下拉项类型 `ProjectItem` 与 `PROJECT_NEW_LABEL` 声明为 **`pub` 并从 `connection_dialog` 再导出** | `ConnectionDialogState::project_sel` 是公开字段，字段类型不能比字段更私有（`private_interfaces` 警告）；集成测试也需能命名该类型以驱动下拉 |
| 32 | **无可用驱动的类型不可选**（类型树置灰 + 右标「暂无驱动」+ `select_type` 拒绝 + 结果行给原因），且类型目录查询加 `WHERE enabled = 1` | 类型目录与驱动目录是两层，可选不等于可用；让用户在入口就看到真实能力，避免“选完存不了”；代价是未接驱动前无法预选类型 |
| 33 | **项目侧更新用 `COALESCE(?9, password_encrypted)`**（空密码 → 保留原密文） | 与全局库 `update_global_connection` 语义对齐：编辑一次（密码框留空）不应把项目侧凭据清空——这是 C18 打通项目侧编辑后暴露的必然路径 |
| 34 | **GP_ 快照为独立副本，同步走显式入口**（`sync_snapshot_from_global` + footer 按钮，仅 GP_ 编辑时显示） | 快照语义简单可预测（不做隐式跟随）；同步拉取全局最新配置与凭据密文，保留快照 ID / 创建时间 / 分组关系；代价：用户需手动触发 |
| 35 | 项目下拉动作项顺序：**「不需要项目（仅全局）」→「打开现有目录…」→ 末项「＋ 新增项目」** | “不需要项目”不是清除选择（项目作用域必须有项目，否则保存必被拦）——它把作用域切为「仅全局」，与落库语义一致；末项仍保持“新增”不变 |
| 36 | **能力矩阵改读 `drivers.capabilities`**（UI 只保留「能力键 → 中文标签」字典；驱动声明字典外的键追加展示） | 旧实现渲染硬编码的 6 项布尔常量，与面板上“能力由驱动声明”的文案自相矛盾；改为跟库后，驱动新增能力（如 `federation`）无需改 UI |
| 37 | **策略覆盖清单改读 `environment_policies`**（选中环境 → 启用策略；摘要直接取 `policy_config` 的值），覆盖键 = `policy_type`；未选环境则不展示可覆盖项 | 旧实现硬编码 6 项（read_only / no_ddl…）且与库里 5 类策略（security / schema / performance / audit / ui）没有映射关系；现“清单 + 值 + 勾选项”均跟库，代价是覆盖键语义变更（旧存值忽略，不影响其它字段） |
| 38 | 环境管理器策略标签按 `policy_type` 字典映射；**新建策略只建空模板**（`policy_config = NULL`）并明确提示“配置项待编辑器实现” | 旧实现用 `POLICY_KEYS` 下标匹配 `policy_type`（永远 None → 5 条策略全部错标「只读连接」），且新建时会把 `read_only` 这类“假类型”写进 `environment_policies` |
| 39 | `DataSourceService::get` 重命名为 **`get_global`**（语义显式） | 该方法只查 `global_connections`；两处历史缺陷（对话框回读 / 导航连接）都因“以为它查全部”而起，命名就应带前提 |
| 40 | **项目根预检（读写分离）**：写路径（save / update / delete / 快照同步）要求目录存在且含 `.RSmeta`，否则明确报错；读路径（项目侧回读 / 列表加载 / 分组读写）判定不合法即降级为空并告警，**绝不创建目录** | `ProjectDatabaseManager::open` 与 `ConnectionOrgStore::open_project` 都会 `create_dir_all(.RSmeta)`：传入任意目录（旧草稿 / 手改路径 / 外来路径）就会在磁盘上凭空造出“项目骨架”，属于最难排查的脏数据；判定函数 `workspace_loader::is_project_root`（忽略大小写，兼容历史 `.RSMETA` 写法） |
| 41 | **项目侧连接时间戳由存储层兜底**：`create_connection` / `update_connection` 用 `COALESCE(NULLIF(?,''), CURRENT_TIMESTAMP)` | 全局侧一直用 SQL `CURRENT_TIMESTAMP`，而项目侧把调用方传入的空串原样写入 → 项目连接的 `created_at` / `updated_at` 长期为空（排序 / 展示都是脏数据）；现在调用方给真实值就用、给空则用当前时间 |
| 42 | **统一项目元数据目录拼写为 `.RSmeta`**（engine `project_db` / `connection_org_store`、insight、workbench `nav_store`） | 代码库曾同时存在 `.RSmeta`（project 模块，权威，45 处）与 `.RSMETA`（早期写法）；Windows 大小写不敏感所以一直“能用”，在大小写敏感系统上会分叉成两个目录（连接写 `.RSMETA`、项目模块读 `.RSmeta`）——属于隐性的跨平台数据错位 |
| 43 | **「认证方法」进 UI 与草稿**：常规 Tab「数据库认证」卡片新增下拉（选项来自 `drivers.supported_auth_types`）；保存写 `auth_method`；草稿持久化新列（迁移 `023`）；`apply_draft` 后按驱动重建选项 | 此前 UI 没有该字段、`collect` 硬编码 `None` → 连接链路里“引用认证配置”因缺 `auth_method` 被静默跳过（引用了档案却不注入凭据）；且草稿切换会丢该选择 |
| 44 | **连接链路兜底**：`auth_method` 缺失时用认证配置自己声明的 `auth_type` 注入凭据 | 存量数据（旧连接 / 其它入口创建）没有 `auth_method`，不能因为“新 UI 已补字段”就继续不生效；服务层回退保证引用语义真正落地 |
| 45 | **暂存列表按作用域合并已保存连接 + 清理幻影条目**：改用 `workspace_loader::load_connections_for_scope`（全局 + P_/GP_，与导航同一加载器）；`saved_id` 已不在可见集合内的条目被清除（未保存草稿永不清理） | 原实现用 `DataSourceService::list()`（只查全局）→ 项目侧连接在暂存列表里根本不出现（与文档 §2.2「按作用域可见性合并」不符）；连接在导航栏删除后暂存里会留下“已保存”幻影条目，点进去是空表单 |
| 46 | 草稿合并在**全局库未初始化时跳过**（不调用加载器的“默认数据目录”回退） | `load_connections_for_scope` 在缺少单例时会回退到用户真实数据目录——那会让测试 / 降级启动碰到真实库；宁可少一个便利功能 |
| 47 | **地址列随驱动变形 + 单一地址输入框**：网络型 Header ③ = `驱动 + URI`（可编辑输入框，占位取驱动 `url_template`）；文件型 Header ③ = `驱动 + 地址`（**只读路径或引导文案**），可编辑地址与 `打开文件… / 新建文件…` 放在「常规 → 连接设置」 | 真机反馈：选了 SQLite 仍显示 `mysql://host:3306/db` 占位，且同一地址在 Header 与卡片各有一个框（分不清权威值）；文件型没有 URI 语义，标签/占位/控件都应随驱动变化（决策 #25 的 Header 3 行结构保留，只是第③行随类型变形） |
| 48 | **常规 Tab 按驱动动态渲染卡片**：文件型只保留「连接设置（地址 + 打开/新建）+ 组织」；网络型四张卡（连接设置 / 数据库认证 / 连接安全 / 组织），且 SSL 卡片仅当驱动声明 `ssl` 时出现 | 真机反馈：SQLite 下出现认证 / 用户名 / 密码 / SSL 卡片，全是无效噪声；“四张卡并排”是网络型的设计（原型 §3.1 原文即“文件库则切换为文件选择”，本轮把“切换”真正实现） |
| 49 | **文件型地址落 `database` 列（修真实缺陷）**：`parse_url_host_port_db` 对文件型驱动返回 `Some(路径)`（去 scheme、修 Windows 三斜杠前导 `/`）；`build_effective_url` 对文件型不注入凭据 | 旧实现对 `sqlite`/`duckdb` 直接返回 `(None, None, None)` → 项目侧（P_/GP_）文件连接 **路径丢失**，`connection::url::build_connection_url` 会报“文件型连接缺少数据库路径”，编辑回读的地址框也是空的（全局侧因引擎自解析而“看起来正常”，掩盖了这个缺陷） |
| 50 | **文件型输入清洗（`strip_file_db_noise`）**：落库前清掉 `username`/`password`/`auth_method`/`auth_config_id`/`network_config_id` 与 `advanced_options` 里的 `ssl`/`network_chain` | 从 MySQL 切到 SQLite 后表单残留的凭据 / 网络链 / TLS 会被写入文件型连接（脏数据，且让“同一连接的凭据来自哪”变得模糊）；策略覆盖等文件型仍有效的选项保留 |
| 51 | **常规 / 高级 Tab 改为「单列分组大纲」，弃用并排卡片**：`outline_section`（标题行 = chevron + 图标 + 标题，点击折叠）+ `form_row`（标签定宽 + 控件弹性）+ `text_row`（只读值纯文本）+ `hint_line`；分组默认全展开，折叠态存 `collapsed_sections`（进程内，不落库） | 真机反馈：并排卡片靠 `flex_wrap + min_w` 均分，窄宽下换行（3+1）、卡高不齐、卡内长值被挤——本质是“用的布局语言不适合一张要填的表”。单列大纲只有一列宽度，阅读顺序即填写顺序，不可能出现换行/遮挡/挤压 |
| 52 | **输入框可见性（对比度）根因修复**：RDS 主题 `input.border` 由 `#D4D4D4` → `#B0B0B0`（Light）、`#3C3C3C` → `#4F4F4F`（Dark）；只读值改为**纯文本行**，不再用与卡底同色的白框 | 根因：`Input` 的边框色就是 `theme.colors.input`（JSON 的 `input.border`），而 Light 下输入框底 = `background` = 卡片底 = `#FFFFFF`，只剩一条极浅的 `#D4D4D4` 细线——真机表现为“输入框几乎看不见”。`val_readonly` 白底白框同理 |
| 53 | **字段集合改为「驱动 schema × 连接方式」的函数**：新增 `helpers::driver_form_fields`（解析 `drivers.config_schema.fields[]`）+ `field_spec` / `address_field` / `address_row_label`；行的存在性 / 标签 / 占位来自声明，schema 为空时回退内置字段 | 为“将来数据源多种多样（服务型 / 文件型 / HTTP / NoSQL）”留口：新驱动只靠 DB 行就能改变表单；未映射字段与「字段 ⇄ URL」双向组装列入 §14 / 原型 §7 待办（现 URL 仍为唯一权威） |
| 54 | **高级 Tab 与常规 Tab 共用同一套大纲组件**；DuckDB 加速从 warning 边框卡片改为分组（warning 色标题 + `启用` / `缓存路径` 行） | 同一对话框内不引入第二套排版语言（卡片已弃用）；warning 色标题保留权重，但不再用“框套框”制造层次 |
| 55 | **分组 = 整幅面板 + 标题栏底部分隔线**；**只读值回到“数据框”**（白底 + `input` 边框）；表单行标签列 `4.25rem` + 间距 `0.375rem`（`hint_line` 同步缩进对齐） | 真机反馈第三轮：① “连接设置 / 认证信息等区域分辨不清”→ 大纲分组需要容量（面板浅底 `group_box` + 分隔线）；② “连接设置没有数据框”→ #52 把只读值改成纯文本过度收敛了（现在是**白底数据框放在浅底面板上**，两个需求同时满足）；③ “标签和输入框距离太远” → 标签列 92px → **68px**、间距 12px → **6px**（仍是定宽列，保证控件左边缘对齐） |
| 56 | **未选类型 / 驱动：表单正常渲染但整体禁用**（`Input/Select/Checkbox/Button` 统一 `.disabled(true)`，SSL 组也以禁用形态出现） | 真机要求：“未选择数据库类型时输入框都要显示，只是置灰不可输入”；不这样做会出现“能打字但存不了”的无效输入 |
| 57 | **文件选择分两个系统对话框**：`打开文件…` = `prompt_for_paths`（Windows 带 `FOS_FILEMUSTEXIST`，只能选已存在文件）；`新建文件…` = **`prompt_for_new_path`**（真正的保存对话框，可输入新文件名；不存在则 `File::create`，已存在则直接引用且**不清空**）；取消 / 失败 / 无响应均写入结果行 | 旧实现用打开对话框当“新建”用 → 用户输入新文件名被 Windows 拒掉，表现为“新建功能没实现”；且三种结局都被静默吞掉，看着像按钮没反应 |


---

## 7. 测试策略

| 层次 | 文件 | 覆盖 |
| --- | --- | --- |
| 传输单测 | `crates/connection/src/*`（含 `chain.rs`） | URL 处理族 / 参数注入 / 隧道注册表 / Secret |
| 传输集成 | `crates/connection/tests/tunnel_roundtrip.rs` | SOCKS5 / HTTP CONNECT / 两跳链真实数据往返 + 守卫释放关闭 |
| 服务层 | `data_source_lifecycle.rs` | 保存 / 回读 / 更新 / 删除 / 同名拦截 / 作用域预检 / tags 同步 / **空密码更新保留原密文** / **项目侧回读（`get_with_project`）** / **GP_ 快照同步（含错误路径）** / **导航入口项目侧解析** / **环境策略按环境名读库** / **文件型路径落 `database`（全局 + 项目两侧；更新不清空；可还原连接 URL）** |
| 服务层 | `real_connections.rs` / `connection_scope_and_state.rs` / `global_service_singleton.rs` | 加载器契约 / 可见性与运行态 / 单例生产路径 |
| 服务层 | `connection_tunnel_cleanup.rs` | 连接失败后隧道回滚（`tunnel_count == 0`） |
| 窗口 | `connection_dialog_ui.rs` | 打开 / 渲染 / 关闭、五 Tab、编辑入口、状态保留、重入不叠加 |
| 窗口 | `dialog_host_layer.rs` | 入口调起（`debug_bounds("dialog-layer")`）、关闭移除层、面板 notify 级联 |
| 窗口 | `connection_staging.rs` | 暂存：切换保留字段 / 删至最后补位 / 保存后转正式补位 / 已保存不参与删除 |
| 窗口 | `connection_drafts_persist.rs` | 跨会话恢复：变更落库 → 新状态恢复草稿与表单；**密码不落库**（恢复后为空） |
| 窗口 | `connection_type_driver.rs` | 类型 × 驱动两层选择：选类型→下拉切到该类型启用驱动并默认选中（短名）；按驱动 id 回读（跨类型同名短名不歧义）；快照携带 `type_id` / `driver_id`；**无可用驱动的类型被拒绝并给出原因**；**文件型与网络型常规 Tab 互切渲染不 panic**（占位 / 分组集合随驱动重建）；**分组折叠态切换后重渲染** |
| 窗口 | `connection_project_picker.rs` | 项目下拉：会话项目置顶 + 选中（默认选当前项目、项目根写回路径）/ 末项 `＋ 新增项目` 在选项中 / 确认「新增项目」→ 置位 `project_new_request` 并清空选中 / 确认普通项目 → 路径写回 / 空确认无副作用 / 下拉项搜索与 `path`·`is_new` 契约（宿主走生产入口 `request_new_connection`） |
| 服务层 | `data_source_lifecycle.rs::nav_runtime_resolves_project_connection_with_project_path` | 导航入口项目侧解析：带项目根可解析（作用域回推为“仅项目”）、无项目根报「数据源不存在」 |
| 单测 | `connection_dialog/helpers.rs`（内嵌） | `driver_short_name` 括号提取与回退 / `find_driver_by_value` 三路匹配 / 类型过滤 / 类型徽标 emoji 回退 / `type_has_driver` / **能力 JSON 解析与矩阵（字典外键保留）** / **策略类型↔标签往返与配置摘要（不造值）** / **地址标签与占位随驱动推导（url_template 示例值 / 文件型提示）** / **文件型输入清洗（凭据·网络·TLS 不落库，策略覆盖保留）** / **`config_schema.fields` 解析（存在性·标签·type、缺失与非法输入不造字段）** |
| 窗口+服务 | `connection_multi_save.rs` | 连续保存两条连接（单例临时库）：暂存列表转正式 + 补空草稿；库中两条可读回 |
| 存储单测 | `engine::persistence::connection_draft_store`（内嵌） | 行序 roundtrip / 全量替换语义 / 表无 password 列（安全约定） |

约定：测试全部落临时目录、不触真实库；`#[gpui_kit::test]` 且**禁用通配导入**（避免 `#[test]` 宏遮蔽，见 gpui-kit-dev skill）。

---

## 8. 实现进度与后续

已实现（本轮）：

| # | 项 | 实现位置 |
| --- | --- | --- |
| 1 | 「保存并关闭」按钮 | `render.rs` footer（与保存共用 `do_save` 闭包） |
| 2 | 草稿跨会话持久化（无密码） | `global/020_add_connection_drafts.sql` + `engine::persistence::connection_draft_store` + `staging_{restore,persist}` |
| 3 | 侧栏条目来源短码 / 脏标记 | `staging.rs::saved_scope_short` + `staging.rs::draft_dirty`（render 处徽标与 `●`） |
| 4 | 键盘导航 | `crates/workbench/src/commands.rs`（actions）+ `app/main.rs`（bind_keys）+ `render.rs`（容器 handler） |
| 5 | 拆分对话框单文件 | `components/connection_dialog/{mod,state,staging,render,managers,helpers}.rs` + `project_picker.rs` |
| 6 | 导航栏「在对话框中编辑」入口 | `panels.rs::render_connection_row`（✎ 按钮 → `shared.open_edit`） |
| 7 | 真机集成用例 | `tests/connection_multi_save.rs`（连续保存两条） |
| 8 | 文档防腐 | 本文（§2.2 / §3.5 / §5.3 / §5.5 / §6 / §7 / §9） |
| 9 | 类型 × 驱动两层选择去噪（左侧选类型 / 右侧驱动实现短名；未选类型时 Header 提示） | `helpers.rs`（`driver_short_name` / `type_badge`）、`state.rs`（`select_type` / `set_driver_by_value`）、`render.rs` |
| 10 | 暂存条目类型徽标 + 草稿 `type_id` / `driver_id`（迁移 `021`） | `staging.rs`、`global/021_add_draft_type_driver.sql` |
| 11 | **布局稳定**：Tab 内容区固定高度 + 内部滚动（切 Tab 不再改变对话框高度） | `render.rs`（`tab_body` + `overflow_y_scrollbar`） |
| 12 | **性能修复**：元数据 / 暂存恢复一次性（`meta_refreshed`）——避免 dialog builder 重渲染反复建 runtime + 查库 | `mod.rs` 字段 + `render.rs` + `panels.rs::request_*` |
| 13 | **Header 去拥挤**：作用域三态分段按钮 + 项目栏（当时为“项目名 + 悬停气泡”，现已被 #20 的单下拉取代） | `render.rs`（`project_hover` 已删除） |
| 14 | **标签 / 分组入口**：常规 Tab「组织」卡片（标签输入 + 项目分组勾选）；服务与存储同步（替换语义，迁移 `022`） | `render.rs`、`data_source_service.rs`、`connection_org_store.rs::set_connection_groups` |
| 15 | 文档体系补全：用户指南（含 USIT 清单）+ 数据字典 / 降级矩阵 / 性能可观测安全 / 成熟度评估 | `connection-user-guide.md`、本文 §10–§13 |
| 16 | **C4 模板导入导出**（剪贴板 JSON，无密码；导出仅未保存非空草稿；导入校验 kind/version） | `staging.rs::templates_{export,import}` + `render.rs` 标题行按钮 + `tests/connection_template.rs` |
| 17 | **UI 缺陷修复**（真机反馈）：分段控件自绘 / 驱动选中校正 / 类型回推仅补空 / 备注宽度 / 来源提示 | `render.rs`、`state.rs`、`staging.rs` |
| 18 | **布局再收敛**：暂存区 7.5rem 固定 + 滚动；类型树占满剩余 + 滚动；项目栏移至备注行（仅全局灰显）；类型徽标定宽省略；模板 UI 入口撤下 | `render.rs`（决策 #21–#24） |
| 19 | **Header 再设计**：3 行（类型徽标 + 名称 + 作用域 / 备注 + 项目 / 驱动 + URI），统一标签列 2.75rem；类型徽标定宽 6rem、未辨识时提示；作用域分段短标签 | `render.rs`、`helpers.rs`（`header_label`）（决策 #25） |
| 20 | **项目栏单下拉 + 新增项目入口**：项目名（左）+ 路径（右、头部省略）左右结构，末项 `＋ 新增项目`；`handle_project_confirm` 为确认落点（可测）；项目会话变更每帧检测并自动跟随；宿主消费 `project_new_request` 开「新建项目」（有脏草稿先走未保存确认） | `project_picker.rs`（新增）、`state.rs`、`render.rs`、`panels.rs`、`view.rs`；测试 `tests/connection_project_picker.rs` 4 项（决策 #26 / #28 / #29） |
| 21 | **项目侧连接编辑回读**：`DataSourceService::get_with_project`（P_/GP_ 路由到项目库）+ `map_project_connection_to_data_source`；`load_for_edit` 带项目根（`open` 取会话快照 / `apply_draft` 取条目路径），分组回显同源 | `services/data_source_service.rs`、`connection_dialog/{state,staging,render}.rs`；测试 `tests/data_source_lifecycle.rs`（新增 1 项 + GP 用例补断言） |
| 22 | **订阅建立位置修正（回归修复）**：项目下拉订阅从 `open()`（面板 `update` 上下文，重入 panic）移到面板入口 `ensure_dialog_subscription`；`EditorPanel::dialog_state()` 暴露状态供宿主 / 测试只读访问；窗口测试改走生产入口 `request_*` | `panels.rs`、`connection_dialog/render.rs`；回归由 `tests/dialog_host_layer.rs` 3 项守住（决策 #30） |
| 23 | **导航入口同源修复**：`nav_runtime::load_entry(_with)` 改用 `get_with_project`（原先只查全局库 → 项目侧连接点「连接」报「数据源不存在」） | `services/nav_runtime.rs`；测试 `tests/data_source_lifecycle.rs::nav_runtime_resolves_project_connection_with_project_path`（§14 #8 的同类风险已排查） |
| 24 | **已知问题清单**：架构 §14（13 项，🔴/🟡/⚪）；原型 HTML 与三份连接文档按当前实现全量同步一遍 | 本文 §14、`connection-prototype-design.md`、`connection-user-guide.md`、`connection-dialog-prototype.html` |
| 25 | **类型可用性（§14 #1/#2 关闭）**：类型目录按 `enabled` 过滤；无可用驱动的类型置灰 + 「暂无驱动」标注 + `select_type` 拒绝并在结果行说明；驱动下拉同理禁用占位 | `engine/persistence/driver_store.rs`、`connection_dialog/{render,state,helpers}.rs`；测试 `connection_type_driver.rs` + `helpers.rs` 单测 |
| 26 | **项目下拉补「打开现有目录…」**（§14 #3 关闭）：动作项两枚（打开现有目录 / ＋ 新增项目，后者仍为末项），宿主置位 `project_open_request` → `open_folder_dialog` | `project_picker.rs`、`state.rs`、`panels.rs`、`view.rs`；测试 `connection_project_picker.rs` |
| 27 | **GP_ 快照同步 + 项目侧密码保留**（§14 #5 关闭 + 新缺陷修复）：`sync_snapshot_from_global` + footer「从全局定义同步」（仅 GP_ 编辑时显示）；`ProjectConnectionStore::update_connection` 改 `COALESCE` 保留空密码时的原密文 | `services/data_source_service.rs`、`connection_dialog/render.rs`、`engine/persistence/project_connection_store.rs`；测试 `data_source_lifecycle.rs`（+2 项） |
| 28 | **数据来源审计：零 UI 造数据**（§15）：能力矩阵改读 `drivers.capabilities`；高级 Tab 策略覆盖改读 `environment_policies`（切环境自动重查，覆盖键 = `policy_type`）；环境管理器策略标签按 `policy_type` 映射（修复“全部错标只读连接”与写假类型）；`get` → `get_global`；项目下拉补「不需要项目（仅全局）」（§14 #6 关闭） | `connection_dialog/{helpers,state,render,managers}.rs`、`services/data_source_service.rs`、`staging.rs`（草稿字段改存策略类型）；测试 `helpers.rs`（+2 单测）、`data_source_lifecycle.rs`（+1：策略按环境名读库）、`connection_project_picker.rs`（+1 项） |
| 29 | **脏数据防护与接口一致性（USIT 前置）**：项目根预检（读写分离，不建目录）/ 项目侧时间戳存储层兜底 / 项目元数据目录拼写统一 `.RSmeta` / 暂存列表按作用域合并 + 幻影条目清理 | `services/{data_source_service,workspace_loader}.rs`、`engine/persistence/{project_db,connection_org_store,project_connection_store}.rs`、`insight/rule_registry.rs`、`nav_store.rs`、`connection_dialog/staging.rs`（决策 #40–#42、#45、#46） |
| 30 | **「认证方法」闭环**：UI 下拉（驱动声明）+ 保存 / 回读 / 草稿新列（迁移 023）+ 连接链路按配置 `auth_type` 兜底 | `connection_dialog/{render,state,staging,helpers,mod}.rs`、`services/connection_service.rs`、`engine/migrations/global/023_*.sql`、`engine/persistence/connection_draft_store.rs`（决策 #43、#44） |
| 31 | **常规 Tab 按驱动动态渲染（USIT 第 1 轮）**：地址标签 / 占位随驱动（网络型取 `url_template`，文件型取文件提示）；文件型 Header 不再放地址框，改在「连接设置」卡内（地址 + `打开文件…` / `新建文件…`）；文件型卡片集合收敛为「连接设置 + 组织」；SSL 卡片按驱动声明显示 | `connection_dialog/{helpers,state,render}.rs`（决策 #47、#48；`pick_db_file` 走 `App::prompt_for_paths`） |
| 32 | **文件型地址数据链修复 + 落库清洗**：`parse_url_host_port_db` 文件型返回路径（去 scheme / 修三斜杠）；`build_effective_url` 文件型不注入凭据；`strip_file_db_noise` 清洗凭据 / 网络 / TLS；`reconstruct_url` 文件型回裸路径 | `services/data_source_service.rs`、`connection_dialog/{helpers,state}.rs`（决策 #49、#50）；测试：`data_source_lifecycle::file_db_path_survives_save_and_readback`、`helpers` 内嵌 2 项、`connection_type_driver` +1 |
| 33 | **常规 / 高级 Tab 改单列分组大纲（USIT 第 2 轮）**：`outline_section` / `form_row` / `value_box` / `hint_line`；分组折叠态（`collapsed_sections`，默认全展开）；字段集合改由 `drivers.config_schema` 推导（存在性 / 标签 / 占位，地址行取 `type=file`）；主题 `input.border` 提对比度（修“输入框几乎看不见”） | `connection_dialog/{helpers,state,render}.rs`、`assets/themes/rds-theme.json`（决策 #51–#54）；测试：`helpers` 内嵌 +1（schema 解析）、`connection_type_driver` 补折叠断言 |
| 34 | **大纲视觉收敛 + 文件选择修复（USIT 第 3 轮）**：分组改「整幅面板（`group_box`）+ 标题栏分隔线」；只读值改回**数据框**（白底 + `input` 边框，放在浅底面板上）；标签列 92px→**68px**、间距 12px→**6px**；未选类型/驱动时表单**显示但禁用**（含 SSL 组以禁用形态出现）；`新建文件…` 改用系统**保存**对话框（`prompt_for_new_path`）+ 取消/失败写结果行 | `connection_dialog/{helpers,state,render,mod}.rs`（决策 #55–#57）；`check` 零警告 + 工作台 98 项测试全绿 |


后续可选（未做）：

| # | 项 | 价值 | 前置 |
| --- | --- | --- | --- |
| A | C4 模板导入导出（无密码） | 团队间共享连接配置；与草稿快照结构天然同源 | 按用户决策暂缓（本轮不做） |
| B | 暂存条目拖拽排序 / 「测试全部」 | 批量配置的可用性 | gpui-kit 0.6 无开箱拖拽，可用上下移替代 |
| C | 自绘 tooltip（暂存条目短码释义） | 减少认知成本 | 需接入 gpui-base `TooltipOverlay` |
| D | 分组 / 标签管理与视图（新建分组、按标签检索） | 组织能力的消费侧 | 导航模块（database-nav） |
| E | 项目下拉支持**浏览目录打开其他项目**（复用 `project::ui::open_folder_dialog`） | 现在只能选“当前 + 最近项目”；未在最近列表的本机项目需先去标题栏切换项目 | 需在宿主消费 `project_new_request` 旁扩展一个 `project_open_request` 入口 |
| F | 国际化 / 可访问性 / 指标（§13 缺口） | 平台级能力 | 全局排期 |

---

## 9. 实现位置映射

| 设计决策 | 代码 |
| --- | --- |
| 对话框（五 Tab / Header / 侧栏 / 暂存列表 / 管理器） | `crates/workbench/src/components/connection_dialog/{render,managers,helpers}.rs` |
| 草稿快照与暂存方法 | `connection_dialog/staging.rs`（`ConnectionDraft` + `staging_*`） |
| 快捷键 Action | `crates/workbench/src/commands.rs` + `crates/app/src/main.rs` |
| 入口与宿主重绘桥 | `crates/workbench/src/panels.rs`（`EditorPanel::{request_new_connection, request_edit_connection}`、`Shared::{host_redraw, notify_host}`） |
| 对话框层挂载与通知级联 | `crates/workbench/src/view.rs`（`Root::render_dialog_layer`、`cx.observe(editor)`） |
| 连接 CRUD / 测试 / 作用域路由 | `crates/workbench/src/services/data_source_service.rs` |
| 列表可见性与运行态 | `crates/workbench/src/services/workspace_loader.rs` |
| 会话与隧道登记 | `crates/workbench/src/services/connection_service.rs` + `crates/connection/src/chain.rs` |
| 驱动 / 类型目录与三类引用 | `crates/engine/src/persistence/{driver_store,auth_store,network_store,env_store}.rs` |
| 组织元数据（标签 / 分组） | `crates/engine/src/persistence/connection_org_store.rs` |
| 暂存草稿持久化 | `crates/engine/src/persistence/connection_draft_store.rs` + `crates/engine/migrations/global/020_add_connection_drafts.sql` |
| Secret 加速 | `crates/workbench/src/services/secret_integration.rs` + `crates/connection/src/secret.rs` |
| 暂存行为测试 | `crates/workbench/tests/connection_staging.rs`、`connection_drafts_persist.rs`、`connection_multi_save.rs` |
| 类型 × 驱动与去噪 | `connection_dialog/{helpers,state}.rs`（`driver_short_name` / `find_driver_by_value` / `select_type`）、`tests/connection_type_driver.rs` |
| 标签 / 分组入口 | `connection_dialog/{render,staging,state}.rs`（组织卡片 + 草稿字段）、`services/data_source_service.rs`（`list_groups` / `groups_of` / `set_connection_groups`）、`engine/connection_org_store.rs`（`set_connection_groups`） |
| Layout 稳定与性能标记 | `render.rs`（`tab_body` 固定高度 + `overflow_y_scrollbar`；`meta_refreshed` 字段） |
| 项目栏下拉与新增项目入口 | `connection_dialog/project_picker.rs`（`ProjectItem`）、`state.rs::handle_project_confirm`、`render.rs::subscribe_project_confirm`、`panels.rs::ensure_dialog_subscription`、`view.rs`（消费 `project_new_request`） |
| 项目侧连接解析（编辑回读 / 导航连接） | `services/data_source_service.rs::get_with_project`、`connection_dialog/state.rs::load_for_edit`、`services/nav_runtime.rs::load_entry(_with)` |
| 用户使用指南 | `docs/architecture/connection/connection-user-guide.md` |

---

## 10. 数据字典与迁移

### 10.1 表一览（连接模块相关）

| 表 | 库 | 关键列 | 用途 |
| --- | --- | --- | --- |
| `global_connections` | global | `id`(G_)、`name`、`db_type`、`driver_id`、`url`、`username`、`password_encrypted`、`scope` 相关、`tags`、`advanced_options`、`driver_properties`、`use_duckdb_fed`、`metadata_path` | 连接定义（全局） |
| `connections` | project | 同上（P_/GP_） | 连接定义（项目侧） |
| `data_source_types` | global | `id`、`name`、`category`、`icon`、`enabled` | 侧栏类型目录（icon 为 emoji，如 🐬 / 🐘）；查询只取 `enabled = 1`（禁用类型不上类型树） |
| `drivers` | global | `id`、`type_id`、`name`、`driver_kind`、`is_file`、`default_port`、`url_template`、`config_schema`、`capabilities`、`driver_properties`、`enabled` | 驱动目录（**种子仅 4 个**：mysql / postgres / sqlite / duckdb；无可用驱动的类型在类型树上置灰不可选，见 §14 #1） |
| `auth_configs` | global | `id`、`name`、`auth_type`、`auth_data`(AES) | 认证引用 |
| `network_configs` | global | `id`、`name`、`method`、`chain`(JSON)、`capabilities` | 网络引用（协议链） |
| `environments` | global | `id`、`name`、`policies`(JSON) | 环境 + 策略 |
| `connection_tags` | global + project | `connection_id`、`tag` | 标签**权威检索表** |
| `connection_groups` / `connection_group_members` | project | `id`/`name`/`sort_order`；`group_id`/`connection_id`/`sort_order` | 项目级分组（多对多 + 组内排序） |
| `connection_drafts` | global | `position`、`name`、`saved_id`、`type_id`、`driver_id`、`driver_name`、`url`、`username`、`remark`、`scope`、`project_path`、`ssl_*`、`cache_path`、`duckdb_fed`、`active_tab`、`hops_json`、`props_json`、`sec_overrides_json`、`auth_ref`、`network_ref`、`env`、`tags`、`groups_json` | 对话框暂存列表（**无密码列**） |

### 10.2 字段语义要点

- `db_type` / `driver_id`：**均为驱动 id**（如 `mysql_native`），与引擎 `DriverRegistry` key 对齐；显示名（`MySQL (sqlx)`）仅存于 `drivers.name`，UI 展示用 `driver_short_name` 取括号内实现名。
- `tags`：JSON 数组文本；检索与导航消费统一读 `connection_tags`（`tags` 为兼容投影）。
- `advanced_options`：内嵌 `ssl`（mode/ca/cert/key）与 `policy_overrides`。
- `connection_drafts.position`：列表下标即主键（全量替换写入）。

### 10.3 迁移清单（连接模块相关）

| 迁移 | 内容 |
| --- | --- |
| `global/008` | 数据源模块建表（types / drivers / connections / 引用配置） |
| `global/009` | ID 前缀约定 |
| `global/010` | `auth_method` |
| `global/011` | 配置索引 |
| `global/012` | 网络配置 `auth_config_id` |
| `global/013` | 原生驱动（`mysql_native` / `postgres_native`） |
| `global/014` | 驱动 ID 与 Registry key 对齐 |
| `global/016` | 驱动属性 |
| `global/017` | 网络能力集 |
| `global/020` | `connection_drafts`（暂存列表） |
| `global/021` | 草稿 `type_id` / `driver_id` |
| `global/022` | 草稿 `tags` / `groups_json` |
| `global/023` | 草稿 `auth_method`（认证方法；空串 = 未选） |
| `project_meta/*` | 项目侧连接 / 分组 / 导航状态（见导航模块方案） |

> 迁移采用 include_dir 编译期嵌入 + 版本号幂等执行；新增列一律带默认值，旧行安全。

---

## 11. 错误处理与降级矩阵

| 失败点 | 用户可见表现 | 处理策略 |
| --- | --- | --- |
| 全局系统未初始化 | 列表空 + 提示 | 降级运行（连接 CRUD 不可用，其余界面可用） |
| 保存失败（校验 / 同名 / 落库） | 结果行红色 + 原因，对话框不关闭 | 草稿保留，可修正后重试 |
| 测试连接失败 | 结果行红色 + 原因 | 不写库；协议链隧道回滚 |
| 隧道建立后握手失败 | 连接报错 | `release_tunnels` 回收（本地端口 + 后台 accept） |
| DuckDB Secret 注册失败 | 无提示（日志告警） | 不影响连接本身（加速为增强能力） |
| 标签 / 分组同步失败 | 无提示（日志告警） | 不阻断保存 |
| 草稿持久化失败 | 无提示（日志告警） | 内存草稿仍可用 |
| 元数据（引用 / 类型 / 驱动）拉取失败 | 对应下拉为空 | 不阻断其他字段；下次打开重试 |
| 未打开项目 + 项目作用域 | 保存被拦截并提示 | 引导改「仅全局」或先打开项目 |
| 项目侧连接（P_/GP_）编辑回读但无项目根 | 表单为空（不报错、不误写） | `get_with_project` 返回 `None`；对话框保持空表单，可改用全局连接或先打开项目 |
| 驱动目录缺该驱动 | 下拉未选中 | 完整名 / 短名回退解析；仍失败需手选类型 |

---

## 12. 性能、可观测性与安全

**性能约束与手段**

- 元数据（引用 / 类型 / 驱动 / 分组）**每次打开对话框拉取一次**：`meta_refreshed` 标记避免 dialog builder 重渲染时反复建 tokio runtime + 查库。
- 驱动下拉只含当前类型启用驱动；类型过滤在内存完成。
- 暂存列表条目通常为个位数，全量替换写入（DELETE + INSERT）。
- Tab 内容区固定高度 + 内部滚动：切换 Tab 不重排侧栏 / Header。
- 协议链 ≤ 4 跳；隧道按连接 ID 注册与回收。

**可观测性**

- 日志 target：`data_source_service`（保存 / 更新 / 标签 / 分组）、`connection_service`（会话与隧道）。
- 关键事件：保存成功 / 失败、标签同步、分组同步、Secret 注册失败。
- 实例日志：`target/rds-app.log` / `target/rds-app.err.log`。
- 诊断接口：`ConnectionService::tunnel_count`（测试与排查隧道泄漏）。

**安全边界**

| 项 | 做法 |
| --- | --- |
| 连接凭据 | `auth_store` AES-256-GCM；连接记录存 `password_encrypted`；**编辑时密码框留空＝保留原密文**（全局与项目侧均为 `COALESCE` 语义，不会因一次保存把凭据清空） |
| 草稿 | `connection_drafts` **无密码列**；恢复后密码框为空 |
| DuckDB Secret | `PERSISTENT` + `SET secret_directory` 指向应用目录（不写用户主目录） |
| URL 展示 | 列表 / 日志脱敏（`mask_password_in_url`） |
| 模板导入导出（C4） | **能力已实现**（`ConnectionTemplate` + `templates_{export,import}`，剪贴板 JSON，不含凭据；2 项测试）；**UI 入口按决策暂缓**，入口接上后无需改存储 |

---

## 13. 成熟度评估（工业级对照）

| 维度 | 现状 | 评级 | 主要缺口 |
| --- | --- | --- | --- |
| 需求与边界 | 模块边界明确（不含导航视图） | 良 | — |
| 架构与依赖 | 硬约束 + 六文件分层 + 类型所有权清晰 | 良 | — |
| 数据模型 | 表 / 字段 / 迁移字典（§10）+ 兼容策略 | 良 | 缺模式版本号（草稿 JSON 结构演进） |
| 交互设计 | 原型文档 + 可交互 HTML + 主题 token 映射 | 良 | 缺多分辨率 / 高 DPI 截图基线 |
| 使用文档 | 用户指南 + USIT 清单（`connection-user-guide.md`） | 良 | 缺录屏 / 动图 |
| 测试 | 单测 / 窗口测试 / 服务集成 / 真机用例；临时库隔离 | 良 | 缺 UI 图像回归、缺 fuzz / 属性测试 |
| 错误处理 | 降级矩阵（§11）+ 结果行内联提示 | 中良 | 缺统一错误码与用户可复制的诊断号 |
| 性能 | 一次性元数据、固定布局、写入量小 | 中 | 缺基准数据与大数据量（数千连接）验证 |
| 可观测性 | 结构化日志 + 诊断接口 | 中 | 缺指标（metric）与面板 / 追踪 |
| 安全 | 凭据加密、草稿无密码、Secret 隔离、日志脱敏 | 良 | 缺审计日志与凭据轮换策略 |
| 国际化 | 中文硬编码 | 弱 | 无 i18n 框架接入 |
| 可访问性 | 快捷键 + 焦点可用 | 中 | 缺屏幕阅读器语义与焦点顺序规范 |
| 发布与回滚 | 迁移幂等 + 版本号 | 中良 | 缺向下回滚脚本 |

**结论**：核心链路（新建 / 编辑 / 暂存 / 保存 / 测试 / 作用域 / 组织元数据）已达到
“可交付的工业级雏形”：架构、数据模型、迁移、测试与使用文档齐备，缺陷集中在
**i18n / 可访问性 / 可观测指标 / 性能基准 / 审计与凭据轮换**——这些属于平台级能力，
建议随全局（非本模块单独）能力排期。

---

## 14. 已知问题与后续项（待办清单）

> 状态说明：以下均为**当前实现在真机 + 测试中确认存在**的缺口或取舍，不是猜测。
> 按“是否阻断主链路”分三级：🔴 影响可用性 / 🟡 体验或语义不完整 / ⚪ 工程与文档债。
> **2026-09-12 更新**：#1 / #2 / #3 / #5 / #6 / #8 已关闭（见下方“已关闭”段）；同时修复了 3 处**“UI 自造业务数据”**（能力矩阵 / 策略覆盖 / 环境管理器策略标签，见 §15）。剩余 #4、#7、#9–#13。
> **USIT 第 1 轮（同日）**：又关闭 2 项——文件型连接地址丢失（🔴 数据链缺陷）与常规 Tab 不随驱动动态渲染（🟡），见「已关闭（USIT 第 1 轮）」段。

**已关闭（本轮）**

| # | 关闭方式 | 验证 |
| --- | --- | --- |
| 6（🟡） | **项目下拉补「不需要项目（仅全局）」**：单选即把作用域切为「仅全局」并清空项目路径（不是“清除选择”——项目作用域必须有项目） | `connection_project_picker.rs::confirm_no_project_switches_scope_to_global` |
| 8（⚪） | **`DataSourceService::get` → `get_global`**（命名带前提，调用点一眼可辨；对话框/导航统一走 `get_with_project`） | 全工作区测试无回归 |
| 新增（🔴） | **UI 自造数据 ×3（审计发现，见 §15）**：① 能力矩阵改读 `drivers.capabilities`；② 策略覆盖清单/值改读 `environment_policies`（切环境重查）；③ 环境管理器策略标签按 `policy_type` 映射（此前 5 条策略全被错标「只读连接」，新建还会写假类型） | `helpers.rs` 单测（能力/策略解析与字典）+ `data_source_lifecycle.rs::environment_policies_come_from_db_by_env_name` |

**已关闭（本轮）**

| # | 关闭方式 | 验证 |
| --- | --- | --- |
| 1（🔴） | **无驱动类型不可选**：类型树对无可用驱动的类型置灰并右标「暂无驱动」，`select_type` 拒绝切换并在结果行给出原因（内置四个驱动，其余类型待驱动插件） | `connection_type_driver.rs::type_without_enabled_driver_is_refused` + `helpers.rs::type_has_driver_requires_enabled_driver` |
| 2（🟡） | **类型目录按 `enabled` 过滤**：`driver_store::get_data_source_types` 加 `WHERE enabled = 1`（与其文档语义一致） | 全工作区测试（engine 228 / workbench 81）无回归 |
| 3（🟡） | **项目下拉补「打开现有目录…」**：动作项顺序 = 「打开现有目录…」→ 末项「＋ 新增项目」（保持末项约定）；宿主置位 `project_open_request` → `project::ui::open_folder_dialog` | `connection_project_picker.rs`（5 项，含 `confirm_open_folder_requests_folder_dialog`） |
| 5（🟡） | **GP_ 快照同步**：新增 `DataSourceService::sync_snapshot_from_global(snapshot_id, project_path)`（配置 + 凭据密文一并复制；保留 ID / 创建时间 / 分组）+ 对话框 footer「从全局定义同步」（仅编辑 GP_ 时显示） | `data_source_lifecycle.rs::snapshot_sync_pulls_latest_global_definition`（含错误路径：非快照 ID / 缺项目路径 / 全局定义已删） |
| 新增（🔴） | **项目侧更新会清空密码（本轮排查发现并修复）**：`ProjectConnectionStore::update_connection` 改为 `password_encrypted = COALESCE(?9, password_encrypted)`，与全局库 update 语义一致 | `data_source_lifecycle.rs::project_update_keeps_password_when_blank` |

**已关闭（USIT 第 1 轮：常规 Tab 动态渲染）**

| # | 关闭方式 | 验证 |
| --- | --- | --- |
| 新增（🔴） | **文件型连接地址丢失（USIT 发现）**：`parse_url_host_port_db` 对 `sqlite`/`duckdb` 改为返回 `Some(路径)`（去 scheme / 修 Windows 三斜杠前导 `/`）；`build_effective_url` 文件型不注入凭据。旧实现下项目侧（P_/GP_）文件连接路径写不进去 → `build_connection_url` 报“文件型连接缺少数据库路径”、编辑回读地址为空 | `data_source_lifecycle.rs::file_db_path_survives_save_and_readback`（含全局 / 项目两侧 + 更新 + URL 还原） |
| 新增（🟡） | **常规 Tab 未随驱动动态渲染**：地址标签 / 占位随驱动推导（不再固定 `mysql://…`）；文件型只保留「连接设置（地址 + 打开/新建）+ 组织」；SSL 卡片按驱动声明；文件型落库前清洗凭据 / 网络 / TLS | `helpers.rs` 单测 2 项 + `connection_type_driver.rs::file_type_general_tab_renders_and_switches_back` |

| # | 级别 | 问题 | 影响 | 建议 |
| --- | --- | --- | --- | --- |
| 1 | 🔴 | ~~驱动目录只内置 4 个~~（**已关闭**：无驱动类型置灰不可选 + 结果行说明；驱动插件机制仍待平台排期） | 选中不可用类型会被拒绝；用户能在类型树直接看到「暂无驱动」 | 中期：接驱动安装（plugin）机制 |
| 2 | 🟡 | ~~类型树不按 `enabled` 过滤~~（**已关闭**） | — | — |
| 3 | 🟡 | ~~项目下拉无「打开现有目录」~~（**已关闭**） | — | — |
| 4 | 🟡 | 「＋ 新增项目」/「打开现有目录…」在编辑区**有脏草稿**时走「先关闭当前项目 → 回选择器」 | 多一步，且未直接弹目标对话框（原因：未保存确认是独立 alert 层，直接叠加会有层栈语义风险） | 项目侧新增 `PendingAction::{Create,OpenFolder}`，把“确认后继续”串进既有未保存确认 |
| 5 | 🟡 | ~~GP_ 快照与 G_ 定义无同步策略~~（**已关闭**：新增显式同步入口） | 语义明确为：快照=独立副本，仅显式同步时刷新 | 后续可选：同步时的差异预览 |
| 6 | 🟡 | ~~项目栏无「清除选择」~~（**已关闭**：改为「不需要项目（仅全局）」——切作用域而非留下无效空态） | — | — |
| 7 | 🟡 | 分组的新建 / 管理在 **database-nav 侧**，本模块只能勾选 | 导航侧分组管理未落地前，用户无法在 UI 创建分组（对话框只显示「暂无分组」） | 随 database-nav Phase B/C 排期 |
| 8 | ⚪ | ~~`DataSourceService::get`（只查全局库）仍是公开 API~~（**已关闭**：重命名为 `get_global`） | — | — |
| 9 | ⚪ | `view.rs` 的「＋ 新增项目 → `open_create_dialog`」宿主消费分支**无自动化测试**（含脏草稿分支） | 该路径只能手动验证 | 待 `WorkbenchView` 可测试化（需服务注入桥）后补窗口测试 |
| 10 | ⚪ | 原型 HTML 为手工维护的示意稿 | 与实现存在漂移风险（需人工同步） | 以 `connection-prototype-design.md` 为权威，HTML 仅作视觉参考；或后续从实现截图生成 |
| 11 | ⚪ | 类型树**不可折叠**（四个分类平铺） | 类型多时占用侧栏高度（靠内部滚动缓解） | 需要时改为可折叠分类（原型早期版本曾如此） |
| 12 | ⚪ | UI 尺寸常量化**只覆盖本模块**（`ui-constraints.md` 三阶段迁移第一阶段） | 其他模块仍写字面量 | 按 `ui-constraints.md` §迁移计划推进 |
| 13 | ⚪ | 缺 UI 图像回归基线 / 大数据量性能基准 / fuzz | 回归靠断言而非视觉 | 平台级排期 |

---

## 15. 数据来源审计（零 UI 造数据）

> 背景：用户提出“**保证都是从后台库里读写，没有前端自己造的数据**”。
> 本模块为本地桌面应用（GPUI 进程内直连 engine 服务 → SQLite / DuckDB），**不存在独立后端服务**：
> “前后端联通”在这套架构里 ＝ **UI 只经 `DataSourceService` / engine 商店读写本地库**（无 HTTP / IPC 层）。
> 因此本节把每个 UI 数据项逐一列出来源，并标注允许的例外（代码内 domain 枚举 / 标签字典 / 用户输入）。

### 15.1 允许的三类“非库数据”（非造数据）

| 类型 | 例子 | 为什么允许 |
| --- | --- | --- |
| **domain 枚举** | `ConnectionScope`（仅全局/项目/全局+项目）、`SSL_MODES`、`AUTH_TYPES`、`MAX_HOPS=4` | 协议/契约内的固定取值，与后端同一套语义（`url_params` 的 SSL 注入、`id_prefix` 前缀） |
| **标签字典** | 能力键 → 中文（`tree` → 数据库导航）、策略类型 → 中文（`security` → 安全策略）、分类名（关系型/文件型…）、地址占位文案（按驱动类型：文件型给 `.db/.sqlite` 等文件提示） | 只做“键 → 显示名 / 提示语”，不产生或补全业务取值；库里出现字典外的键时**原样展示**；网络型占位则由 `drivers.url_template` + `default_port` 拼出（不是字典） |
| **用户输入/派生值** | 名称、URI、主机/端口/数据库（从 URI 解析）、项目根、标签文本 | 本身就是用户输入；派生值由同一份输入算出（`parse_url_host_port_db`），非凭空构造 |

### 15.2 逐项来源表（本轮审计结果）

| UI 元素 | 数据来源 | 状态 |
| --- | --- | --- |
| 类型树（分类 / 类型 / 图标 / 可选性） | `data_source_types`（`enabled=1`）+ `drivers`（可用性判定） | ✅ |
| 驱动下拉（实现短名） | `drivers`（`type_id` 过滤 + `enabled`） | ✅ |
| 能力矩阵 | **`drivers.capabilities`（本轮修复）** | ✅（旧为硬编码 6 项） |
| 驱动属性初始行 | `drivers.driver_properties` / 连接记录 `driver_properties` | ✅ |
| 认证/网络/环境引用下拉 | `auth_configs` / `network_configs` / `environments` | ✅ |
| 环境策略摘要（高级 Tab） | `environment_policies.policy_config` | ✅ |
| 策略覆盖勾选项 | **`environment_policies`（选中环境的启用策略；本轮修复）** | ✅（旧为硬编码 6 项） |
| 环境管理器策略列表 / 落库类型 | **`environment_policies.policy_type`（本轮修复错标与假类型写入）** | ✅ |
| 项目下拉（项目名 + 路径） | `project::service::list_recent` + 当前会话（项目库/全局项目表） | ✅ |
| 分组勾选 | 项目库 `connection_groups` / `connection_group_members` | ✅ |
| 标签 | 连接记录 `tags` / `connection_tags` | ✅ |
| 暂存列表草稿 | `connection_drafts`（无密码列）+ 会话内快照 | ✅ |
| 暂存列表已保存条目 | `workspace_loader::load_connections_for_scope`（全局 + 项目库合并） | ✅ |
| 连接设置卡（网络型：主机/端口/数据库） | 从当前 URI 输入解析（用户输入派生）；**行的存在性与标签取 `drivers.config_schema.fields[]`**（未声明则不出该行；schema 为空才回退内置三行） | ✅ |
| 连接设置卡（文件型：地址） | 用户选择/输入的文件路径；系统选择器返回真实路径（新建时才创建空文件） | ✅ |
| 地址标签与输入占位 | 标签固定（文件型 = 地址 / 网络型 = URI，按用户要求不被 schema 覆盖）；网络型占位取 `drivers.url_template` + `default_port`（文件型走类型文案字典）；文件型地址行**占位**优先取 `config_schema` 的 `type=file` 字段 `placeholder` | ✅ |
| 结果行提示（保存 / 测试 / 同步） | 服务层真实返回（`DataSourceService::{save,update,test}` 等） | ✅ |
| 测试连接结果（版本 / 延迟） | 真实探测（`DataSourceService::test`） | ✅ |
| 模板导入导出（能力就绪） | 草稿快照（库 + 会话状态），不含密码 | ✅（UI 入口待接） |
| 驱动安装 `/install` | 占位错误（“待后续版本”） | ⚠️ 诚实地报不可用，**不造假数据** |

### 15.4 脏数据防护（写入侧守卫）

> 审计不只查“读得对不对”，也查“写下去的是不是脏的”。以下守卫都在**服务 / 存储层**，
> 与 UI 无关，任何入口（对话框、导航、脚本、将来插件）都会经过。

| 守卫 | 位置 | 防的是什么 | 验证 |
| --- | --- | --- | --- |
| 项目根预检（写路径报错） | `data_source_service::{save,update,delete,sync_snapshot_from_global}` | 把连接写进非项目目录 → `open()` 会顺手造出 `.RSmeta` 骨架（磁盘脏数据 / 半成品落库） | `data_source_lifecycle::project_scope_rejects_non_project_root` |
| 项目根预检（读路径降级） | `data_source_service::get_with_project` / `open_org_store` / `workspace_loader::load_project_connections` | 一次回读 / 列表刷新就在磁盘上建目录（读操作有副作用） | 同上 + `connection_scope_and_state::loader_degrades_on_non_project_root_without_creating_dirs` |
| 项目根判定容错 | `workspace_loader::is_project_root` | 大小写 / 前导点写错导致“真项目判成非项目”（本轮自己的回归就是它拦住的） | `workspace_loader` 内嵌单测（含 `.RSmeta` 为文件 / 普通目录 / 不存在） |
| 时间戳兜底 | `ProjectConnectionStore::{create,update}_connection` | 项目侧 `created_at` / `updated_at` 为空串 | `data_source_lifecycle::project_update_keeps_password_when_blank`（含时间戳断言） |
| 空密码保留原密文 | `ProjectConnectionStore::update_connection`（`COALESCE`） | 编辑一次（密码框留空）就把凭据清成 NULL | 同上 |
| 幻影条目清理 | `connection_dialog::staging_merge_saved` | 已删连接在暂存列表留下“已保存”空壳 | `connection_multi_save::staging_merge_prunes_phantom_and_keeps_drafts` |
| 分组与字段声明 | `helpers::{driver_form_fields, field_spec, address_field}` | 未声明字段被凭空渲染（“假行”）；schema 非法时造默认字段 | `helpers` 内嵌单测（真实种子 schema / 缺 key / 非法 JSON） |
| 草稿无密码 | `connection_drafts` 表结构（无 password 列） | 凭据落盘 | `connection_drafts_persist` |
| 旧值不硬套新语义 | 策略覆盖（`policy_type`）/ 草稿布尔数组 | 旧数据被误读成新格式 | 解析失败即忽略（`load_for_edit` / `row_to_draft`） |
| 文件型地址规范化 | `data_source_service::normalize_file_db_path` | 用户输入的 `sqlite://…` / 三斜杠 / 裸路径写法不一 → `database` 列存法不一致，回读与连接 URL 还原都对不上 | `data_source_service` 内嵌单测（4 种写法）+ `data_source_lifecycle::file_db_path_survives_save_and_readback` |
| 文件型输入清洗 | `connection_dialog::helpers::strip_file_db_noise` | 切类型后残留的凭据 / 网络链 / TLS 被写进文件型连接 | `helpers` 内嵌单测（凭据·网络·TLS 清空，策略覆盖保留，非法 JSON 不静默丢） |

### 15.3 约束与回归手段

- **新代码规则**：新增 UI 数据项前先回答“它来自哪张表 / 哪个服务方法”；只能从字典来的东西（标签）不得携带取值。
- **回归手段**：字典与解析函数均有单测（`helpers.rs`）；按库读取的服务方法有集成测试（`data_source_lifecycle.rs`）；渲染层不产生业务值，因此不需要图像基线也能拦住“造数据”类回归。
- **待办**：驱动安装能力（§14 #1 的中期项）落地后，`drivers` 目录会真实增长，类型树的可用性判定无需改动即生效。
