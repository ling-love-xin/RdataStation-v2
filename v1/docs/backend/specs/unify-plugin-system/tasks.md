# Tasks: 统一插件系统

> 对应 Spec: `.trae/specs/unify-plugin-system/spec.md`
> 总任务数：7 个主任务，可并行展开

---

## Task 1: 定义 Manifest 数据模型与解析器

- [ ] **SubTask 1.1**: 在 `src-tauri/src/core/` 下创建 `plugin/` 模块目录（`mod.rs` + `manifest.rs`）
  - 定义 `PluginManifest` 结构体（含 `PluginMeta`、`PluginCapabilities`、`PluginPermissions`、`PluginContributes`、`PluginDependency`）
  - 定义 `CapabilitiesFrontend`、`CapabilitiesWasm`、`PermissionsLevel`
  - 定义 `ContributesCommand`、`ContributesPanel`、`ContributesDriver`、`ContributesSetting`
  - 所有结构体 `#[derive(Debug, Clone, Serialize, Deserialize)]`
- [ ] **SubTask 1.2**: 实现 `ManifestParser::parse(path: &Path) -> Result<PluginManifest, CoreError>`
  - 读取并解析 TOML 文件
  - 验证必需字段：`plugin.id`、`plugin.name`、`plugin.version`、`publisher`
  - 验证 `engines.rdatastation` 版本兼容（semver 匹配）
  - 验证至少存在 `capabilities.frontend` 或 `capabilities.wasm` 之一
  - 错误信息包含具体的字段名和期望值
- [ ] **SubTask 1.3**: 在 `core/plugin/mod.rs` 中 `pub use manifest::*`，在 `core/mod.rs` 中注册 `pub mod plugin`
- [ ] **SubTask 1.4**: 在 `src/extensions/core/types.ts` 中新增 TypeScript 侧 `PluginManifest` 类型定义
  - 镜像 Rust 侧结构体，使用 `interface` + `type`
  - 导出 `PluginManifest`、`PluginCapabilities`、`PluginPermissions`、`PluginContributes`

**验证**: `cargo build` 通过 + `pnpm run lint` 通过

---

## Task 2: 迁移 8 个内置扩展 package.json → rdata-plugin.toml

- [ ] **SubTask 2.1**: 创建 `workbench` 的 `rdata-plugin.toml`
  - `[plugin]` 段：id=`rdatastation.workbench`，name=`Workbench`，version=`1.0.0`
  - `[capabilities.frontend]`：entry=`./extension.ts`
  - `[contributes.panels]`：声明 6 个面板（emptyWorkbench、sqlHistory、outputPanel、plugins、mockPanel、dynamicObjectProperties）
  - `[contributes.commands]`：workbench.openPanel / workbench.closePanel / workbench.focusPanel
- [ ] **SubTask 2.2**: 创建 `connection` 的 `rdata-plugin.toml`
  - id=`rdatastation.connection`，迁移 `package.json` 中的 commands/menus/views
- [ ] **SubTask 2.3**: 创建 `database` 的 `rdata-plugin.toml`
- [ ] **SubTask 2.4**: 创建 `query` 的 `rdata-plugin.toml`
- [ ] **SubTask 2.5**: 创建 `analytics-resource` 的 `rdata-plugin.toml`
- [ ] **SubTask 2.6**: 创建 `mysql-driver` 的 `rdata-plugin.toml`
- [ ] **SubTask 2.7**: 创建 `scratchpad` 的 `rdata-plugin.toml`
- [ ] **SubTask 2.8**: 创建 `settings` 的 `rdata-plugin.toml`

> 每个 `rdata-plugin.toml` 都可以通过 `cargo test` 中的 `ManifestParser::parse()` 测试来验证格式正确性。

**验证**: 所有 8 个 manifest 文件通过 Rust 侧 `ManifestParser::parse()` 解析无错误

---

## Task 3: 实现 PluginHost Loader（前端 TS 侧）

- [ ] **SubTask 3.1**: 在 `src/core/` 下创建 `plugin-loader.ts`
  - 实现 `PluginLoader` 类，负责扫描插件目录
  - `discoverPlugins()`: 返回 `DiscoveredPlugin[]`（id、path、manifest、source: builtin|user|project）
  - 扫描顺序：`project/.rdata/plugins/` → `{data_dir}/plugins/` → `builtinExtensions`（内置列表）
  - 同名 ID 去重：project > user > builtin
- [ ] **SubTask 3.2**: 实现 `PluginLoader.validateManifest(manifest: PluginManifest): ValidationResult`
  - 检查必需字段完整性
  - 检查 `engines.rdatastation` 版本兼容
  - 返回 `{ valid: boolean, errors: string[] }`
- [ ] **SubTask 3.3**: 实现 `PluginLoader.resolveDependencies(plugins: DiscoveredPlugin[]): DependencyGraph`
  - 拓扑排序，检测循环依赖
  - 缺失依赖标记为 Error
- [ ] **SubTask 3.4**: 重构 `src/core/builtin-extensions.ts`
  - 新增 `loadBuiltinManifests()` 函数，从 8 个 `rdata-plugin.toml` 读取 manifest（替代硬编码列表）
  - 向后兼容：保留 `builtinExtensions: BuiltinExtension[]` 导出（内部改为从 manifest 合并生成）

**验证**: `pnpm run lint` 通过

---

## Task 4: 重构 PluginContext（前端 TS 侧）

- [ ] **SubTask 4.1**: 在 `src/extensions/core/types.ts` 中定义新 `PluginContext` 接口
  - 保留：`project: ProjectInfo`、`extensionPath: string`、`subscribe()`
  - 新增：`pluginId: string`、`manifest: PluginManifest`
  - 新增：`logging: { info, warn, error }`
  - 新增：`storage: { get<T>, set<T>, delete, keys }`（ScopedStorage）
  - 新增：`events: EventBus`（现有 `src/extensions/core/event-bus.ts` 已实现）
  - 重命名：`window` → `panels`（API 不变）
  - 改造：`database` → 实现真实对接（新增 `query`、`getActiveConnection`、`getMetadata`）
  - 移除：`workspace`（合并到 `system.fs`）
  - 移除：`configuration`（合并到 `storage`）
  - 移除：`sqlEditor`（暂移除，后续再设计）
  - 移除：`utils`（暂移除，后续再设计）
- [ ] **SubTask 4.2**: 实现 `ScopedStorage`（`src/core/scoped-storage.ts`）
  - 以 `pluginId` 作为命名空间前缀
  - `get<T>(key)` → `tauri.invoke('plugin_storage_get', { pluginId, key })`
  - `set<T>(key, value)` → `tauri.invoke('plugin_storage_set', { pluginId, key, value })`
  - `delete(key)` → `tauri.invoke('plugin_storage_delete', { pluginId, key })`
  - `keys()` → `tauri.invoke('plugin_storage_keys', { pluginId })`
- [ ] **SubTask 4.3**: 重构 `ExtensionHost` → `PluginHost`（`src/core/extension-host.ts`）
  - 激活时构建完整的 `PluginContext`（注入 `storage`、`logging`、`events` 等新字段）
  - `logging` 对接 `console` 带 `[Plugin:{id}]` 前缀
  - `storage` 实例化为 `new ScopedStorage(pluginId)`

**验证**: `pnpm run lint` + `pnpm run typecheck` 通过

---

## Task 5: 实现 Rust 侧 Wasm Host Functions

- [ ] **SubTask 5.1**: 在 `src-tauri/src/adapters/wasm/` 下创建 `host_functions.rs`
  - 定义 `HostFunctionRegistry` 结构体，管理所有 host function 注册
  - `fn register_all(plugin: &mut Plugin, plugin_id: &str) -> Result<()>`
- [ ] **SubTask 5.2**: 实现 `host_db_query`
  - 签名：`(plugin_id: String, conn_id: String, sql: String) → Vec<u8>`
  - 实现：校验权限 → 获取 `ConnectionManager` → 调用 `connection.query(sql)` → 序列化 `QueryResult.batches` 为 Arrow IPC Stream → 返回字节
  - 权限校验：检查 manifest 中是否声明 `wasm:db_query`
- [ ] **SubTask 5.3**: 实现 `host_db_metadata`
  - 签名：`(plugin_id: String, conn_id: String, catalog: String, schema: String, kind: String) → Vec<u8>`
  - 实现：校验权限 → 调用 `Database.list_tables()` / `list_columns()` 等方法 → 序列化为 JSON
- [ ] **SubTask 5.4**: 实现 `host_duckdb_query`
  - 签名：`(plugin_id: String, sql: String) → Vec<u8>`
  - 实现：校验权限 → 获取 `DuckDBManager` → 执行分析 SQL → 返回 Arrow IPC Stream
- [ ] **SubTask 5.5**: 实现 `host_duckdb_load`
  - 签名：`(plugin_id: String, table_name: String, arrow_bytes: Vec<u8>) → ()`
  - 实现：校验权限 → 反序列化 Arrow IPC Stream → `DuckDBManager` 创建临时表
- [ ] **SubTask 5.6**: 修改 `ExtismPluginManager::load_plugin()` 在加载时自动注册所有 host functions
- [ ] **SubTask 5.7**: 更新 `PluginSandbox` 的资源计量为真实值
  - 每次 host function 调用后记录 `cpu_time_used_ms`

**验证**: `cargo build` + `cargo test` 通过

---

## Task 6: 改造 DriverRegistry 支持运行时注册

- [ ] **SubTask 6.1**: 在 `src-tauri/src/core/driver/registry/mod.rs` 中新增 `runtime` 字段
  - `runtime: RwLock<HashMap<String, Arc<dyn DriverFactory>>>`
  - `get()` 方法改为：先查 runtime，再查 builtin
- [ ] **SubTask 6.2**: 实现 `DriverRegistry::register_wasm_driver()`
  - 参数：`driver_id`、`plugin_id`、`wasm_bytes`、`descriptor: DriverDescriptor`
  - 创建 `WasmDriverFactory` → 写入 `runtime`
  - 返回 `Result<()>`
- [ ] **SubTask 6.3**: 实现 `WasmDriverFactory`（实现 `DriverFactory` trait）
  - `create_pool()` 创建 `WasmDbPool`（包装 Extism Plugin）
  - `descriptor()` 返回驱动描述符
- [ ] **SubTask 6.4**: 实现 `WasmDbPool`（简单实现 `DbPool` trait）
  - `acquire()` → 调用 Extism `call_plugin("connect", config_json)` → 返回 `conn_id`
  - `close()` → 调用 Extism `call_plugin("disconnect", conn_id)`
  - `is_closed()` / `status()` → 返回占位值

**验证**: `cargo build` 通过

---

## Task 7: 集成测试与验证

- [ ] **SubTask 7.1**: 编写 Manifest 解析器单元测试
  - 有效 manifest 解析成功
  - 缺少 `plugin.id` → 错误
  - 版本不兼容 → 错误
  - 纯前端插件（无 wasm）→ 正确
  - 纯后端插件（无 frontend）→ 正确
- [ ] **SubTask 7.2**: 编写 8 个内置 manifest 的解析测试（遍历文件验证）
- [ ] **SubTask 7.3**: 编写 Wasm Host Functions 单元测试
  - `host_db_query` 权限校验测试
  - `host_duckdb_query` 基本查询测试
- [ ] **SubTask 7.4**: 编写 Loader 单元测试
  - 优先级覆盖测试（project > user > builtin）
  - 循环依赖检测测试
  - 缺失依赖检测测试
- [ ] **SubTask 7.5**: 全链路集成测试
  - 应用启动 → 所有内置插件正常激活
  - 前端 `PluginContext.storage` 读写正常
  - `pnpm run lint` + `cargo clippy -- -D warnings` 均通过

**验证**: 所有测试通过，lint + clippy 零错误

---

# Task Dependencies

```
Task 1 (Manifest 数据模型) ──┬── Task 2 (迁移 8 个内置 manifest)
                             │
                             ├── Task 3 (PluginHost Loader)
                             │
                             ├── Task 4 (重构 PluginContext)
                             │
                             ├── Task 5 (Wasm Host Functions)
                             │
                             └── Task 6 (DriverRegistry 改造)
                                          │
Task 7 (集成测试) ←── all above tasks
```

- Task 2 依赖 Task 1（需要 manifest 格式定义完成后才能迁移）
- Task 3 依赖 Task 1（Loader 需要 manifest 类型）
- Task 4 依赖 Task 1（PluginContext 需要 manifest 类型）
- Task 5 依赖 Task 1（Host Functions 需要 manifest 权限字段）
- Task 6 依赖 Task 5（DriverRegistry 需要 WasmDriverFactory，依赖 Wasm 基础设施）
- Task 2/3/4/5 之间可以并行开发
- Task 7 依赖所有前序任务
