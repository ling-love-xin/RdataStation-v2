# RdataStation 插件系统设计文档

## Overview

- **Summary**: 设计一个类似 VSCode 的插件系统，支持前端扩展、WASM 插件和 Go-Sidecar 驱动插件，实现完整的插件生命周期管理、贡献点系统和安全隔离机制。
- **Purpose**: 为 RdataStation 提供可扩展的插件生态，允许开发者扩展功能而不修改核心代码。
- **Target Users**: 插件开发者、RdataStation 用户、系统管理员

## Goals

- 实现完整的插件生命周期管理（安装、激活、停用、卸载）
- 支持三种插件类型：前端扩展、WASM 插件、Go-Sidecar 驱动插件
- 提供丰富的贡献点系统（命令、面板、驱动、设置、菜单）
- 实现安全的沙箱隔离机制
- 提供插件市场集成支持

## Non-Goals (Out of Scope)

- 插件市场前端界面（仅提供后端 API）
- 插件打包工具（建议使用 npm/rust cargo）
- 插件国际化系统
- 插件调试器

## Background & Context

- 现有基础：`core/plugin/manifest.rs` 已定义插件清单结构
- WASM 支持：`adapters/wasm/` 已集成 Extism 运行时
- Sidecar 支持：`adapters/sidecar/` 已实现 Go 侧通信
- 参考架构：VSCode Extension API + Eclipse Plugin Framework

## Functional Requirements

- **FR-1**: 插件清单解析与验证
- **FR-2**: 插件加载与激活（按需/启动时）
- **FR-3**: 插件贡献点注册（命令、面板、驱动、设置）
- **FR-4**: 插件通信机制（前端 ↔ 后端）
- **FR-5**: 插件生命周期管理（安装、更新、卸载）
- **FR-6**: 插件沙箱隔离（资源限制、权限控制）
- **FR-7**: 插件市场集成（搜索、安装、更新）
- **FR-8**: 插件状态持久化

## Non-Functional Requirements

- **NFR-1**: 插件加载时间 < 500ms
- **NFR-2**: 插件崩溃不影响主程序稳定性
- **NFR-3**: 内存限制可配置（默认 512MB/插件）
- **NFR-4**: 支持热更新（无需重启）
- **NFR-5**: 插件权限最小化原则

## Constraints

- **Technical**: Rust 2021, Tauri 2.x, Extism 43.x, Go 1.21+
- **Business**: 插件必须签名验证（生产环境）
- **Dependencies**: 依赖外部 WASM 运行时和 Go Sidecar 进程

## Assumptions

- 用户已熟悉 VSCode 插件概念
- 插件开发者使用 TypeScript/Rust/Go 进行开发
- 插件通过 HTTP(S) 或本地文件系统安装

---

## 架构设计

### 整体架构

```
┌─────────────────────────────────────────────────────────────────────────┐
│                        RdataStation 插件系统                           │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  ┌─────────────────┐    IPC/RPC    ┌─────────────────────────────────┐ │
│  │   前端扩展层     │ ────────────►│       插件管理器                  │ │
│  │ (Vue/TypeScript)│               │   (Plugin Manager)               │ │
│  └────────┬────────┘               └──────────────┬──────────────────┘ │
│           │                                        │                     │
│           │ 消息通信                                │                     │
│           ▼                                        ▼                     │
│  ┌─────────────────┐               ┌─────────────────────────────────┐ │
│  │   WASM 插件层    │               │         贡献点注册表             │ │
│  │ (Rust/C/Go WASM)│               │   (Contribution Registry)      │ │
│  └────────┬────────┘               └──────────────┬──────────────────┘ │
│           │                                        │                     │
│           │ 宿主函数调用                            │                     │
│           ▼                                        ▼                     │
│  ┌─────────────────┐               ┌─────────────────────────────────┐ │
│  │  Go-Sidecar层   │               │           核心服务层             │ │
│  │ (数据库驱动)     │               │  (Connection/SQL/Driver)       │ │
│  └─────────────────┘               └─────────────────────────────────┘ │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

### 插件类型架构

| 类型             | 运行位置   | 语言支持         | 用途                   | 隔离级别     |
| ---------------- | ---------- | ---------------- | ---------------------- | ------------ |
| **前端扩展**     | 渲染进程   | TypeScript/Vue   | UI 扩展、命令注册      | 渲染进程隔离 |
| **WASM 插件**    | 主进程沙箱 | Rust/C/Go → WASM | 数据分析、工具函数     | WASM 沙箱    |
| **Sidecar 插件** | 独立进程   | Go               | 数据库驱动、系统级任务 | 进程隔离     |

### 目录结构

```
src-tauri/src/
├── core/
│   └── plugin/
│       ├── manifest.rs      # 插件清单定义
│       ├── manager.rs       # 插件管理器核心
│       ├── registry.rs      # 贡献点注册表
│       ├── lifecycle.rs     # 生命周期管理
│       ├── sandbox.rs       # 沙箱隔离
│       └── types.rs         # 类型定义
├── adapters/
│   ├── wasm/                # WASM 运行时适配
│   ├── sidecar/             # Go-Sidecar 适配
│   └── frontend/            # 前端扩展适配
├── commands/
│   └── plugin_commands.rs   # 插件管理命令
└── persistence/
    └── plugin_store.rs      # 插件状态持久化
```

---

## 核心组件设计

### 1. 插件清单 (Plugin Manifest)

参考现有 `core/plugin/manifest.rs`，完整结构如下：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub plugin: PluginMeta,
    pub capabilities: PluginCapabilities,
    pub permissions: PluginPermissions,
    pub contributes: PluginContributes,
    pub dependencies: Vec<PluginDependency>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMeta {
    pub id: String,                 // 唯一标识，如 "com.example.driver"
    pub name: String,               // 显示名称
    pub version: String,            // 版本号 (semver)
    pub publisher: String,          // 发布者
    pub description: String,        // 描述
    pub icon: Option<String>,       // 图标路径
    pub homepage: Option<String>,   // 主页
    pub license: Option<String>,    // 许可证
    pub engines: PluginEngines,     // 引擎版本要求
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginCapabilities {
    pub frontend: Option<CapabilitiesFrontend>,  // 前端扩展
    pub wasm: Option<CapabilitiesWasm>,          // WASM 插件
    pub sidecar: Option<CapabilitiesSidecar>,    // Sidecar 插件
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilitiesFrontend {
    pub entry: String,                    // 入口文件路径
    pub activation_events: Vec<String>,   // 激活事件列表
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilitiesWasm {
    pub entry: String,                    // WASM 文件路径
    pub max_memory_mb: Option<usize>,     // 内存限制
    pub max_cpu_time_ms: Option<u64>,     // CPU 时间限制
    pub allowed_host_functions: Vec<String>, // 允许的宿主函数
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilitiesSidecar {
    pub entry: String,                    // 可执行文件路径
    pub protocol: String,                 // 通信协议 (grpc/jsonrpc)
    pub requires_jvm: bool,              // 是否需要 JVM
}
```

**贡献点结构**：

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginContributes {
    pub commands: Vec<ContributesCommand>,   // 命令
    pub panels: Vec<ContributesPanel>,       // 面板
    pub drivers: Vec<ContributesDriver>,     // 数据库驱动
    pub settings: Vec<ContributesSetting>,   // 设置项
    pub menus: Option<serde_json::Value>,    // 菜单
    pub keyboard_shortcuts: Vec<Shortcut>,   // 快捷键
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributesCommand {
    pub id: String,
    pub title: String,
    pub category: Option<String>,
    pub icon: Option<String>,
    pub shortcut: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributesPanel {
    pub id: String,
    pub title: String,
    pub location: String,        // left/right/bottom/center
    pub icon: Option<String>,
    pub order: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributesDriver {
    pub id: String,
    pub display_name: String,
    pub default_port: Option<u16>,
    pub connection_schema: Option<String>,  // JSON Schema
    pub features: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributesSetting {
    pub key: String,
    pub setting_type: String,    // string/boolean/number/enum
    pub default: serde_json::Value,
    pub label: Option<String>,
    pub description: Option<String>,
    pub enum_values: Option<Vec<String>>,  // 枚举值
}
```

### 2. 插件管理器 (Plugin Manager)

```rust
pub trait PluginManager {
    // 加载插件
    fn load_plugin(&mut self, manifest_path: &Path) -> Result<PluginId, PluginError>;

    // 卸载插件
    fn unload_plugin(&mut self, id: &PluginId) -> Result<(), PluginError>;

    // 激活插件（触发激活事件）
    fn activate_plugin(&mut self, id: &PluginId) -> Result<(), PluginError>;

    // 停用插件
    fn deactivate_plugin(&mut self, id: &PluginId) -> Result<(), PluginError>;

    // 获取插件状态
    fn get_plugin_status(&self, id: &PluginId) -> Option<PluginStatus>;

    // 获取已加载插件列表
    fn list_plugins(&self) -> Vec<PluginInfo>;

    // 调用插件方法（前端/WASM）
    fn call_plugin_method(
        &self,
        id: &PluginId,
        method: &str,
        args: serde_json::Value
    ) -> Result<serde_json::Value, PluginError>;

    // 广播事件到所有插件
    fn broadcast_event(&self, event: &PluginEvent) -> Result<(), PluginError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginStatus {
    Installed,    // 已安装但未加载
    Loaded,       // 已加载但未激活
    Active,       // 已激活运行中
    Disabled,     // 已禁用
    Error,        // 出错状态
}

pub struct PluginInfo {
    pub id: PluginId,
    pub name: String,
    pub version: String,
    pub status: PluginStatus,
    pub capabilities: PluginCapabilities,
    pub resource_usage: Option<ResourceUsage>,
}
```

### 3. 贡献点注册表 (Contribution Registry)

```rust
pub struct ContributionRegistry {
    commands: HashMap<CommandId, RegisteredCommand>,
    panels: HashMap<PanelId, RegisteredPanel>,
    drivers: HashMap<DriverId, RegisteredDriver>,
    settings: HashMap<String, RegisteredSetting>,
    menus: Vec<MenuContribution>,
    shortcuts: HashMap<ShortcutKey, CommandId>,
}

impl ContributionRegistry {
    // 注册命令
    fn register_command(&mut self, plugin_id: &PluginId, command: ContributesCommand);

    // 注册面板
    fn register_panel(&mut self, plugin_id: &PluginId, panel: ContributesPanel);

    // 注册驱动
    fn register_driver(&mut self, plugin_id: &PluginId, driver: ContributesDriver);

    // 注册设置
    fn register_setting(&mut self, plugin_id: &PluginId, setting: ContributesSetting);

    // 根据 ID 获取命令
    fn get_command(&self, id: &CommandId) -> Option<&RegisteredCommand>;

    // 获取所有命令
    fn get_all_commands(&self) -> Vec<&RegisteredCommand>;

    // 根据插件 ID 移除所有贡献
    fn remove_by_plugin(&mut self, plugin_id: &PluginId);
}
```

### 4. 沙箱隔离 (Sandbox)

```rust
pub struct PluginSandbox {
    config: PluginSandboxConfig,
    resource_tracker: ResourceTracker,
}

#[derive(Debug, Clone)]
pub struct PluginSandboxConfig {
    max_memory_mb: usize,
    max_cpu_time_ms: u64,
    max_files: usize,
    allowed_paths: Vec<PathBuf>,
    allowed_hosts: Vec<String>,
    allowed_host_functions: Vec<String>,
}

#[derive(Debug)]
pub struct ResourceUsage {
    memory_usage_mb: usize,
    cpu_time_ms: u64,
    file_access_count: usize,
    network_request_count: usize,
}

impl PluginSandbox {
    // 创建沙箱实例
    fn new(config: PluginSandboxConfig) -> Self;

    // 检查资源使用
    fn check_resource_limits(&self, usage: &ResourceUsage) -> Result<(), SandboxError>;

    // 验证文件访问权限
    fn check_file_access(&self, path: &Path) -> Result<(), SandboxError>;

    // 验证网络访问权限
    fn check_network_access(&self, host: &str) -> Result<(), SandboxError>;

    // 获取资源使用统计
    fn get_resource_usage(&self) -> ResourceUsage;
}
```

### 5. 插件事件系统

```rust
#[derive(Debug, Clone)]
pub enum PluginEvent {
    // 应用启动完成
    ApplicationStarted,

    // 插件加载完成
    PluginLoaded(PluginId),

    // 插件激活
    PluginActivated(PluginId),

    // 插件停用
    PluginDeactivated(PluginId),

    // 配置变更
    ConfigurationChanged {
        key: String,
        old_value: Option<serde_json::Value>,
        new_value: serde_json::Value,
    },

    // 连接状态变更
    ConnectionChanged {
        connection_id: String,
        status: ConnectionStatus,
    },

    // 自定义事件
    Custom {
        name: String,
        data: serde_json::Value,
    },
}

pub trait EventHandler {
    fn handle_event(&self, event: &PluginEvent) -> Result<(), PluginError>;
}
```

---

## 插件生命周期

### 完整生命周期流程

```
安装 → 加载 → 激活 → 运行 → 停用 → 卸载
    │       │       │       │       │
    ▼       ▼       ▼       ▼       ▼
  解压    解析     注册     执行    清理
  插件    清单     贡献点   命令    资源
```

### 详细阶段说明

| 阶段     | 触发条件           | 执行操作                         | 状态变更  |
| -------- | ------------------ | -------------------------------- | --------- |
| **安装** | 用户安装插件       | 下载、解压、验证签名             | Installed |
| **加载** | 应用启动或手动触发 | 解析清单、验证兼容性、加载代码   | Loaded    |
| **激活** | 激活事件触发       | 注册贡献点、调用 activate 钩子   | Active    |
| **运行** | 激活后             | 处理命令调用、响应事件           | Active    |
| **停用** | 用户停用或应用关闭 | 移除贡献点、调用 deactivate 钩子 | Loaded    |
| **卸载** | 用户卸载           | 删除文件、清理状态               | 已删除    |

### 激活事件类型

```rust
pub enum ActivationEvent {
    // 应用启动时激活
    OnStartup,

    // 命令被调用时激活
    OnCommand(String),

    // 面板被打开时激活
    OnPanel(String),

    // 设置被访问时激活
    OnSetting(String),

    // 特定数据库连接时激活
    OnDatabaseConnection(String),

    // 自定义事件激活
    OnEvent(String),

    // 工作区包含特定文件类型时激活
    OnWorkspaceContains(String),
}
```

---

## 插件通信机制

### 前端 ↔ 后端通信

```
┌─────────────────┐          JSON-RPC          ┌─────────────────┐
│   前端扩展       │ ────────────────────────► │   插件管理器     │
│ (Vue Extension) │ ◄─────────────────────── │   (Rust)        │
└─────────────────┘     HTTP/WebSocket        └─────────────────┘
```

### 通信协议

采用 JSON-RPC 2.0 协议：

```json
// 请求
{
"jsonrpc": "2.0",
"id": 1,
"method": "plugin.call",
"params": {
    "plugin_id": "com.example.analytics",
    "method": "analyze",
    "args": {"query": "SELECT * FROM users"}
}
}

// 响应
{
"jsonrpc": "2.0",
"id": 1,
"result": {"columns": [...], "rows": [...]},
"error": null
}
```

### WASM 宿主函数

```rust
// 宿主函数定义
pub struct HostFunctions;

impl HostFunctions {
    // 日志函数
    pub fn log(level: LogLevel, message: &str);

    // 数据库查询
    pub fn db_query(connection_id: &str, sql: &str) -> Result<QueryResult, HostFunctionError>;

    // 获取配置
    pub fn get_config(key: &str) -> Option<serde_json::Value>;

    // 设置配置
    pub fn set_config(key: &str, value: serde_json::Value) -> Result<(), HostFunctionError>;

    // 发送事件
    pub fn emit_event(name: &str, data: &[u8]);

    // HTTP 请求
    pub fn http_request(request: HttpRequest) -> Result<HttpResponse, HostFunctionError>;

    // 文件系统操作
    pub fn fs_read(path: &str) -> Result<Vec<u8>, HostFunctionError>;
    pub fn fs_write(path: &str, data: &[u8]) -> Result<(), HostFunctionError>;
}
```

---

## 安全设计

### 权限系统

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginPermissions {
    pub frontend: Vec<String>,   // 前端权限
    pub wasm: Vec<String>,       // WASM 权限
    pub sidecar: Vec<String>,    // Sidecar 权限
}

// 权限常量定义
pub mod permissions {
    // 数据访问权限
    pub const DATA_QUERY: &str = "data:query";
    pub const DATA_WRITE: &str = "data:write";
    pub const DATA_METADATA: &str = "data:metadata";

    // 文件系统权限
    pub const FS_READ: &str = "fs:read";
    pub const FS_WRITE: &str = "fs:write";
    pub const FS_EXECUTE: &str = "fs:execute";

    // 网络权限
    pub const NETWORK_HTTP: &str = "network:http";
    pub const NETWORK_WEBSOCKET: &str = "network:websocket";

    // 系统权限
    pub const SYSTEM_PROCESS: &str = "system:process";
    pub const SYSTEM_ENV: &str = "system:env";

    // 设置权限
    pub const SETTINGS_READ: &str = "settings:read";
    pub const SETTINGS_WRITE: &str = "settings:write";
}
```

### 权限验证流程

```
插件请求 → 权限检查 → 资源限制检查 → 执行操作 → 返回结果
            ↓                ↓
        权限不足         资源超限
            ↓                ↓
        拒绝请求         拒绝请求
```

### 签名验证

生产环境下，插件必须经过数字签名验证：

```rust
pub struct PluginSignature {
    pub signature: Vec<u8>,      // 签名数据
    pub public_key: Vec<u8>,     // 公钥
    pub certificate: Option<Vec<u8>>,  // 证书链
    pub timestamp: u64,          // 签名时间戳
}

impl PluginManager {
    fn verify_plugin_signature(&self, manifest_path: &Path) -> Result<bool, PluginError> {
        // 1. 读取插件签名文件
        // 2. 获取发布者公钥
        // 3. 验证签名有效性
        // 4. 验证证书链（可选）
        // 5. 返回验证结果
    }
}
```

---

## API 设计

### 插件管理 API

| 方法                | 描述         | 参数                      | 返回值         |
| ------------------- | ------------ | ------------------------- | -------------- |
| `plugin.install`    | 安装插件     | `url: string`             | `PluginInfo`   |
| `plugin.uninstall`  | 卸载插件     | `plugin_id: string`       | `void`         |
| `plugin.load`       | 加载插件     | `plugin_id: string`       | `void`         |
| `plugin.unload`     | 卸载插件     | `plugin_id: string`       | `void`         |
| `plugin.activate`   | 激活插件     | `plugin_id: string`       | `void`         |
| `plugin.deactivate` | 停用插件     | `plugin_id: string`       | `void`         |
| `plugin.list`       | 获取插件列表 | 无                        | `PluginInfo[]` |
| `plugin.get`        | 获取插件信息 | `plugin_id: string`       | `PluginInfo`   |
| `plugin.call`       | 调用插件方法 | `plugin_id, method, args` | `any`          |
| `plugin.update`     | 更新插件     | `plugin_id: string`       | `PluginInfo`   |

### 插件市场 API

| 方法             | 描述         | 参数                                  | 返回值                 |
| ---------------- | ------------ | ------------------------------------- | ---------------------- |
| `market.search`  | 搜索插件     | `query: string, filters: object`      | `PluginSearchResult[]` |
| `market.get`     | 获取插件详情 | `plugin_id: string`                   | `PluginDetails`        |
| `market.install` | 从市场安装   | `plugin_id: string, version?: string` | `PluginInfo`           |
| `market.updates` | 检查更新     | 无                                    | `UpdateInfo[]`         |

---

## 数据持久化

### 插件状态存储

```rust
pub struct PluginStore {
    db: DatabaseConnection,
}

impl PluginStore {
    // 保存插件信息
    fn save_plugin(&self, info: &PluginInfo) -> Result<(), StorageError>;

    // 获取插件信息
    fn get_plugin(&self, id: &PluginId) -> Result<Option<PluginInfo>, StorageError>;

    // 获取所有插件
    fn get_all_plugins(&self) -> Result<Vec<PluginInfo>, StorageError>;

    // 删除插件记录
    fn delete_plugin(&self, id: &PluginId) -> Result<(), StorageError>;

    // 更新插件状态
    fn update_plugin_status(&self, id: &PluginId, status: PluginStatus) -> Result<(), StorageError>;

    // 获取已启用的插件列表
    fn get_enabled_plugins(&self) -> Result<Vec<PluginInfo>, StorageError>;
}
```

### 数据库表结构

```sql
-- 插件信息表
CREATE TABLE plugins (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    publisher TEXT NOT NULL,
    description TEXT,
    icon TEXT,
    status TEXT NOT NULL DEFAULT 'installed',
    install_path TEXT NOT NULL,
    last_updated INTEGER,
    enabled BOOLEAN NOT NULL DEFAULT 1
);

-- 插件设置表
CREATE TABLE plugin_settings (
    plugin_id TEXT NOT NULL,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    PRIMARY KEY (plugin_id, key),
    FOREIGN KEY (plugin_id) REFERENCES plugins(id)
);

-- 插件贡献点注册表
CREATE TABLE plugin_contributions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    plugin_id TEXT NOT NULL,
    type TEXT NOT NULL,  -- command/panel/driver/setting
    data TEXT NOT NULL,  -- JSON 数据
    FOREIGN KEY (plugin_id) REFERENCES plugins(id)
);
```

---

## 错误处理

### 错误类型

```rust
#[derive(Debug)]
pub enum PluginError {
    // 清单错误
    ManifestError(String),

    // 加载错误
    LoadError(String),

    // 激活错误
    ActivationError(String),

    // 权限错误
    PermissionError(String),

    // 资源限制错误
    ResourceLimitError(String),

    // 通信错误
    CommunicationError(String),

    // 签名验证错误
    SignatureError(String),

    // 兼容性错误
    CompatibilityError(String),

    // 存储错误
    StorageError(StorageError),

    // Core 错误
    CoreError(CoreError),
}
```

### 错误处理策略

1. **清单错误**：记录日志，拒绝加载
2. **加载错误**：回滚状态，保持系统稳定
3. **运行时错误**：隔离插件，不影响其他插件
4. **权限错误**：拒绝操作，通知用户
5. **资源限制**：暂停插件，通知用户

---

## 性能优化

### 按需加载

```rust
impl PluginManager {
    fn load_plugin_on_demand(&mut self, activation_event: &ActivationEvent) {
        // 根据激活事件查找需要加载的插件
        let plugins_to_load = self.find_plugins_for_event(activation_event);

        for plugin_id in plugins_to_load {
            if !self.is_plugin_loaded(plugin_id) {
                self.load_plugin(plugin_id);
            }

            if !self.is_plugin_active(plugin_id) {
                self.activate_plugin(plugin_id);
            }
        }
    }
}
```

### 缓存策略

```rust
pub struct PluginCache {
    manifest_cache: HashMap<PluginId, PluginManifest>,
    contribution_cache: HashMap<PluginId, PluginContributes>,
    last_access: HashMap<PluginId, u64>,
    max_cache_size: usize,
}

impl PluginCache {
    // 获取缓存的清单
    fn get_manifest(&mut self, id: &PluginId) -> Option<&PluginManifest>;

    // 缓存清单
    fn cache_manifest(&mut self, id: PluginId, manifest: PluginManifest);

    // 清理过期缓存
    fn cleanup(&mut self);
}
```

---

## 测试策略

### 单元测试

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_validation() {
        // 测试清单解析和验证
    }

    #[test]
    fn test_plugin_loading() {
        // 测试插件加载流程
    }

    #[test]
    fn test_permission_check() {
        // 测试权限验证
    }

    #[test]
    fn test_resource_limits() {
        // 测试资源限制
    }

    #[test]
    fn test_event_broadcast() {
        // 测试事件广播机制
    }
}
```

### 集成测试

1. **插件安装/卸载流程**
2. **插件激活/停用流程**
3. **贡献点注册/注销**
4. **跨插件通信**
5. **沙箱隔离验证**
6. **资源限制测试**

### 端到端测试

1. **完整插件生命周期**
2. **插件市场集成**
3. **多插件共存**
4. **插件错误处理**
5. **性能基准测试**

---

## 部署与集成

### 插件目录结构

```
~/.rdatastation/plugins/
├── com.example.driver/
│   ├── rdata-plugin.toml    # 插件清单
│   ├── extension.ts         # 前端扩展入口
│   ├── plugin.wasm          # WASM 模块（可选）
│   ├── sidecar/             # Sidecar 目录（可选）
│   │   └── driver.go
│   └── assets/              # 资源文件
│       └── icon.png
└── com.example.analytics/
    ├── rdata-plugin.toml
    ├── extension.ts
    └── plugin.wasm
```

### 插件打包格式

插件打包为 `.rdp` 文件（ZIP 压缩）：

```
plugin-name-1.0.0.rdp
├── rdata-plugin.toml
├── extension.ts
├── plugin.wasm (可选)
└── assets/
```

### 启动加载流程

```
应用启动
    │
    ▼
加载已启用插件列表（从数据库）
    │
    ▼
并行加载插件清单
    │
    ▼
验证兼容性和签名
    │
    ▼
注册贡献点
    │
    ▼
触发 OnStartup 激活事件
    │
    ▼
等待按需激活插件
```

---

## 扩展与定制

### 添加新的贡献点类型

```rust
// 1. 在 PluginContributes 中添加新字段
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginContributes {
    // ... 现有字段 ...
    pub custom_widgets: Vec<ContributesCustomWidget>,  // 新贡献点
}

// 2. 定义新贡献点结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributesCustomWidget {
    pub id: String,
    pub type: String,
    pub config: serde_json::Value,
}

// 3. 在 ContributionRegistry 中添加注册方法
impl ContributionRegistry {
    fn register_custom_widget(&mut self, plugin_id: &PluginId, widget: ContributesCustomWidget) {
        // 注册逻辑
    }
}
```

### 添加新的宿主函数

```rust
// 1. 在 HostFunctions 中添加新方法
impl HostFunctions {
    pub fn custom_api_call(arg1: &str, arg2: i32) -> Result<String, HostFunctionError> {
        // 实现逻辑
    }
}

// 2. 在 WASM 适配器中注册
impl ExtismPluginManager {
    fn register_host_functions(&mut self) {
        // ... 注册现有函数 ...
        self.extism_context.register_function("custom_api_call", custom_api_call_handler);
    }
}
```

---

## 未来规划

### 版本 1.0（基础版）

- [ ] 插件清单解析
- [ ] 插件加载与激活
- [ ] 命令贡献点
- [ ] 面板贡献点
- [ ] WASM 插件支持

### 版本 1.1（扩展版）

- [ ] 驱动贡献点
- [ ] 设置贡献点
- [ ] 菜单贡献点
- [ ] 快捷键贡献点

### 版本 1.2（高级版）

- [ ] 插件市场集成
- [ ] 插件签名验证
- [ ] 热更新支持
- [ ] 插件调试支持

### 版本 2.0（完善版）

- [ ] 插件国际化
- [ ] 插件依赖管理
- [ ] 插件性能监控
- [ ] 插件协作开发支持

---

## 附录

### 插件清单示例

```toml
[plugin]
id = "com.example.database-driver"
name = "My Database Driver"
version = "1.0.0"
publisher = "Example Corp"
description = "A custom database driver plugin"
icon = "icon.png"
homepage = "https://example.com"
license = "MIT"

[plugin.engines]
rdatastation = "^1.0.0"

[capabilities.sidecar]
entry = "./sidecar/driver.exe"
protocol = "grpc"
requires_jvm = false

[permissions]
sidecar = ["data:query", "data:write"]

[[contributes.drivers]]
id = "my-custom-db"
display_name = "My Custom Database"
default_port = 5432
connection_schema = "./schema.json"
features = ["tables", "views", "stored_procedures"]

[[contributes.commands]]
id = "my-db.connect"
title = "Connect to My Database"
category = "Database"
icon = "database"
```

### 宿主函数列表

| 函数名       | 描述      | 参数                                        | 返回值          |
| ------------ | --------- | ------------------------------------------- | --------------- |
| `log`        | 记录日志  | `level: LogLevel, message: string`          | `void`          |
| `db_query`   | 执行查询  | `conn_id: string, sql: string`              | `QueryResult`   |
| `db_execute` | 执行命令  | `conn_id: string, sql: string`              | `ExecuteResult` |
| `get_config` | 获取配置  | `key: string`                               | `any`           |
| `set_config` | 设置配置  | `key: string, value: any`                   | `void`          |
| `emit_event` | 发送事件  | `name: string, data: bytes`                 | `void`          |
| `http_get`   | HTTP GET  | `url: string, headers: object`              | `HttpResponse`  |
| `http_post`  | HTTP POST | `url: string, body: bytes, headers: object` | `HttpResponse`  |
| `fs_read`    | 读取文件  | `path: string`                              | `bytes`         |
| `fs_write`   | 写入文件  | `path: string, data: bytes`                 | `void`          |
| `fs_list`    | 列出目录  | `path: string`                              | `string[]`      |

### 权限矩阵

| 权限             | 前端扩展 | WASM 插件 | Sidecar 插件 | 默认值 |
| ---------------- | -------- | --------- | ------------ | ------ |
| `data:query`     | ✅       | ✅        | ✅           | 否     |
| `data:write`     | ❌       | ✅        | ✅           | 否     |
| `data:metadata`  | ✅       | ✅        | ✅           | 是     |
| `fs:read`        | ❌       | ✅        | ✅           | 否     |
| `fs:write`       | ❌       | ✅        | ✅           | 否     |
| `network:http`   | ❌       | ✅        | ✅           | 否     |
| `settings:read`  | ✅       | ✅        | ✅           | 是     |
| `settings:write` | ❌       | ✅        | ✅           | 否     |
