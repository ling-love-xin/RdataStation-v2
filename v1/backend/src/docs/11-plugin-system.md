# 插件系统设计

> 版本：v1.0
> 最后更新：2026-05-10
> 状态：📋 远期规划（当前不实施）
>
> 本文档描述 RdataStation 插件系统的完整设计方案，对标 VSCode Extension System，采用 Go Sidecar + Extism WASM 双引擎架构。

---

## 设计目标

1. **对标 VSCode**：激活事件、贡献点、进程隔离、双向 IPC 全部对齐业界标杆
2. **安全分级**：WASM 沙箱插件可放心从市场安装，Sidecar 插件需用户确认权限
3. **能力互补**：Sidecar 覆盖系统级场景（CGO 驱动、网络服务），WASM 覆盖算法级场景（格式化、Diff、转换）
4. **统一体验**：插件开发者只需写一份 `plugin.toml`，引擎差异对开发者透明
5. **平滑扩展**：未来可加入 Python 子进程、JavaScript 引擎等新运行时

---

## VSCode 插件系统参考

VSCode 插件系统的六个核心设计原则及 RdataStation 对应实现：

| 原则         | VSCode 实现                                        | RdataStation 对应                         |
| ------------ | -------------------------------------------------- | ----------------------------------------- |
| **进程隔离** | Extension Host 独立 Node.js 进程，崩溃不影响主窗口 | Go Sidecar 独立进程                       |
| **沙箱安全** | 无（仅进程隔离），靠权限声明                       | Extism WASM 字节码级沙箱                  |
| **延迟激活** | `activationEvents`（`onLanguage`、`onCommand` 等） | `[plugin.activation]` 同机制              |
| **能力声明** | `package.json` 中 `contributes` 节点               | `plugin.toml` 中 `[plugin.contributions]` |
| **双向 IPC** | JSON-RPC over stdin/stdout                         | Arrow IPC + Protobuf gRPC                 |
| **UI 扩展**  | Webview + TreeView + StatusBar API                 | 前端组件插槽 + 视图注册                   |

```
┌─────────────────────────────────────────────────────┐
│  VSCode Main Process (Electron)                     │
│  ┌──────────────┐  ┌──────────────┐                 │
│  │ Window       │  │ Extension    │                 │
│  │ Manager      │  │ Management   │                 │
│  └──────┬───────┘  └──────┬───────┘                 │
│         │                 │ IPC (JSON-RPC)          │
└─────────┼─────────────────┼─────────────────────────┘
          │                 │
          │    ┌────────────▼──────────────┐
          │    │ Extension Host Process     │  ◄── 独立进程
          │    │ (Node.js)                  │
          │    │  ┌──────────────────────┐ │
          │    │  │ Extension A (激活)    │ │
          │    │  │ Extension B (休眠)    │ │
          │    │  │ Extension C (激活)    │ │
          │    │  └──────────────────────┘ │
          │    └───────────────────────────┘
          │
    ┌─────▼──────────┐
    │ Renderer       │  ◄── Webview / iframe
    │ (UI Process)   │
    └────────────────┘
```

---

## 双引擎插件架构

### 总体架构图

```
┌──────────────────────────────────────────────────────────────────┐
│  RdataStation Rust Core (Tauri Main Process)                      │
│                                                                    │
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐   │
│  │ Connection Mgr  │  │ Metadata Cache  │  │ Plugin Registry │   │
│  │ (连接生命周期)   │  │ (元数据缓存)    │  │ (插件注册中心)  │   │
│  └────────┬────────┘  └────────┬────────┘  └────────┬────────┘   │
│           │                    │                      │            │
│  ┌────────▼────────────────────▼──────────────────────▼────────┐  │
│  │                    Plugin Orchestrator                       │  │
│  │  ┌──────────────────────┐  ┌──────────────────────────────┐ │  │
│  │  │ Sidecar Manager      │  │ WASM Runtime Manager         │ │  │
│  │  │ (Go 进程生命周期)     │  │ (Extism 插件生命周期)        │ │  │
│  │  └──────────┬───────────┘  └──────────────┬───────────────┘ │  │
│  └─────────────┼─────────────────────────────┼─────────────────┘  │
└────────────────┼─────────────────────────────┼────────────────────┘
                 │                             │
    ┌────────────▼────────────┐   ┌────────────▼────────────┐
    │  Go Sidecar Process     │   │  WASM Sandbox           │
    │  (独立进程，长生命周期)   │   │  (Extism Runtime)       │
    │                         │   │                         │
    │  ┌───────────────────┐  │   │  ┌───────────────────┐  │
    │  │ Oracle Driver     │  │   │  │ CSV 导入导出       │  │
    │  │ S3 Export Service │  │   │  │ Schema Diff 算法   │  │
    │  │ AI/ML Inference   │  │   │  │ SQL 格式化         │  │
    │  │ SQL LSP Server    │  │   │  │ 数据校验引擎       │  │
    │  └───────────────────┘  │   │  └───────────────────┘  │
    └─────────────────────────┘   └─────────────────────────┘
```

### 双引擎职责划分

| 维度         | 🐹 Go Sidecar（重量级引擎） | 🧩 Extism WASM（轻量级引擎） |
| ------------ | --------------------------- | ---------------------------- |
| **定位**     | 长生命周期服务 + 系统级能力 | 安全沙箱 + 纯计算逻辑        |
| **启动速度** | 慢（进程启动 ~100ms+）      | 快（WASM 实例化 ~1ms）       |
| **内存模型** | 独立进程，500MB~2GB 可配    | 沙箱内，默认上限 512MB       |
| **系统权限** | 完整（网络、文件、子进程）  | 受限（WASI 权限声明）        |
| **崩溃影响** | 仅自身进程重启              | 仅当前沙箱销毁               |
| **热加载**   | 需重启 Sidecar              | 支持（卸载→加载新 WASM）     |
| **安全性**   | 中等（进程级别隔离）        | 高（字节码级别沙箱）         |

#### Go Sidecar 适用场景

```
🟢 需要 CGO 的数据库驱动（Oracle OCI、SQL Server ODBC、Hive）
🟢 云服务集成（S3/GCS/Azure Blob 文件导出）
🟢 AI/ML 推理服务（Python 子进程调度、LLM 调用）
🟢 长连接代理（SSH Tunnel、VPN、HTTP Proxy）
🟢 文件系统密集操作（大文件导入导出、批量 DDL 生成）
🟢 外部工具调用（mysqldump、pg_dump、dbt 运行）
🟢 语言服务器（SQL LSP 提供自动补全、诊断）
```

#### Extism WASM 适用场景

```
🟢 数据格式转换（CSV↔JSON↔Parquet↔Arrow，纯算法）
🟢 Schema Diff 算法（比较两份元数据的纯计算）
🟢 SQL 格式化 / 美化（纯字符串处理）
🟢 数据校验 / 清洗规则引擎（正则、类型检查）
🟢 轻量级可视化数据预处理（聚合、采样）
🟢 密码学工具（哈希、加密、JWT 解析）
🟢 第三方数据协议解析（自定义二进制格式）
```

### 选择建议（给插件开发者）

```
是否需要网络访问？
  ├── 是 → Go Sidecar
  └── 否 → 是否需要 CGO 或外部进程？
              ├── 是 → Go Sidecar
              └── 否 → Extism WASM ✅ 推荐
```

---

## 插件清单（plugin.toml）

对标 VSCode 的 `package.json`，每个插件目录下必须包含 `plugin.toml`。

### 完整示例

```toml
[plugin]
name = "rdatastation-plugin-oracle"
display_name = "Oracle Database Driver"
version = "1.2.0"
description = "Oracle 数据库连接支持（通过 Go CGO OCI 驱动）"
author = "RdataStation Team"
icon = "assets/oracle.svg"
homepage = "https://plugins.rdatastation.com/oracle"

# ===== 运行时引擎选择 =====
[plugin.runtime]
engine = "sidecar"           # "sidecar" | "wasm"
sidecar_binary = "bin/oracle-driver"  # 仅 sidecar
wasm_entry = "plugin.wasm"            # 仅 wasm
wasm_wasmtime_version = "43.0"

# ===== 激活事件（对标 VSCode Activation Events） =====
[plugin.activation]
# 当用户打开某类数据库连接时激活
on_connection_type = ["oracle", "oracle_rac"]
# 当用户执行特定命令时激活
on_command = [
    "rdatastation.oracle.importDump",
    "rdatastation.oracle.exportDump",
]
# 当某个 UI 视图可见时激活
on_view = ["oracleSchemaBrowser"]
# 语言模式激活（SQL 方言）
on_language = ["plsql"]
# 启动时激活（谨慎使用）
on_startup = false

# ===== 贡献点（对标 VSCode Contributions） =====
[plugin.contributions]

# --- 数据库驱动 ---
[[plugin.contributions.drivers]]
id = "oracle"
display_name = "Oracle"
default_port = 1521
protocols = ["oracle:thin", "jdbc:oracle:thin"]

# --- 命令注册 ---
[[plugin.contributions.commands]]
id = "rdatastation.oracle.importDump"
title = "Oracle: 导入 Dump 文件"
category = "数据迁移"
icon = "file-input"
# 绑定到右键菜单
[[plugin.contributions.commands.menus]]
location = "context_menu.table"
when = "connection.driver == 'oracle'"

# --- 视图扩展 ---
[[plugin.contributions.views]]
id = "oracleSchemaBrowser"
name = "Oracle 对象浏览器"
location = "sidebar.left"        # 侧边栏 / 面板 / 独立窗口
when = "connection.driver == 'oracle'"

# --- 数据导出格式 ---
[[plugin.contributions.export_formats]]
id = "oracle.dump"
display_name = "Oracle Dump (.dmp)"
extensions = [".dmp"]
mime_type = "application/octet-stream"

# --- 数据导入格式 ---
[[plugin.contributions.import_formats]]
id = "oracle.dump"
display_name = "Oracle Dump (.dmp)"
extensions = [".dmp"]

# ===== 权限声明（仅 WASM） =====
[plugin.permissions]   # sidecar 不需要此节
network = ["api.github.com"]
filesystem = ["/tmp/rdatastation/*"]
environment = ["HOME", "TMPDIR"]

# ===== 依赖 =====
[plugin.dependencies]
# 依赖其他插件
"rdatastation-plugin-ssh" = ">=1.0.0"
# 依赖 Core API 版本
core_api = ">=2.0.0"

# ===== 沙箱配置（仅 WASM） =====
[plugin.sandbox]
max_memory_mb = 512
max_cpu_time_ms = 60000
max_file_size_mb = 1024
network_requests_limit = 100
```

### 清单字段说明

#### [plugin] — 基本信息

| 字段           | 类型   | 必填 | 说明                                            |
| -------------- | ------ | ---- | ----------------------------------------------- |
| `name`         | string | ✅   | 插件唯一标识，格式 `rdatastation-plugin-{name}` |
| `display_name` | string | ✅   | 用户可见的显示名称                              |
| `version`      | string | ✅   | SemVer 版本号                                   |
| `description`  | string | ✅   | 简要描述                                        |
| `author`       | string | ✅   | 作者名或组织名                                  |
| `icon`         | string | ❌   | 图标路径（相对于插件根目录）                    |
| `homepage`     | string | ❌   | 插件主页 URL                                    |

#### [plugin.runtime] — 运行时引擎

| 字段             | 类型   | 必填 | 说明                           |
| ---------------- | ------ | ---- | ------------------------------ |
| `engine`         | enum   | ✅   | `"sidecar"` 或 `"wasm"`        |
| `sidecar_binary` | string | 条件 | Sidecar 插件入口二进制相对路径 |
| `wasm_entry`     | string | 条件 | WASM 插件入口文件相对路径      |

#### [plugin.activation] — 激活事件

| 字段                 | 类型     | 说明                       |
| -------------------- | -------- | -------------------------- |
| `on_connection_type` | string[] | 连接指定类型数据库时激活   |
| `on_command`         | string[] | 执行指定命令时激活         |
| `on_view`            | string[] | 指定 UI 视图可见时激活     |
| `on_language`        | string[] | 打开指定 SQL 方言时激活    |
| `on_startup`         | bool     | 应用启动时激活（谨慎使用） |

#### [plugin.contributions] — 贡献点

| 贡献点                      | 说明                                        |
| --------------------------- | ------------------------------------------- |
| `drivers`                   | 注册数据库驱动（连接协议、默认端口）        |
| `commands`                  | 注册命令（菜单项、快捷键）                  |
| `views`                     | 注册侧边栏/面板视图                         |
| `export_formats`            | 注册数据导出格式                            |
| `import_formats`            | 注册数据导入格式                            |
| `connection_providers`      | SSH Tunnel / VPN 等连接代理（预留）         |
| `schema_diff_strategies`    | Schema Diff 算法策略（预留）                |
| `data_migration_strategies` | 数据迁移策略（全量/增量/CDC）（预留）       |
| `sql_dialects`              | SQL 方言定义（PLSQL/T-SQL/PLpgSQL）（预留） |
| `visualization_providers`   | 图表渲染引擎（ECharts/D3/Plotly）（预留）   |
| `themes`                    | UI 主题（预留）                             |
| `keybindings`               | 快捷键绑定（预留）                          |

#### When Clause 条件表达式

对标 VSCode 的 `when` 子句，支持以下上下文变量：

```
connection.driver          # 当前连接驱动类型：mysql / postgres / sqlite / duckdb / oracle / ...
connection.name            # 当前连接名称
connection.is_connected    # 连接是否活跃
editor.language            # 当前编辑器语言模式
view.active                # 当前激活的视图 ID
os.platform                # 操作系统：windows / macos / linux
```

---

## 通信协议

### 协议总览

```
┌─────────────────────────────────────────────────────────┐
│                   Plugin Orchestrator                    │
│                                                         │
│  ┌─────────────────────┐    ┌─────────────────────────┐ │
│  │ Sidecar Bridge      │    │ WASM Host Functions     │ │
│  │                     │    │                         │ │
│  │ Protocol: Arrow     │    │ Protocol: Extism        │ │
│  │ IPC + gRPC stream   │    │ Host Function calls     │ │
│  │                     │    │                         │ │
│  │ Transport:          │    │ Data: Arrow IPC         │ │
│  │ Unix Socket/Named   │    │ (shared memory via      │ │
│  │ Pipe (本地)         │    │  WASI preview2)         │ │
│  └─────────┬───────────┘    └───────────┬─────────────┘ │
└────────────┼────────────────────────────┼───────────────┘
             │                            │
    ┌────────▼────────┐          ┌────────▼────────┐
    │ Go Sidecar      │          │ WASM Plugin     │
    │ (gRPC server)   │          │ (Extism instance)│
    └─────────────────┘          └─────────────────┘
```

### Protobuf API 定义

```protobuf
// plugin_api.proto — 插件 API 统一定义
// 由 Rust Core 实现 gRPC server，Sidecar/WASM 作为 client 调用

service PluginHost {
  // === 数据库操作 ===
  rpc ExecuteQuery(QueryRequest) returns (stream ArrowBatch);
  rpc ExecuteCommand(CommandRequest) returns (CommandResponse);

  // === 元数据操作 ===
  rpc ListDatabases(ListRequest) returns (StringList);
  rpc ListSchemas(SchemaRequest) returns (StringList);
  rpc ListTables(TableListRequest) returns (SchemaObjectList);
  rpc ListColumns(ColumnRequest) returns (ColumnDetailList);
  rpc ListForeignKeys(FKRequest) returns (ForeignKeyList);
  rpc ListIndexes(IndexRequest) returns (IndexList);
  rpc GetTableDDL(DDLRequest) returns (DDLResponse);
  rpc GetRoutineSource(RoutineRequest) returns (RoutineResponse);

  // === 连接管理 ===
  rpc GetActiveConnections(Empty) returns (ConnectionList);
  rpc GetConnectionInfo(ConnectionRequest) returns (ConnectionInfo);

  // === UI 交互 ===
  rpc ShowMessage(MessageRequest) returns (MessageResponse);
  rpc ShowProgress(ProgressRequest) returns (stream ProgressUpdate);
  rpc OpenEditor(EditorRequest) returns (Empty);

  // === 文件系统 ===
  rpc ReadFile(FileRequest) returns (FileData);
  rpc WriteFile(FileWriteRequest) returns (Empty);

  // === 日志 ===
  rpc Log(LogEntry) returns (Empty);

  // === 健康检查 ===
  rpc HealthCheck(Empty) returns (HealthStatus);
}

message QueryRequest {
  string connection_id = 1;
  string sql = 2;
  repeated Value params = 3;
  uint64 max_rows = 4;
  uint64 timeout_ms = 5;
}

message ArrowBatch {
  bytes ipc_message = 1;  // Arrow IPC Stream Format
}

message ForeignKeyDetail {
  string name = 1;
  repeated string columns = 2;
  string ref_table = 3;
  string ref_schema = 4;
  repeated string ref_columns = 5;
  string on_delete = 6;   // CASCADE / SET NULL / RESTRICT / NO ACTION
  string on_update = 7;
}
```

### 两种引擎的协议实现差异

| 层级          | Go Sidecar                      | Extism WASM                           |
| ------------- | ------------------------------- | ------------------------------------- |
| **传输层**    | Unix Domain Socket / Named Pipe | Extism Host Function 调用             |
| **序列化**    | Arrow IPC + Protobuf            | Arrow IPC（共享内存）+ JSON（控制面） |
| **流式数据**  | gRPC bidirectional stream       | 回调 + Ring Buffer                    |
| **连接管理**  | Sidecar 内部管理引用            | Host 管理，插件无连接所有权           |
| **错误传播**  | gRPC Status codes + CoreError   | Host Function 返回值                  |
| **心跳/健康** | gRPC Health Check protocol      | 由 Extism runtime 管理                |

### Arrow 数据传输策略

大块数据（查询结果、导入导出流）统一使用 Arrow IPC Stream Format：

```
Rust Core ←→ Go Sidecar:  Arrow Flight / Arrow IPC over Unix Socket
Rust Core ←→ WASM Plugin: Arrow IPC via shared WASM memory buffer
```

这确保两种引擎都享受零拷贝的数据传输，避免 JSON 序列化性能瓶颈。

---

## 插件生命周期

对标 VSCode 的插件生命周期，所有插件遵循统一的状态机：

```
┌──────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐
│ Discover │──▶│ Resolve  │──▶│ Activate │──▶│ Running  │──▶│ Deactivate│
│ 发现     │   │ 解析依赖  │   │ 激活     │   │ 运行中   │   │ 停用     │
└──────────┘   └──────────┘   └──────────┘   └──────────┘   └──────────┘
      │              │              │              │              │
      ▼              ▼              ▼              ▼              ▼
  扫描插件      检查依赖      onCreate()*    callPlugin()    onDestroy()
  目录          版本兼容     onActivate()   定时任务        资源释放
  读取          安全审计     onStartup       UI 更新        卸载 WASM
  plugin.toml                onConnection                   关闭 Sidecar
                             onCommand
                             onView
```

### 生命周期钩子

| 钩子                     | 触发时机                                 | Sidecar | WASM |
| ------------------------ | ---------------------------------------- | :-----: | :--: |
| `plugin.onCreate()`      | 插件首次加载到注册中心（清单解析通过后） |   ✅    |  ✅  |
| `plugin.onActivate(ctx)` | 满足任意激活事件，插件被唤醒             |   ✅    |  ✅  |
| `plugin.onDeactivate()`  | 所有激活条件消失，插件进入休眠           |   ✅    |  ✅  |
| `plugin.onDestroy()`     | 插件被用户卸载或应用退出                 |   ✅    |  ✅  |
| `plugin.onError(err)`    | 插件执行出错（不影响其他插件）           |   ✅    |  ✅  |
| `plugin.healthCheck()`   | 定期健康检查心跳                         |   ✅    |  ❌  |

### 激活上下文 (ActivationContext)

```rust
/// 插件激活时注入的上下文
pub struct PluginActivationContext {
    /// 触发激活的事件类型
    pub event: ActivationEvent,
    /// 插件工作目录（隔离的临时目录）
    pub workspace: PathBuf,
    /// 插件数据持久化目录
    pub data_dir: PathBuf,
    /// 日志通道
    pub logger: Arc<dyn PluginLogger>,
    /// 配置（从 plugin.toml 解析）
    pub config: PluginConfig,
}

pub enum ActivationEvent {
    Startup,
    ConnectionType(String),
    Command(String),
    View(String),
    Language(String),
}
```

---

## Go Sidecar 内部架构

### 推荐设计：子进程模型（非 Go Plugin）

Go 原生 plugin（`.so`）机制存在以下致命问题：

- Windows 支持极差（CGO 交叉编译困难）
- 要求 Host 和 Plugin 使用完全相同的 Go 版本编译
- 不支持热加载（plugin 加载后无法卸载）

因此推荐**子进程 + gRPC**模型：

```
┌─────────────────────────────────────────────┐
│          Go Sidecar Host Process            │
│                                              │
│  ┌──────────────────────────────────────┐   │
│  │        gRPC Server (Host)            │   │
│  │  - Listens on Unix Socket            │   │
│  │  - Arrow IPC data channel            │   │
│  │  - Health Check                      │   │
│  └────────────┬─────────────────────────┘   │
│               │ gRPC (local)                │
│  ┌────────────▼─────────────────────────┐   │
│  │        Plugin Process Manager        │   │
│  │  - fork/exec subprocesses            │   │
│  │  - stdin/stdout JSON-RPC control     │   │
│  │  - crash recovery & restart          │   │
│  └────────────┬─────────────────────────┘   │
│               │                              │
│  ┌───▼──┐ ┌───▼──┐ ┌───▼──┐ ┌───▼──┐      │
│  │Oracle│ │ SQL  │ │ S3   │ │ AI   │      │
│  │Driver│ │Server│ │Export│ │Model │      │
│  │(exe) │ │(exe) │ │(exe) │ │(exe) │      │
│  └──────┘ └──────┘ └──────┘ └──────┘      │
│    独立 Go 二进制，各自编译部署              │
└─────────────────────────────────────────────┘
```

### Go Plugin Interface

```go
// sidecar/plugin.go — 子进程插件接口定义

type Plugin interface {
    Name() string
    Version() string
    OnActivate(ctx PluginContext) error
    OnDeactivate() error
    HandleCommand(cmd string, args []byte) ([]byte, error)
}

type PluginContext struct {
    Workspace string
    DataDir   string
    Config    map[string]interface{}
    Client    PluginHostClient  // gRPC client to Rust Core
}

// 示例：Oracle 驱动插件
// sidecar/plugins/oracle/main.go
type OracleDriver struct { ... }

func (d *OracleDriver) Name() string { return "oracle" }
func (d *OracleDriver) OnActivate(ctx PluginContext) error {
    // 初始化 OCI 环境
    return nil
}
func (d *OracleDriver) HandleCommand(cmd string, args []byte) ([]byte, error) {
    switch cmd {
    case "connect":
        // 建立 Oracle 连接
    case "query":
        // 执行查询
    }
    return nil, nil
}
```

---

## 插件市场（远期规划）

### 目录结构

```
~/.rdatastation/
├── plugins/
│   ├── rdatastation-plugin-oracle/
│   │   ├── plugin.toml
│   │   ├── bin/
│   │   │   └── oracle-driver    # Sidecar 二进制
│   │   └── assets/
│   │       └── oracle.svg
│   ├── rdatastation-plugin-csv-export/
│   │   ├── plugin.toml
│   │   └── plugin.wasm           # WASM 文件
│   └── ...
├── plugin_cache/                  # 插件下载缓存
├── plugin_state.json              # 插件启用/禁用状态
└── plugin_logs/                   # 插件日志
```

### 分发方式

| 阶段 | 方式             | 说明                                                      |
| ---- | ---------------- | --------------------------------------------------------- |
| MVP  | 本地目录加载     | 用户手动放置 `plugin.toml` + 二进制文件到 `plugins/` 目录 |
| V1   | Git Release 下载 | 从 GitHub Releases 自动下载对应平台二进制                 |
| V2   | 插件市场 API     | 集中式插件索引 + 版本管理 + 评分/评论                     |

---

## 与当前架构的集成路径

### 现有资产复用

| 现有组件                          | 在插件系统中的角色                               |
| --------------------------------- | ------------------------------------------------ |
| `adapters/wasm/mod.rs`            | WASM Runtime Manager 的接口基础                  |
| `adapters/wasm/extism.rs`         | Extism 插件实例化引擎                            |
| `adapters/wasm/plugin_manager.rs` | 沙箱配置 + 资源限制（直接复用）                  |
| `core/driver/traits.rs`           | PluginHost API 的能力模型（需扩展 FK/Index/DDL） |
| `core/models.rs`                  | QueryResult + ArrowBatch 作为核心数据载体        |
| `core/error.rs`                   | CoreError 统一错误传播                           |
| `core/arrow.rs`                   | ArrowHandler 数据格式转换                        |

### 新增文件规划

```
src-tauri/src/
├── adapters/
│   ├── wasm/              ← 现有（扩展 Extism host functions）
│   │   ├── mod.rs
│   │   ├── extism.rs
│   │   ├── api.rs          ← 替换 DefaultWasmPluginApi stub
│   │   ├── plugin_manager.rs
│   │   └── host_functions.rs   🆕 注册 Database trait 为 host functions
│   └── sidecar/            🆕 新增
│       ├── mod.rs           🆕 Sidecar 进程管理入口
│       ├── manager.rs       🆕 Sidecar 生命周期管理
│       ├── bridge.rs        🆕 gRPC + Arrow IPC 通信桥
│       └── health.rs        🆕 健康检查
├── core/
│   └── plugin/             🆕 新增（Core 层插件抽象）
│       ├── mod.rs           🆕 Plugin trait 定义
│       ├── registry.rs      🆕 插件注册中心
│       ├── manifest.rs      🆕 plugin.toml 解析器
│       ├── context.rs       🆕 PluginActivationContext
│       └── types.rs         🆕 PluginType / PluginState / ActivationEvent
└── proto/
    └── plugin_api.proto     🆕 Protobuf API 定义
```

### 对现有 Database Trait 的依赖扩展

插件系统需要 Database trait 补充以下方法（均提供默认空实现，不破坏现有驱动）：

```rust
// 新增：外键关系（ERD 可视化必须）
async fn list_foreign_keys(&self, db: &str, schema: Option<&str>, table: &str)
    -> Result<Vec<ForeignKeyDetail>, CoreError> { Ok(vec![]) }

// 新增：索引列表
async fn list_indexes(&self, db: &str, schema: Option<&str>, table: &str)
    -> Result<Vec<IndexDetail>, CoreError> { Ok(vec![]) }

// 新增：表 DDL 语句
async fn get_table_ddl(&self, db: &str, schema: Option<&str>, table: &str)
    -> Result<Option<String>, CoreError> { Ok(None) }

// 新增：表统计信息（行数、大小等）
async fn get_table_stats(&self, db: &str, schema: Option<&str>, table: &str)
    -> Result<Option<TableStats>, CoreError> { Ok(None) }
```

---

## 实施路线图

| 阶段        | 内容                                                      | 优先级      | 预计工作量 |
| ----------- | --------------------------------------------------------- | ----------- | ---------- |
| **Phase 0** | Database Trait 扩展（FK/Index/DDL/Stats）                 | 🔴 基础依赖 | 2-3 天     |
| **Phase 1** | `core/plugin/` 抽象层（manifest 解析、registry、context） | 🔴 基础依赖 | 3-5 天     |
| **Phase 2** | WASM Host Functions 注册（让 WASM 插件能调 Database API） | 🔴 核心阻塞 | 3-5 天     |
| **Phase 3** | Arrow IPC 通道（Rust↔WASM 零拷贝数据交换）                | 🟡 性能关键 | 2-3 天     |
| **Phase 4** | Go Sidecar Manager（进程启动/停止/健康检查）              | 🟡 架构补全 | 3-5 天     |
| **Phase 5** | gRPC + Arrow IPC Bridge（Rust↔Go Sidecar 通信）           | 🟡 架构补全 | 3-5 天     |
| **Phase 6** | 首个示例插件（CSV 导入导出 WASM 插件）                    | 🟢 验证闭环 | 1-2 天     |
| **Phase 7** | 插件市场（GitHub Release 下载）                           | 🟢 体验完善 | 3-5 天     |

---

## 设计评审

### 🟢 优势

1. **对标 VSCode 完备度**：Activation Events、Contributions、进程隔离、双向 IPC 全部对标，开发者学习成本低
2. **双引擎合理性**：Sidecar 覆盖 CGO/网络/AI，WASM 覆盖算法/Diff/格式化，互补而非重叠
3. **安全性分级**：WASM 沙箱 + Sidecar 进程隔离，天然形成"安全插件"和"信任插件"的分级
4. **统一清单**：`plugin.toml` 作为唯一真相源，`engine = "sidecar"` vs `engine = "wasm"` 只是一行配置差异
5. **与现有架构兼容**：在现有 `adapters/wasm/` 旁增加 `adapters/sidecar/` 和 `core/plugin/`，不对现有代码做破坏性改动

### 🟡 风险与缓解

| 风险                                            | 缓解措施                                                    |
| ----------------------------------------------- | ----------------------------------------------------------- |
| Go Sidecar 部署复杂度（用户需安装 Go 运行时？） | Sidecar 编译为独立 Go 二进制（静态链接），零运行时依赖      |
| Rust↔Go IPC 性能                                | Arrow 大块数据走共享内存，控制面走 Unix Socket              |
| 双引擎心智负担                                  | 统一 `plugin.toml` + Protobuf IDL，引擎差异对插件开发者透明 |
| WASM WASI 标准不稳定                            | 先用 Extism SDK（抽象了 WASI 细节），WASI 稳定后再切换      |
| Windows Named Pipe 支持                         | gRPC 在 Windows 上走 `localhost` TCP loopback，性能损耗可控 |

---

## 版本历史

| 版本 | 日期       | 变更                                                                                |
| ---- | ---------- | ----------------------------------------------------------------------------------- |
| v1.0 | 2026-05-10 | 初始版本，双引擎架构设计、plugin.toml 清单、Protobuf API 定义、生命周期、实施路线图 |
