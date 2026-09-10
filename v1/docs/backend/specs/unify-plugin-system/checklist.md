# Checklist: 统一插件系统

> 对应 Spec: `.trae/specs/unify-plugin-system/spec.md`
> 对应 Tasks: `.trae/specs/unify-plugin-system/tasks.md`

---

## 架构红线

- [ ] `core/plugin/` 模块不依赖 `tauri` 框架 — 纯业务逻辑
- [ ] `ManifestParser` 不使用 `unwrap()` / `expect()`
- [ ] Wasm Host Functions 通过 `CoreError` 返回错误，不 panic
- [ ] Arrow IPC Stream 格式用于 Wasm ↔ Core 数据传输（遵守 IPC 生死线）
- [ ] `DriverRegistry` 运行时注册不破坏编译期注册能力

---

## Manifest 数据模型

- [ ] `PluginManifest` 结构体包含 `#[derive(Debug, Clone, Serialize, Deserialize)]`
- [ ] `plugin.id` 使用反向域名格式（如 `rdatastation.workbench`）
- [ ] `plugin.version` 使用 semver 格式
- [ ] `engines.rdatastation` 支持 semver 范围匹配（至少 `^x.y.z`）
- [ ] `capabilities.frontend` 和 `capabilities.wasm` 至少存在一个
- [ ] `contributes.panels` 中 `location` 仅允许 `left` | `right` | `bottom` | `center`
- [ ] `permissions` 为可选段，默认无权限
- [ ] TypeScript 侧 `PluginManifest` 类型与 Rust 侧一一对应
- [ ] 解析失败时错误信息包含文件路径和字段名

---

## 8 个内置扩展 Manifest

- [ ] 每个内置扩展都有对应的 `rdata-plugin.toml` 文件
- [ ] 每个 manifest 通过 `ManifestParser::parse()` 解析无错误
- [ ] 每个 `package.json` 的内容（commands/menus/views）完整迁移到 `[contributes]` 段
- [ ] 前端 `entry` 路径指向正确的 `extension.ts`
- [ ] 内置扩展的 `id` 使用 `rdatastation.*` 命名空间

---

## PluginHost Loader

- [ ] `PluginLoader.discoverPlugins()` 返回按优先级去重后的插件列表
- [ ] 扫描三个目录：`project/.rdata/plugins/` > `{data_dir}/plugins/` > `builtinExtensions`
- [ ] 同名 ID 优先级：project > user > builtin
- [ ] `validateManifest()` 返回结构化验证结果（`{ valid, errors }`），非抛异常
- [ ] `resolveDependencies()` 正确拓扑排序，检测循环依赖
- [ ] 缺失依赖插件标记为 Error，不阻塞其他插件
- [ ] 内置插件加载失败时应用仍正常启动

---

## PluginContext 重构

- [ ] `PluginContext` 包含 `pluginId: string` 字段
- [ ] `PluginContext` 包含 `manifest: PluginManifest` 字段
- [ ] `logging` 输出带 `[Plugin:{id}]` 前缀
- [ ] `storage` 使用命名空间隔离（不同插件无法读取对方数据）
- [ ] `events` 对接现有 `EventBus`
- [ ] `panels.register()` 行为与原 `window.registerViewProvider()` 一致
- [ ] `database.query()` 通过 `tauri.invoke('plugin_db_query', ...)` 调用后端
- [ ] 旧 `window`、`workspace`、`configuration`、`sqlEditor`、`utils` 字段已移除

---

## Wasm Host Functions

- [ ] `host_db_query` 校验调用方权限后执行查询
- [ ] `host_db_query` 返回 Arrow IPC Stream 字节
- [ ] `host_db_metadata` 返回 JSON 格式元数据
- [ ] `host_duckdb_query` 通过 DuckDB Manager 执行分析 SQL
- [ ] `host_duckdb_load` 反序列化 Arrow IPC Stream 到 DuckDB 临时表
- [ ] 未授权调用返回 `PermissionError`
- [ ] 每次调用后资源计量更新
- [ ] Host Functions 注册在 `ExtismPluginManager::load_plugin()` 时自动完成

---

## DriverRegistry 运行时注册

- [ ] `DriverRegistry.runtime` 字段存在且用 `RwLock` 保护
- [ ] `get(driver_id)` 优先返回 runtime 条目，fallback 到 builtin
- [ ] `register_wasm_driver()` 成功后驱动出现在 `get_all_drivers()` 列表中
- [ ] `WasmDriverFactory` 实现 `DriverFactory` trait
- [ ] `WasmDbPool` 实现 `DbPool` trait

---

## 代码质量

- [ ] Rust: `cargo build` 零错误
- [ ] Rust: `cargo clippy -- -D warnings` 零警告
- [ ] Rust: `cargo fmt` 通过
- [ ] TypeScript: `pnpm run lint` 零错误
- [ ] TypeScript: `pnpm run typecheck` 零错误
- [ ] 无 `any` 类型
- [ ] 无 `unwrap()` / `expect()`
- [ ] 所有公开 API 有文档注释

---

## 测试覆盖

- [ ] Manifest 解析器：有效/无效/版本不兼容/边界用例
- [ ] 8 个内置 manifest 解析测试
- [ ] Loader 优先级覆盖测试
- [ ] Loader 依赖检测测试
- [ ] Host Functions 权限校验测试
- [ ] ScopedStorage 命名空间隔离测试
- [ ] DriverRegistry 运行时覆盖编译期测试
