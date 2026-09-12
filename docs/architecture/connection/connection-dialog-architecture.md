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
| 视图 | ├ `staging.rs` | `ConnectionDraft`、暂存状态机、持久化映射（`draft_to_row` / `row_to_draft`）、来源短码 |
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

- 快照字段 = Header + 五 Tab 的全部可编辑状态（`type_id` 数据库类型、`driver_id` 驱动 id、`driver_name` 实现短名、URL、凭据、作用域与项目路径、SSL、驱动属性、策略覆盖、三类引用）。
- **内存 + 跨会话持久化**：关闭对话框不丢失（状态挂在 `EditorPanel` 的对话框句柄上）；变更与关闭时写入 global.db 的 `connection_drafts` 表（迁移 `020`），重启后首次打开自动恢复。
- **凭据安全边界**：持久化表**不含密码列**（`ConnectionDraftRow` 无 password 字段，恢复后密码框为空），只随正式保存写入连接库（AES-256-GCM）。

### 3.6 元数据缓存身份指纹（规则已冻结，未接线）

**问题**：L2 元数据缓存按**连接 ID** 分文件（`conn_{id}.sqlite`），指向同一物理库的多条连接（改名 / 改密码 / 换驱动实现 / 加 SSL 参数）各自重建缓存、反复预热。

**身份**（`engine::persistence::metadata_identity`，纯函数、无 I/O、无 FS 访问）：

| 组成 | 取值 | 理由 |
| --- | --- | --- |
| 数据库**族** `type_id` | `data_source_types.id`（`mysql` / `postgres` / `sqlite` / `duckdb`） | 同库多驱动内省的是同一台库；驱动实现 id（`mysql_native`）**不进身份**，差异属“对象覆盖面”，由能力掩码在命中时校验 |
| 规范化地址 | 网络型 `net:{host}:{port}/{database}/{schema}`：主机小写、IPv6 去括号后统一补 `[]`、端口空 → 驱动默认端口、库名与 schema **不折叠**（大小写敏感）；文件型 `file:{规范化路径}`：去 `file:`/`sqlite:`/`duckdb:` scheme、去查询串与片段、`\`→`/`、折叠重复分隔符、去 Windows 三斜杠残留、Windows 下折叠大小写 | `h/db` 与 `h:3306/db` 必须同身份；`?mode=ro` / `?sslmode=require` / 超时等只影响“怎么连”，不改变“是哪个库” |
| 主体 `principal` | 用户名（优先）→ 认证档案 id → 认证类型 → `-` | 权限决定**可见对象集合**（`information_schema` 过滤 / schema 可见性 / 行级安全）：跨主体共享会让窄权限用户的缓存污染宽权限用户的树。密码与密钥**绝不进身份**（会进文件名与日志，且轮换即整份失效） |
| 格式版本 `format_version` | 当前 `CACHE_FORMAT_VERSION = 1` | 缓存结构变更时 +1 → 指纹变化 → 旧缓存自然分池（按约定不删除） |

- 规范化串（可读，供索引展示与排障）：`v1|mysql|net:db.internal:3306/orders/-|user:app_ro`。
- 指纹 = `sha256(规范化串)` 前 16 位十六进制（64 bit）；文件名 `meta_{fp}.sqlite`。索引表同时保存规范化串，命中时校验，不一致按“不可复用”处理（防碰撞）。
- **不可共享**（`fingerprint() == None`）：主机 / 路径为空、`:memory:` / `file::memory:` 等内存库。
- 指纹只作**缓存键**，不作连接主键与唯一性约束（连接 ID 规则见 `id_prefix`，两者职责正交）。

**适用边界（反例，满足不了就不能依赖共享）**：

- 身份把 `schema` 静态写进键——仅适用于“schema 固定在连接配置里”的模型；将来支持会话级 `search_path` / `currentSchema` 时，schema 必须随查询携带（或切换身份），否则会命中其它命名空间的缓存。
- 同一 `host:port` 背后是动态路由到不同后端（代理 / 负载均衡）时会误共享；兜底是在 `driver_properties` 提供显式 `identity_salt`（人工声明“这是独立目标”），或对该连接关闭共享。
- 若缓存将来包含与权限强相关或敏感的结构（行级采样、DBA 专属对象），键需增加“可见性级别”维度，否则不能跨主体共享。
- 两级共享（`target 级`（不含 principal）共享 catalog / schema 清单，`principal 级`单独存表 / 列等权限相关层）理论上能再省一半预热，但需要 L2 分层，**暂不做**：待出现“同库多账号”的真实用例再评估（观察项，不在 §14 计数内）。

**落地状态**：规则与纯函数已就绪（13 项单测），**路径仍按连接 ID**——切换与导航接入 L2 同轮进行（避免两套键并存的过渡期）。切换时需一并落地：

| 配套 | 内容 |
| --- | --- |
| 索引表 `metadata_cache_index` | `fingerprint`(PK) / `canonical_desc`(可读串) / `type_id` / `format_version` / `ref_conn_ids`(引用计数) / `last_used_at` / `size_bytes`：给「缓存管理」提供占用与孤儿视图，并让指纹**可读** |
| 引用计数与孤儿 | 连接删除只减引用，引用为 0 标记孤儿、**不自动删**（与“缓存只增不删”一致，唯一删除入口仍是「缓存管理」） |
| 并发预热 | 同一指纹被两条连接同时预热 → 进程内 per-fingerprint 互斥 + SQLite WAL / `busy_timeout`，避免写出半份缓存 |
| 存量迁移 | 旧 `conn_*.sqlite` 全部按 legacy 保留；需要时按需复制（copy，不 move）到指纹路径 |

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
- 对话框**内部**状态刷新（切 Tab、暂存切换、测试结果）走 `EditorPanel` 的 notify，由 `WorkbenchView` 的 `cx.observe` 级联到宿主。
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
    D->>D: collect：校验（名称 / 驱动 / URL）
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
    编辑中 --> 编辑中: 切 Tab / 引用档案（面板 notify → 宿主级联）
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
| 58 | **连接设置字段可编辑 + 字段 ⇄ URI 双向同步**（网络型）：主机 / 端口 / 数据库为 `Input`，与 Header URI 双向同步（任一侧改动同步另一侧）；字段→URI 用 `connection::url_params::rewrite_url_authority`（scheme 无关，保留凭据 / 查询串；端口空→驱动默认端口；数据库空→去路径；主机空→不重建）；同步状态存 `fields_synced_for: (驱动 id, URI)`，渲染层每帧只执行一个方向（避免循环） | 真机反馈“连接设置无法输入”：上一版把三字段做成**只读摘要**（“编辑在 Header URI”）与用户预期不符——工业级连接管理器（DataGrip / TablePlus）都允许直接改主机/端口/库，URI 与字段互为体现。新增 `rewrite_url_authority` 是因为既有 `rewrite_url_host_port` 只认 mysql/postgres 两个协议且强制重写端口 |
| 59 | **结构尺寸统一登记到 `crates/workbench/src/ui.rs`**（`DIALOG_*` 一组），对话框不再自带一套尺寸常量；间距改用工作台统一 `GAP_SM/MD/LG`（`GAP_SM` 由 0.375rem 对齐为全局的 0.25rem），圆角用 `theme.radius` | 用户侧新增全局 UI 规范（`rds-ui-spec` skill + `ui-design-spec.md` + `ui_contract` 契约测试）要求“结构尺寸先进 `ui.rs` 登记、局部间距用 Tailwind 尺度、视图不得写裸尺寸”——对话框不能继续做一个尺寸孤岛 |
| 60 | **启动时注册内置驱动**：`engine::migration::initialize_global_system()` 首行调 `AutoDriverRegistrar::auto_register()`（幂等：`HashMap::insert`） | 🔴 真机「测试连接」报 `CONN_DRIVER_NOT_FOUND: Driver 'sqlite' not found in registry`——全仓搜下来 **只有测试里调过注册**，应用启动路径根本没注册过 `DriverRegistry`；这不只影响测试连接，**导航树连接、重连也都会失败**（同一个注册表） |
| 61 | **文件型工厂改从 `to_url()` 取地址**：`SqliteDriverFactory` / `DuckDbDriverFactory` 先 `config.to_url()`（尊重 `url_override`：服务层 / 对话框传的就是它），再去 `sqlite://` 前缀与查询串；回退顺序 `url_override → database → file_path` | 网络型工厂一直用 `config.to_url()`，只有文件型工厂直接读 `config.database` → 服务层传了 `url_override` 也会报「Database path is required for SQLite」（修完 #60 后用户下一次点击就会撞上）；查询串（driver_properties 追加）不能拼进文件名 |
| 62 | **暂存条目“当前项”显示正在编辑的表单**：`staging_display_type_id(草稿类型, live 类型)`——光标位条目用表单快照，其余用已存草稿；名称与脏标记同源（同一份 live 快照） | 真机反馈：选 SQLite 后条目仍显示 mysql 图标——草稿只在“切条目 / 保存 / 关闭”时写回，显示层不能等写回；同时删掉重复计算快照的 `draft_dirty`（脏标记改用同一份 live） |
| 63 | **元数据缓存身份指纹** = 数据库族 + 规范化地址 + principal + 缓存格式版本；**驱动实现 id 不进身份** | 缓存按连接 ID 分文件时，改名 / 改密 / 换驱动 / 调连接参数都会整份重建；指纹让同一物理库共享 L2。驱动差异属“对象覆盖面”（能力掩码校验），不是身份差异。规则与单测先冻结（`metadata_identity`），路径切换另轮 |
| 64 | **principal 进身份，密码与密钥绝不进** | 权限决定可见对象集合（`information_schema` 过滤 / schema 可见性 / 行级安全）：跨主体共享会让窄权限缓存污染宽权限视图；密码进身份则轮换即失效，且会进文件名与日志 |
| 65 | 指纹只作**缓存键**，不作连接主键 / 唯一性约束 | 与连接 ID 职责正交：主键要长期稳定（不可变），缓存键要可再生、可丢失、可校验碰撞——混用会重演“一个字符串承担四种职责”的问题 |
| 66 | 指纹切换时机 = **导航接入 L2 的那一轮**（本轮只落规则与纯函数） | 若先按连接 ID 铺开再换指纹，同一段编排要改两遍，并留下两套键并存的过渡期；同时索引表 / 引用计数 / 并发互斥都是 L2 编排的一部分 |
| 67 | **驱动派生数据按（驱动 id + 声明原文）缓存**（`DriverDerived`：表单字段 / 能力 / 认证方法），渲染期只克隆已解析结果；**地址占位只在真正变化时写入** | 旧实现每帧 `driver_form_fields` / `driver_capabilities` / `driver_auth_types` 重新解析 `config_schema` / `capabilities` / `supported_auth_types`（对同一份字符串反复反序列化）；而 `InputState::set_placeholder` 在 gpui-base 里是**无条件赋值 + `cx.notify()`**（`input/base/state.rs`）→ 每帧写占位会让地址输入框每帧重绘。缓存键取声明原文而非“驱动 id”，所以驱动目录刷新 / 声明变化会自动失效重算；与 `fields_synced_for` 同一约定：刷新点仍在 `render`（权威同步点） |
| 68 | **作用域判定单一来源**：`id_prefix::{is_global_connection, uses_project_storage}`（`G_` 与遗留 `conn-` → 全局库；`P_`/`GP_` → 项目库），服务层 3 处与 `nav_runtime` 4 处全部改调它，不再各自写前缀推导 | 历史隐患：服务层把遗留 `conn-` 视作全局，而 `nav_runtime` / `database::NavSource::from_conn_id` 把它归为项目 → 同一连接的标签 / 导航状态可能写错库；“同一事实两处实现”是这类缺陷的温床，谓词收归 engine 后 M4 也能直接复用 |
| 69 | **隧道注册表挂在 `ConnectionManager`**（`ConnectionManager::tunnels()`）而不是 `ConnectionService` 实例字段；`ConnectionService::new` 从 manager 取，传独立管理器的测试仍天然隔离 | 生产连接入口是短生命周期的（每次调用 `new` 一个服务）。守卫存实例字段时，隧道会随建立它的临时实例释放——即使传了 `network_method`，也会“刚建好就关掉” |
| 70 | **网络配置类型键归一化**：`parse_network_config_json` 先 `trim().to_ascii_lowercase()` 再匹配，并接受 `ssh_tunnel` / `tls` / `http` / `socks_proxy` 等别名；对话框新增 `NETWORK_TYPES = [ssh, proxy, ssl, chain]`（**规范键**）并在打开管理器时按类填充类型下拉、写库前白名单校验 | UI 写入的是 `SSH` / `Proxy` 这类大写标签，而解析器只匹配小写 → 档案存在、连接也引用了，但**整条链静默不生效**（审计 #20 第二层根因）；管理器类型下拉此前借用认证选项（`password`/`ssh_key`/`proxy_pwd`），会把错值写进 `network_type`（第三层） |
| 71 | **网络配置改为结构化字段表单**（`ssh` / `proxy` / `socks` / `ssl`）：字段声明 + JSON 组装/校验/回填全部是纯函数（`helpers::{network_field_specs, build_network_config_json, network_config_values}`），渲染层只摆输入框；`chain` 仍走原始 JSON | 原先只有一个「数据(JSON)」文本框，用户要手写 `SshConfig` / `ProxyConfig` 的 JSON；且**编辑时只回填名称**，保存会把 config 覆盖成空。字段与 `connection::config` 的 serde 模型对齐（单测直接反序列化验证），校验必填 / 端口 / 布尔与 SSH 认证二选一，错误写结果行不落库 |
| 72 | **撤下内联协议链 UI**（`Hop` 占位模型 / 上移下移 / 拓扑预览 / `advanced_options.network_chain` 写入全部删除）；多跳统一走**类型 `chain` 的网络档案**（管理器中填 JSON 数组，连接入口解析为 `ConnectionMethod::Chain` 并逐跳执行）；网络 Tab 只留「引用下拉 + 管理入口 + 诚实提示 + 数据路径预览」 | 内联链的 `Hop` 只有 `kind/label/enabled`，**没有任何主机与凭据字段，根本无法执行**；它既造出“配了就该生效”的假象，又在初始状态里种了两条假数据（`跳板机·prod-gw` / `公司代理·http`，属于 §15 禁的 UI 造数据）。而同样能力已由档案路径完整提供（含多跳），所以是删除而非补齐；`connection_drafts.hops_json` 列保留（不迁移 schema），固定写 `[]`，旧草稿的占位链直接忽略 |
| 73 | **暂存列表显示与脏比对不再构造整份 `ConnectionDraft`**：新增 `LiveEntryView`（名称 / 类型 / 脏标记）与 `form_matches_draft`（逐字段、无分配比较），`render` 里整表 `Vec<ConnectionDraft>` 克隆改为逐行短借用；`InputState::value()` 返回 `SharedString`（引用计数克隆）是「无分配比较」成立的前提 | 旧实现每帧要：克隆整张草稿表（N × ~40 字段）+ 构造一份完整快照做脏比对（~40 次 `to_string` + 两个 `Vec` 克隆）。现在每帧只剩：光标位 2 个短字符串 + 每行 3 个展示字段。代价是“表单字段集合”与 `snapshot_form` 出现两处定义，靠等价性测试（`connection_staging::form_matches_draft_agrees_with_snapshot`）锁定不漂移 |
| 74 | **测试连接与真实连接同源**：`ConnectionService::{inject_auth_config_credentials, build_probe_config}` 成为**唯一**的“档案 → 建连参数”组装点（`connect` 与测试共用）；测试连接按同一规则注入认证档案凭据、**真实建立网络档案隧道**（探测结束即 drop 守卫）、应用驱动属性 / 高级选项；档案缺失 / 读取失败 / 注入失败必须产生**可见说明**（不允许静默）；`DataSourceService::test(input, project_path)` 增加项目根入参以解析 P_/GP_ 档案；`url_params::merge_credentials` 统一「字段凭据 → URL」的写法（userinfo 百分号转义） | 旧 `test` 只吃表单字段：引用认证档案时 UI 已把用户名 / 密码换成只读说明 → **测试必然缺凭据**（假失败；宽松库还会假成功）；引用 SSH / 代理档案也不走隧道 → 测试结论与真实连接相反。同源后「测试通过」与「连接能建立」共用同一条组装路径，差异只剩“不注册连接池 / 不落库”；userinfo 转义顺带修掉「密码含 `@` 把 host 截断」的隐患 |
| 75 | **档案引用完整性与严格模式（A2）**：① 管理器列表显示**被引用计数**（`ReferenceCount{global,project}`，`count_references_batch` 一次取齐）、删除被引用的认证 / 网络 / 环境配置被拦下（`DataSourceService::ensure_no_references`，消息含引用数与范围）；② 引用的档案不存在 / 读不出 / 注入失败一律**报错**（`inject_auth_config_credentials` 返回 `Result`），不再回退“直连 + 无凭据”；③ 引用了网络档案但解析不出连接方式（被删 / 类型未知 / 内容非法）时，`connect` 与测试连接都拒绝静默直连 | 静默降级是安全缺陷：用户以为走跳板机 / 专用账号，实际走公网直连；引用计数与拦截把“删档案”的后果在执行前说清。计数范围 = 全局库 + 当前打开项目（未打开项目的引用不可见，已登记为 #33），所以拦截是“尽力而为”，连接时的显式报错是兼底 |


---

## 7. 测试策略

| 层次 | 文件 | 覆盖 |
| --- | --- | --- |
| 传输单测 | `crates/connection/src/*`（含 `chain.rs`） | URL 处理族 / 参数注入 / 隧道注册表 / Secret（含 `rewrite_url_authority`：scheme 无关改写、凭据与查询串保留、端口/数据库边界） |
| 传输集成 | `crates/connection/tests/tunnel_roundtrip.rs` | SOCKS5 / HTTP CONNECT / 两跳链真实数据往返 + 守卫释放关闭 |
| 服务层 | `data_source_lifecycle.rs` | 保存 / 回读 / 更新 / 删除 / 同名拦截 / 作用域预检 / tags 同步 / **空密码更新保留原密文** / **项目侧回读（`get_with_project`）** / **GP_ 快照同步（含错误路径）** / **导航入口项目侧解析** / **环境策略按环境名读库** / **文件型路径落 `database`（全局 + 项目两侧；更新不清空；可还原连接 URL）** / **驱动目录 id 能在 DriverRegistry 解析 + SQLite 真实文件测试连接（建库 + 版本探测）** |
| 服务层 | `real_connections.rs` / `connection_scope_and_state.rs` / `global_service_singleton.rs` | 加载器契约 / 可见性与运行态 / 单例生产路径 |
| 服务层 | `connection_tunnel_cleanup.rs` | 连接失败后隧道回滚（`tunnel_count == 0`） |
| 窗口 | `connection_dialog_ui.rs` | 打开 / 渲染 / 关闭、五 Tab、编辑入口、状态保留、重入不叠加 |
| 窗口 | `dialog_host_layer.rs` | 入口调起（`debug_bounds("dialog-layer")`）、关闭移除层、面板 notify 级联 |
| 窗口 | `connection_staging.rs` | 暂存：切换保留字段 / 删至最后补位 / 保存后转正式补位 / 已保存不参与删除 |
| 窗口 | `connection_drafts_persist.rs` | 跨会话恢复：变更落库 → 新状态恢复草稿与表单；**密码不落库**（恢复后为空） |
| 窗口 | `connection_type_driver.rs` | 类型 × 驱动两层选择：选类型→下拉切到该类型启用驱动并默认选中（短名）；按驱动 id 回读（跨类型同名短名不歧义）；快照携带 `type_id` / `driver_id`；**无可用驱动的类型被拒绝并给出原因**；**文件型与网络型常规 Tab 互切渲染不 panic**（占位 / 分组集合随驱动重建）；**分组折叠态切换后重渲染**；**主机/端口/数据库 ↔ URI 双向同步（含幂等）**；**地址占位按驱动推导且只在变化时写入（`url_placeholder_for` 缓存与输入框实际占位一致）** |
| 窗口 | `connection_project_picker.rs` | 项目下拉：会话项目置顶 + 选中（默认选当前项目、项目根写回路径）/ 末项 `＋ 新增项目` 在选项中 / 确认「新增项目」→ 置位 `project_new_request` 并清空选中 / 确认普通项目 → 路径写回 / 空确认无副作用 / 下拉项搜索与 `path`·`is_new` 契约（宿主走生产入口 `request_new_connection`） |
| 服务层 | `data_source_lifecycle.rs::nav_runtime_resolves_project_connection_with_project_path` | 导航入口项目侧解析：带项目根可解析（作用域回推为“仅项目”）、无项目根报「数据源不存在」 |
| 单测 | `connection_dialog/helpers.rs`（内嵌） | `driver_short_name` 括号提取与回退 / `find_driver_by_value` 三路匹配 / 类型过滤 / 类型徽标 emoji 回退 / `type_has_driver` / **能力 JSON 解析与矩阵（字典外键保留）** / **策略类型↔标签往返与配置摘要（不造值）** / **地址标签与占位随驱动推导（url_template 示例值 / 文件型提示）** / **文件型输入清洗（凭据·网络·TLS 不落库，策略覆盖保留）** / **`config_schema.fields` 解析（存在性·标签·type、缺失与非法输入不造字段）** |
| 窗口+服务 | `connection_multi_save.rs` | 连续保存两条连接（单例临时库）：暂存列表转正式 + 补空草稿；库中两条可读回 |
| 存储单测 | `engine::persistence::connection_draft_store`（内嵌） | 行序 roundtrip / 全量替换语义 / 表无 password 列（安全约定） |
| 存储单测 | `engine::persistence::metadata_identity`（内嵌，13 项） | 身份指纹：默认端口归一（`h/db` ≡ `h:3306/db`）/ 主机小写折叠而库名与 principal 不折叠 / IPv6 括号归一 / 类型族与格式版本改变身份 / 空地址与内存库不共享 / 文件路径跨 scheme·分隔符·查询串归一 / UNC 保留前导 `//` / 指纹形状与 `meta_{fp}.sqlite` 约定 |
| 存储单测 | `engine::persistence::id_prefix`（新增 1 项） | 作用域判定单一来源：`G_` 与遗留 `conn-` → 全局库；`P_`/`GP_` → 项目库（`is_global_connection` / `uses_project_storage`） |
| 服务层 | `data_source_lifecycle.rs::snapshot_sync_pulls_latest_global_definition`（扩展） | 快照同步除配置 / 凭据外，**标签权威表（`connection_tags`）同步更新**；同步前项目侧保持旧值（快照=独立副本） |
| 服务层 | `data_source_lifecycle.rs::global_delete_cleans_project_group_membership` | 全局连接加入项目分组后删除 → 项目库成员关系清理、分组定义保留（防 B3 幻影成员） |
| 服务层 | `data_source_lifecycle.rs::referenced_network_profile_reaches_connect_request` | 网络配置档案（类型用 UI 实际会写的 `Proxy` 大写）→ `resolve_network_method_with_project` 可解析 → `build_connect_request` 带出 `network_method` + `P_` 路由到项目侧（修「配了跳闸机却直连」） |
| 服务单测 | `connection_service`（内嵌 +2） | 同一管理器下的服务实例共用隧道注册表（独立管理器保持隔离）；网络配置类型键大小写 / 别名归一（未知类型不 panic 返回 None） |
| 单测 | `connection_dialog/helpers.rs`（内嵌 +4） | 网络配置字段：类型→字段声明覆盖（含大写 / 别名；`chain` 走 JSON）/ 组装的 JSON **可被 `connection::config` 的 serde 模型直接反序列化**（proxy·ssh 密码·ssh 私钥·ssl）/ 必填与端口与布尔校验 + SSH 认证二选一 / 编辑回填往返（非法 JSON 不反填） |
| 单测 | `connection/src/url_params.rs`（内嵌 +1） | `merge_credentials`：普通凭据与既有写法等价 / `p@ss:w/rd` 转义（`%40` `%3A` `%2F`）/ 仅密码（`:p%20wd@`）/ 已有 userinfo 原样 / 非 URL（文件路径）原样 |
| 服务层 | `data_source_lifecycle.rs::probe_config_applies_referenced_auth_profile`（A1） | 测试配置与真实连接同源：引用存在的认证档案 → 凭据（解密后）注入 URL 并回填字段凭据（`config.username` / `password`）+ 说明含「已应用认证档案凭据」；引用不存在的档案 → URL 原样且说明含「未找到引用的认证档案」（不静默） |
| 服务层 | `data_source_lifecycle.rs::probe_config_applies_referenced_network_profile`（A1/A2） | 网络档案在测试连接时**真实建隧道**：不可达 SSH 跳板（`127.0.0.1:1`）→ `build_probe_config` 返回 Err（「网络档案应用失败」）；**类型未知的档案 → 同样失败**（不因解析不了而退回直连） |
| 服务层 | `data_source_lifecycle.rs::manager_reference_count_and_delete_guard`（A2） | 引用计数：无引用为 0 / 空 id 不误报 / 全局 1 条与项目 1 条分别可见 / 未打开项目时项目侧为 0；删除守卫：被引用的认证与网络配置返回 Err 且消息含范围（「全局 1 条、当前项目 1 条」） |
| 单测 | `connection_service::referenced_network_profile_without_method_is_rejected`（A2） | 引用了网络档案但未解析出连接方式 → `connect_with_type` 直接 Err（在 `apply_network_method` 之前拦下，不建隧道、不碰数据库） |

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
| 35 | **连接设置可编辑 + 字段⇄URI 双向同步（USIT 第 4 轮）**：主机/端口/数据库 改 `Input`；`connection::url_params::rewrite_url_authority`（scheme 无关，保留凭据/查询串）+ `data_source_service::rebuild_url_from_fields`（幂等、主机空不重建、端口空用默认端口）；渲染层每帧单方向同步（`fields_synced_for` 防循环）；结构尺寸登记到 `ui.rs`（`DIALOG_*`）并对齐全局间距/圆角方案 | `connection/src/url_params.rs`、`services/data_source_service.rs`、`connection_dialog/{helpers,state,render,mod}.rs`、`ui.rs`（决策 #58、#59）；测试：URL 改写 1 项 + 重建 1 项 + 窗口双向同步 1 项（104 项全绿） |
| 36 | **驱动注册与文件型取址修复 + 暂存条目显示同步（USIT 第 5 轮）**：① `initialize_global_system` 首行注册内置驱动（修 `CONN_DRIVER_NOT_FOUND`，同时修好了导航连接 / 重连）；② sqlite / duckdb 工厂改从 `to_url()`（url_override）取地址（修「Database path is required」）、去前缀与查询串；③ 暂存条目当前项显示 live 表单类型/名称（修“表单已 SQLite、条目还显示 mysql 图标”），删掉重复算快照的 `draft_dirty` | `engine/{migration/global_init.rs, driver/factory.rs, driver/auto_register.rs}`、`connection_dialog/{helpers,render,staging}.rs`（决策 #60–#62）；测试：engine `auto_register` 内置 id 断言、`data_source_lifecycle::catalog_drivers_resolve_and_sqlite_probe_succeeds`（真实文件探测）、`helpers::staging_type_badge_prefers_live_form_for_current_entry` |
| 37 | **元数据缓存身份指纹（规则冻结，未接线）**：新增 `engine::persistence::metadata_identity`（纯函数 + 13 项单测）——身份 = 数据库族 + 规范化地址 + principal + 格式版本；驱动实现 / 密码 / 连接参数不进身份；`fingerprint()` = sha256 前 16 hex、`cache_file_name()` = `meta_{fp}.sqlite` | `crates/engine/src/persistence/metadata_identity.rs`（决策 #63–#66）、文档 §3.6；**路径未切换**（仍 `conn_{id}.sqlite`），索引表与并发互斥待导航接入 L2 时落地 |
| 38 | **渲染热路径收敛（第一批，§14 #16）**：① 驱动派生数据（表单字段 / 能力 / 认证方法）改 `DriverDerived` 缓存（键 = 驱动 id + 三份声明原文），不再每帧解析声明 JSON；② 地址占位改「变化才写」（复用已有 `url_placeholder_for` 字段做守卫）—— `set_placeholder` 是无条件赋值 + notify | `connection_dialog/{helpers.rs（DriverDerived）,state.rs（driver_derived）,mod.rs,render.rs}`（决策 #67）；测试：`helpers::driver_derived_parses_declarations_and_keys_on_them`、`connection_type_driver::address_placeholder_is_cached_and_follows_driver`；**遗留**：类型 / 驱动目录每帧深拷贝与 `snapshot_form` 脏比对仍待收敛（#16 后半） |
| 39 | **M3↔M4 契约审计修复（本轮）**：① 快照同步补标签权威表（`connection_tags`）；② 删除全局连接时清理项目侧分组成员（项目根合法时）；③ 作用域判定收归 `id_prefix::{is_global_connection, uses_project_storage}`（服务层 3 处 + `nav_runtime` 4 处，遗留 `conn-` 归全局库）；④ `nav_runtime::rename_group` 适配 engine `update_group` 新增的 `sort_order` 参数（保留库中现值） | `engine/persistence/id_prefix.rs`、`services/{data_source_service.rs,nav_runtime.rs}`（决策 #68）；测试见 §7；审计结论与 M4 侧待办见 **§16** |
| 40 | **网络配置真正生效（审计 #20 收口）**：① `nav_runtime::connect_entry` 解析引用的网络档案 → `ConnectionMethod` 并随 `ConnectRequest` 传出（新增可测纯函数 `build_connect_request`）；② 隧道注册表改挂 `ConnectionManager`（决策 #69），修「守卫随临时服务实例释放」；③ 类型键归一化 + 对话框 `NETWORK_TYPES` 规范键 + 管理器类型下拉按类填充（决策 #70） | `engine/connection_manager.rs`、`connection/chain.rs`（`shares_with`）、`services/{nav_runtime.rs,connection_service.rs}`、`connection_dialog/{mod.rs,managers.rs}`；测试：`referenced_network_profile_reaches_connect_request` + `connection_service` 内嵌 2 项 |
| 41 | **网络配置结构化字段表单（审计 #24 表单部分收口）**：新增字段声明 + 纯函数 JSON 组装 / 校验 / 回填（`helpers::{network_field_specs, build_network_config_json, network_config_values}`）；管理器按类型展开真实字段（`ssh`：主机/端口/用户名/密码或私钥/目标主机与端口；`proxy`/`socks`：主机/端口/认证/直连主机；`ssl`：校验证书 + 三个路径），`chain` 仍走原始 JSON；编辑时回填真实字段（此前只回填名称 → 保存会把 config 覆盖成空） | `connection_dialog/{helpers.rs,mod.rs,state.rs,managers.rs,render.rs}`（决策 #71）；测试：`helpers` 内嵌 +4 |
| 42 | **撤下内联协议链（审计 #25 关闭）**：删除 `Hop` 模型 / 链列表 UI（上移下移启用删除）/ 添加入口 / 拓扑预览 / `advanced_options.network_chain` 写入 / `hops_valid`；网络 Tab 改为「引用下拉 + 管理入口 + 诚实提示 + 数据路径预览（本机 → 档案/直连 → 目标数据库）」；多跳走 `chain` 类型档案（JSON 数组）；同时移除初始状态里的两条假跳数据（§15 零 UI 造数据） | `connection_dialog/{render.rs,state.rs,staging.rs,mod.rs}`（决策 #72）；`connection_drafts.hops_json` 列保留固定写 `[]`；测试：工作台 33 lib + 各连接套件全绿 |
| 43 | **暂存列表热路径收敛（§14 #16 后半关闭）**：`LiveEntryView` + `form_matches_draft`（逐字段无分配比较）；整表草稿克隆 → 逐行短借用；脏标记与显示名/类型徽标改为读「表单显示视图」 | `connection_dialog/{staging.rs,render.rs}`（决策 #73）；测试：`connection_staging::form_matches_draft_agrees_with_snapshot`（等价性 + 脏标记 + 越界 None） |
| 44 | **测试连接与真实连接同源（审计 A1，§14 #26 关闭）**：`ConnectionService::{inject_auth_config_credentials, build_probe_config}`（认证档案凭据 + 真实隧道 + 高级选项 / 驱动属性，`connect` 与测试共用）；`DataSourceService::test(input, project_path)`；`url_params::merge_credentials`（userinfo 转义）；`load_auth_data_from_db` 增加库入参（测试可注入临时库） | `services/{connection_service.rs,data_source_service.rs}`、`connection/src/url_params.rs`、`connection_dialog/render.rs`（决策 #74）；测试：`data_source_lifecycle` +2、`url_params` +1；工作区 check 零警告 |
| 45 | **档案引用完整性与严格模式（审计 A2，§14 #27 部分关闭）**：管理器列表显示被引用计数（批量查询）；删除被引用档案被拦下（`ensure_no_references`）；档案缺失 / 网络档案无法解析 → `connect` 与测试连接双双报错（不再静默无凭据直连） | `services/data_source_service.rs`（`ReferenceField`/`ReferenceCount`/`count_references*`/`ensure_no_references`）、`services/connection_service.rs`（`inject_auth_config_credentials` 返回 `Result` + 网络守卫）、`connection_dialog/{managers.rs,mod.rs,state.rs}`（决策 #75）；测试：`data_source_lifecycle` +1、`connection_service` 内嵌 +1 |


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
> **A1 轮（同日）**：关闭 #26（测试连接与真实连接同源）；新增 #27–#32（档案引用完整性 / 结果行分级 / 首次使用引导 / auth_data 字段化 / 标签单源 / 连接 ID 命名待拍板）。
> **A2 轮（同日）**：#27 部分关闭（引用计数 + 删除拦截 + 档案缺失显式报错）；残留（跨项目全量引用扫描）登记为 #33。

**已关闭（A2 轮，2026-09-12：档案引用完整性与严格模式）**

| # | 关闭方式 | 验证 |
| --- | --- | --- |
| 27（🟡，部分关闭） | **档案引用完整性**：① 管理器列表显示**被引用计数**（`count_references_batch`，全局 + 项目一次取齐）；② 删除被引用的认证 / 网络 / 环境配置被拦下，消息含引用数与范围（原型 §3.6「被引用的配置不可删除」）；③ 引用的认证 / 网络档案缺失或无法解析时，`connect` 与测试连接**双双报错**（不再静默回退「直连 + 无凭据」）。**残留**：计数只覆盖全局库 + 当前打开项目（未打开项目里的引用不可见），跨项目全量扫描与“引用方列表”UI 见新 #33 | `manager_reference_count_and_delete_guard`、`referenced_network_profile_without_method_is_rejected`、`probe_config_applies_referenced_{auth,network}_profile` |

**已关闭（A1 轮，2026-09-12：测试连接与真实连接同源）**

| # | 关闭方式 | 验证 |
| --- | --- | --- |
| 26（🔴） | **测试连接忽略认证 / 网络档案**：`test` 只看表单字段，而引用档案时 UI 已把用户名 / 密码换成只读说明 → 缺凭据（假失败；宽松库假成功）、不走隧道（与真实连接结论相反）。现抽 `ConnectionService::{inject_auth_config_credentials, build_probe_config}`（`connect` 与测试共用）：按 `auth_method`（缺失回退档案 `auth_type`）注入凭据 → **真实建立隧道**（探测结束 drop 守卫）→ 应用驱动属性 / 高级选项；档案缺失 / 读取失败 / 注入失败全部产生**可见说明**（不再静默）；`test(input, project_path)` 增加项目根入参（P_/GP_ 档案才能解析）；`url_params::merge_credentials` 统一「字段凭据 → URL」写法（userinfo 转义，修「密码含 `@` 截断 host」） | `probe_config_applies_referenced_auth_profile`（凭据进 URL + 回填字段 + 档案缺失可见）、`probe_config_applies_referenced_network_profile`（不可达 SSH → 直接失败）、`url_params::merge_credentials` 单测 |

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
| 新增（🔴） | **内置驱动从未注册（USIT 发现）**：`initialize_global_system` 首行调 `AutoDriverRegistrar::auto_register()`（幂等）——此前只有测试调注册，应用启动路径没注册过 `DriverRegistry`，测试连接报 `CONN_DRIVER_NOT_FOUND`，导航连接 / 重连同样会失败 | `engine/driver/auto_register.rs` 单测（内置 id 断言）+ `data_source_lifecycle::catalog_drivers_resolve_and_sqlite_probe_succeeds` |
| 新增（🔴） | **文件型工厂忽略 `url_override`（USIT 发现）**：sqlite / duckdb 工厂改从 `to_url()` 取地址（去前缀与查询串）——原先只读 `config.database`，服务层传 url_override 也会报「Database path is required for SQLite」 | 同上（真实文件探测：成功且磁盘出现空库文件） |
| 新增（🟡） | **暂存条目与表单不一致（USIT 发现）**：当前条目的类型徽标 / 名称改取 live 表单快照（`staging_display_type_id`），不再等草稿写回 | `helpers::staging_type_badge_prefers_live_form_for_current_entry` |

**已关闭（契约审计轮，2026-09-12，详见 §16）**

| # | 关闭方式 | 验证 |
| --- | --- | --- |
| 新增（🟡） | **标签双源漏同步**：`sync_snapshot_from_global` 只复制 JSON `tags`，权威检索表 `connection_tags` 仍是旧值 → `tag:x` 检索与后续标签视图读到同步前标签；现补 `sync_connection_tags` | `data_source_lifecycle::snapshot_sync_pulls_latest_global_definition`（扩展标签断言） |
| 新增（🟡） | **删除全局连接残留项目侧分组成员**：`delete` 的全局分支只清全局库（`cleanup_connection_org(..., None)`），而「全局连接加入项目分组」是合法配置 → 分组视图出现幻影成员；现带项目根时一并清理（非项目根跳过，避免误清全局库） | `data_source_lifecycle::global_delete_cleans_project_group_membership` |
| 新增（⚪） | **遗留 `conn-` 作用域判定两侧不一致**（服务层视作全局、`nav_runtime` 视作项目 → 标签 / 导航状态可能写错库）；现统一由 `id_prefix::{is_global_connection, uses_project_storage}` 判定 | `engine::persistence::id_prefix` 新增单测；M4 侧 `NavSource::from_conn_id` 待另一会话改依赖 `id_prefix`（#23） |
| 新增（🔴） | **协议链 / SSH 隧道保存后不生效（三层根因一次性收口）**：① 所有生产 connect 路径 `network_method: None` → 入口解析档案（`resolve_network_method_with_project`）并随请求传出；② 解析器只认小写类型键，而 UI 写入 `SSH`/`Proxy` → 大小写 / 别名归一；③ 隧道守卫存 `ConnectionService` 实例字段，而生产入口每次新建服务 → 注册表改挂 `ConnectionManager`（同一管理器共用，测试仍隔离）。另：对话框管理器类型下拉此前借用认证选项 → 按类填充规范键 + 写库白名单 | `referenced_network_profile_reaches_connect_request`（服务层）；`connection_service` 内嵌 2 项（注册表共享 / 类型键归一）；`connection_tunnel_cleanup` 回归通过 |

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
| 14 | ⚪ | 连接对话框**仍有存量裸 `px(...)`**（图标 / 圆角 / 描边等）未迁到 `ui.rs` 或 Tailwind 尺度 | 与用户侧新增的全局 UI 规范（`ui-design-spec.md` + `ui_contract` 契约测试）不一致（契约测试目前只扫 `view.rs` / `panels.rs`） | 按 `ui-design-spec.md` 迁移计划逐步扫一遍本模块 |
| 15 | 🟡 | **Tab 条 / 分段控件 / 开关为自绘**（`render.rs:354-382`、`1786-1834`、`873-961`、`930-961`、`managers.rs:309-337`）；`mod.rs` 曾错误记录「库无 Tabs/Switch」（已更正） | 无 hover / 键盘 / a11y / disabled；三处开关尺寸互不一致，且未接 `form_disabled`（未选驱动时仍可点） | 迁到 `TabBar::underline()` / `TabBar::segmented()` / `Switch`（0.6.1 均已提供）；顺带统一 disabled 语义 |
| 16 | ⚪→✅ | ~~**渲染热路径上的写状态与重计算**~~（**已关闭**）：第一批（决策 #67）驱动派生数据缓存 + 地址占位守卫；后半（决策 #73）暂存列表 `LiveEntryView` + `form_matches_draft` + 逐行短借用。**唯一保留项**：`render.rs` 里类型 / 驱动目录的每帧克隆（类型 ≤10、驱动 ≤6，各仅若干小字符串，量级远小于已收敛的两项）| 每帧 JSON 反序列化、额外 notify 循环与整表草稿克隆均已消除 | 若将来目录规模增长（驱动插件生态）再优化：把 `types` / `drivers` 改为 `Rc<Vec<…>>` 快照（会改动 `pub` 字段类型，需同步测试赋值写法），当前收益不抵改动面 |
| 17 | ⚪ | **下标参与 ElementId**：`render.rs:464/484/502/520`（协议链 hop）、`742`（驱动属性）、`1548/1640`（暂存条目）、`managers.rs:60/84/102/263`；另有 `sec-` 前缀在分组与策略覆盖两处复用 | 增删/重排后 hover、滚动等按 id 记录的控件状态串行；`policy_type` 命中分组 id 时潜在冲突 | 改用业务键（hop 名 / `saved_id` / `gid`），策略覆盖换独立前缀 |
| 18 | ⚪ | `project_path` 只有写入没有渲染点（`render.rs:185/233/2043`），与 `386-390` 注释承诺的「项目根可编辑」不符 | 无项目会话时用户无法输入/修正项目根 | 补 `Input::new(&project_path)` 或收敛注释与作用域分支 |
| 19 | 🟡 | **元数据缓存身份指纹未接线**：规则与纯函数（`engine::persistence::metadata_identity`，§3.6）已就绪，但 L2 路径仍按连接 ID（`conn_{id}.sqlite`）；`metadata_cache_index`（引用计数 / 孤儿 / 可读描述）与同指纹并发预热互斥未建 | 同一物理库的多条连接仍各自重建缓存（重复预热）；改名 / 改密 / 换驱动后命中旧缓存的收益尚未兑现 | 与 database-nav 接入 L2 的 Phase C 同轮：路径切 `meta_{fp}.sqlite` + 索引表 + per-fingerprint 互斥 + 旧 `conn_*.sqlite` 按 legacy 保留（不删） |
| 20 | 🔴→✅ | ~~协议链 / SSH 隧道保存后不生效~~（**已关闭**：三层根因一次性收口，见上方已关闭段） | — | 残留见 #24 |
| 21 | ⚪ | **启动即有项目会话时不加载 P_/GP_**（`view.rs:165` 用 `load_persisted_connections`，L168 才解析会话）；**关闭项目不清理残留**（`project/ui.rs::do_close` 不触发 `on_opened`） | 项目标签页看不到项目连接；关闭项目后残留行点“连接/编辑”必失败（`project_root=None`） | workbench 宿主侧：构造后按会话刷新一次；关闭后等价刷新（或给 `ProjectUiHost` 加 `on_closed`）；触碰 `view.rs` / `project` UI，需与布局会话协调 |
| 22 | ⚪ | **M4 导航行无删除入口**：唯一入口在编辑区详情卡（`panels.rs:3165`）；导航行点击也不写 `shared.selected` | M4 用户路径上没有删除能力；删除目标不直观（默认只指第一条） | M4 侧（另一会话）：行内 / 右键删除调同一 `workspace_loader::delete_connection`，删除成功后清导航缓存与状态 |
| 23 | ⚪ | **M4 标签 / 分组视图未接线**：`nav_runtime::{list_tags,set_tags,*group*}` 有 API、零调用；`database::model::ConnectionGroup` 是未消费的重复模型；`database::model::NavSource::from_conn_id` 自实现前缀推导（与 `id_prefix` 分裂） | 用户看不到 / 改不了标签与分组；遗留 `conn-` ID 在导航侧归错库 | M4 侧（另一会话）：B3 视图接线（消费 `nav_runtime` 组织 API）；`NavSource::from_conn_id` 改依赖 `engine::persistence::id_prefix`（M3 侧已收归单一来源，决策 #68） |
| 24 | 🟡→✅ | ~~网络配置仍难以在 UI 里真正建成~~（**表单部分已关闭**：本轮改为结构化字段表单 + 组装的 JSON 经 serde 模型单测验证；编辑回填真实字段；`chain` 仍走 JSON） | — | 残留见 #25 |
| 25 | ⚪→✅ | ~~内联协议链仍是占位（`Hop` 无主机 / 凭据字段，不参与执行）~~（**已关闭**：整块 UI 撤下，多跳改走 `chain` 档案；初始状态里的两条假跳数据一并移除，见决策 #72） | — | 后续如需“可视化多跳编辑器”，应在**档案侧**做（复用 `chain` 的 `ChainHop` 模型与 `network_field_specs` 思路），不在连接表单里做 |
| 26 | 🔴→✅ | ~~测试连接与真实连接不同源（忽略认证 / 网络档案）~~（**已关闭**：抽唯一组装点 `build_probe_config`，`connect` 与测试共用；详见上方 A1 轮已关闭段） | — | — |
| 27 | 🟡→✅ | ~~档案引用完整性缺失~~（**部分关闭**：引用计数 + 删除拦截 + 档案缺失显式报错；残留「跨项目全量引用扫描」见 #33） | — | — |
| 28 | 🟡 | **结果行只有成败不分级**：保存 / 测试 / 同步的反馈都是单行文本（成功与失败仅 `result_ok` 布尔），长消息被截断、无“复制详情” | 真机排障时拿不到完整原因（网络 / 认证 / SQL 错误混在一行） | 结果行分级（info / warning / error）+ 可展开详情 + 复制；可用 gpui-kit 的 `Alert` / `Notification` 组件 |
| 29 | ⚪ | **首次使用引导缺失**：新用户打开对话框看到类型树 / 暂存 / 档案引用，但没有“从哪开始”的引导（原型 §4 流程未在 UI 内体现） | 学习成本高（需读用户指南） | 空态引导（无连接 / 首启）+ 类型树 hover 说明；不引外链 |
| 30 | ⚪ | **`auth_configs.auth_data` 仍是裸 JSON 文本**：字段化组装只在网络档案侧做过（`build_network_config_json`），认证档案仍需手写 JSON | 认证档案缺少字段级校验与可视化编辑；写错只能在连接时暴露 | 与网络档案同款：`auth_field_specs` + 组装 / 回填纯函数 + serde 单测（对齐 `connection::url_params::inject_auth_into_url` 的键约定） |
| 31 | ⚪ | **标签双源未收敛**：连接行 `tags` JSON 与权威检索表 `connection_tags` 并存（写入时同步，读取侧仍有两路） | 同步失败仅告警 → 长期存在漂移风险 | 明确单一权威（建议表），连接行 JSON 降级为兼容投影并标记后续移除 |
| 32 | ⚪ | **连接 ID 命名方案待拍板**：`generate_gid("conn", name)` 是名字哈希（改名即换 ID），`G_`/`P_`/`GP_` 前缀同时承担作用域 / 存储路由 / 快照语义 / 可读性四种职责 | 用户看到 ID 里的名字片段会误以为是稳定主键；改名行为（新建 / 覆盖）需解释 | 方案 B：UI / 日志只出现 `name`，ID 内部化；方案 C：ULID 重做 + 迁移（代价大）；待用户决策后开工 |
| 33 | 🟡 | **引用计数不覆盖未打开的项目**：删除守卫只统计全局库 + 当前项目；未打开项目里的 P_/GP_ 引用不可见（删除仍会创建悬空引用，靠连接时报错兼底） | 用户删档案后打开另一个项目 → 那边的连接都报「引用的认证配置不存在」，且无从得知是哪个项目在用 | ① 全局库维护「档案 → 引用方」投影表（写入时维护，读取 O(1)）；或 ② 删除前遍历已知项目库（全局库项目注册表 + project crate 最近列表）并在 UI 列出引用方（项目 + 连接名） |

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
| 管理器列表（认证 / 网络 / 环境）与「被引用 N」 | 列表来自 `auth_configs` / `network_configs` / `environments`（名称 / 类型 / 内容）；计数为真实统计（`global_connections` + 当前项目库 `connections`） | ✅（A2：计数不再由 UI 估算） |
| 环境策略摘要（高级 Tab） | `environment_policies.policy_config` | ✅ |
| 策略覆盖勾选项 | **`environment_policies`（选中环境的启用策略；本轮修复）** | ✅（旧为硬编码 6 项） |
| 环境管理器策略列表 / 落库类型 | **`environment_policies.policy_type`（本轮修复错标与假类型写入）** | ✅ |
| 项目下拉（项目名 + 路径） | `project::service::list_recent` + 当前会话（项目库/全局项目表） | ✅ |
| 分组勾选 | 项目库 `connection_groups` / `connection_group_members` | ✅ |
| 标签 | 连接记录 `tags` / `connection_tags` | ✅ |
| 暂存列表草稿 | `connection_drafts`（无密码列）+ 会话内快照 | ✅ |
| 暂存列表已保存条目 | `workspace_loader::load_connections_for_scope`（全局 + 项目库合并） | ✅ |
| 连接设置卡（网络型：主机/端口/数据库） | 从当前 URI 输入解析（用户输入派生）；**行的存在性与标签取 `drivers.config_schema.fields[]`**（未声明则不出该行；schema 为空才回退内置三行）；字段**可编辑**，改动经 `rebuild_url_from_fields` 回写 URI（反向：URI → 字段） | ✅ |
| 连接设置卡（文件型：地址） | 用户选择/输入的文件路径；系统选择器返回真实路径（新建时才创建空文件） | ✅ |
| 地址标签与输入占位 | 标签固定（文件型 = 地址 / 网络型 = URI，按用户要求不被 schema 覆盖）；网络型占位取 `drivers.url_template` + `default_port`（文件型走类型文案字典）；文件型地址行**占位**优先取 `config_schema` 的 `type=file` 字段 `placeholder` | ✅ |
| 结果行提示（保存 / 测试 / 同步） | 服务层真实返回（`DataSourceService::{save,update,test}` 等） | ✅ |
| 测试连接结果（版本 / 延迟 / 范围说明） | 真实探测（`DataSourceService::test`）：认证档案凭据**从库读取并解密**后注入、网络档案**真实建隧道**、驱动属性 / 高级选项来自连接字段与库；范围说明（「已应用认证档案凭据 / 已建立网络档案隧道 / 档案缺失」）由服务层返回，UI 只拼接展示 | ✅（A1：此前档案被静默忽略） |
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
| userinfo 转义 | `connection::url_params::merge_credentials`（保存 / 更新 / 测试共用） | 密码 / 用户名含 `@` `:` `/` `?` `#` `%` 时 URL 结构被破坏（host 被截断 / 端口歧义）→ 落库 URL 与实际连接目标不一致 | `url_params` 内嵌单测（`p@ss:w/rd` → `%40 %3A %2F`，仅密码与已有 userinfo 分支）+ `probe_config_applies_referenced_auth_profile` |
| 删除引用守卫 | `DataSourceService::ensure_no_references`（管理器删除路径） | 删除仍被连接引用的认证 / 网络 / 环境配置 → 悬空引用（用户以为已清理） | `manager_reference_count_and_delete_guard` |
| 档案缺失不降级 | `connection_service::{inject_auth_config_credentials, build_probe_config}` + `connect` 网络守卫 | 档案被删 / 类型未知时静默直连（用户以为走的是隧道 / 专用账号）；测试与真实连接结论相反 | `probe_config_applies_referenced_auth_profile`（缺失报错）、`referenced_network_profile_without_method_is_rejected` |

### 15.3 约束与回归手段

- **新代码规则**：新增 UI 数据项前先回答“它来自哪张表 / 哪个服务方法”；只能从字典来的东西（标签）不得携带取值。
- **回归手段**：字典与解析函数均有单测（`helpers.rs`）；按库读取的服务方法有集成测试（`data_source_lifecycle.rs`）；渲染层不产生业务值，因此不需要图像基线也能拦住“造数据”类回归。
- **待办**：驱动安装能力（§14 #1 的中期项）落地后，`drivers` 目录会真实增长，类型树的可用性判定无需改动即生效。

---

## 16. M3 ↔ M4（数据库导航）契约面审计（2026-09-12）

> 背景：用户要求核对“新增数据源模块与 db-nav 是否打通”。结论按**证据**给出（`file:line` 为核对时的位置）；跨模块（M4 侧）缺口归另一会话，本节只做登记。

| 契约点 | 生产方（M3） | 消费方（M4） | 状态 |
| --- | --- | --- | --- |
| 连接列表可见性 | `workspace_loader::load_connections_for_scope`（全局 + 项目侧 P_/GP_ 合并） | 面板读 `shared.connections`（`panels.rs`） | ✅ 打通（刷新时机见 #21） |
| 来源短码 `P/G/GP` | `engine::persistence::id_prefix`（本轮收归单一来源）；对话框暂存条目 `saved_scope_short` | `database::model::NavSource::from_conn_id`（M4 自实现） | ⚠️ 半通（遗留 `conn-` 判定不一致；M3 侧已统一，M4 侧待改 → #23） |
| 运行时连接 | `nav_runtime::connect_entry(_with)` → `get_with_project` + 解析网络档案（本轮） | 导航面板按钮 | ✅ 打通（网络方式随请求带出） |
| 断开 | `nav_runtime::disconnect_entry` → `close_connection`（保留 L2 缓存） | 导航面板按钮 | ✅ 打通 |
| 删除 | `workspace_loader::delete_connection(conn_id, project_root)` → `DataSourceService::delete`（作用域路由 + Secret / 组织清理） | 仅编辑区详情卡；**导航行无入口** | ⚠️ M4 侧缺 UI（#22） |
| 编辑 | `EditorPanel::request_edit_connection`（生产入口：订阅 + 宿主重绘） | 导航行 ✎ | ✅ 打通 |
| 标签 | `DataSourceService::{save,update,delete}` + 本轮补齐的 `sync_snapshot_from_global` → `connection_tags`（权威检索表） | `nav_runtime::{list_tags,set_tags}` | ⚠️ M3 侧已闭环；**M4 视图未接线**（#23） |
| 分组 | `ConnectionOrgStore`（项目库；多对多 + 组内排序）+ 对话框勾选（替换语义）+ 本轮补的全局连接删除清理 | `nav_runtime::{list_groups,create_group,rename_group,delete_group,*_member}` | ⚠️ 服务层打通；M4 视图未接线（B3，另一会话进行中） |
| L2 元数据缓存 | `MetadataCacheManager::build_metadata_path`（按 `conn_id`）；`ConnectionService::ensure_metadata_cache` | **无调用方**（导航当前走实时内省） | ⛔ 未接（Phase C；路径切身份指纹见 §3.6 / C8） |
| 连接状态点 | `workspace_loader::fill_connected`（来自 `ConnectionManager` 运行态） | 导航行状态点 | ✅ 打通 |
| 类型 / 驱动目录 | `DataSourceService::{list_types,list_drivers}` | 导航不消费（无需） | — 非契约 |
| 项目会话切换 | `project_host::refresh_after_open`（打开 / 切换后刷新列表） | 面板读同一 `shared.connections` | ⚠️ 半通（启动 / 关闭两处不刷新 → #21） |

**命名辨析（审计中确认无实际混用，但极易误读）**：`global_connections.metadata_path` / 对话框「缓存路径」是 **DuckDB 联邦加速**的缓存路径（自由文本），与 L2 元数据缓存 `conn_{id}.sqlite` 是两回事；前者随连接落库（`connection_service.rs:408` 一带）、后者由 `MetadataCacheManager` 管理。改 L2 键时**不要**动 `metadata_path` 列语义。

> **2026-09-12 补充（A1/A2）**：`ConnectionService::connect` 现在对「引用的认证 / 网络档案缺失或无法解析」**直接报错**（不再静默直连 / 无凭据连接），且 `url_params::merge_credentials` 会对 userinfo 做百分号转义。M4 侧**无需改动**：`nav_runtime::connect_entry` 原本就透传 `Result`，导航面板把错误消息展示出来即可。若将来想在导航行上预检并标记“引用悬空”，属 M4 侧增强（可在 B 系列登记）。
