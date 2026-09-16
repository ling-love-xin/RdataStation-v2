# 插件系统架构（M9）

状态：**三期（Phase 3）设计文档**。本文档只记录现状与设计，**不实施**；实现排在三期。
路径与进程约束见 `../runtime/data-paths.md` §8（同样归三期）。

## 1. 定位

插件系统为 RdataStation 提供**不重新发版即可扩展**的能力，扩展点由清单声明（四类）：

| 扩展点 | 清单字段 | 说明 |
| --- | --- | --- |
| 驱动 | `contributes.driver` | 外部数据库驱动（wasm 或 sidecar 承载） |
| 面板 | `contributes.panel` | 侧栏 / 面板 UI（含 **wasm 侧栏**：`capabilities.frontend`） |
| 命令 | `contributes.command` | 注册到命令面板与快捷键系统 |
| 设置 | `contributes.setting` | 插件自己的设置项 |

## 2. 模块与代码位置

`crates/plugin`（7488 行，依赖方向 `plugin → engine → shared`，符合三层约束）

| 子系统 | 文件 | 职责 |
| --- | --- | --- |
| 清单 | `src/manifest.rs`（575） | `PluginManifest` / `PluginMeta` / `PluginEngines` / `PluginCapabilities`（`CapabilitiesFrontend` / `CapabilitiesWasm`）/ `PluginPermissions` / `PluginContributes`（`ContributesCommand` / `Panel` / `Driver` / `Setting`） |
| 权限 | `src/permission.rs`（373） | `PermissionType` / `Permission` / `PermissionGrant` / `GrantStatus` / `PermissionManager`（全局单例 `get_permission_manager()`） |
| 生命周期 | `src/manager.rs`（337） | `PluginManager`：`add_plugin_dir` / `scan_plugins` / `load_plugin` / `activate_` / `deactivate_` / `unload_plugin` / `list_plugins` |
| 依赖解析 | `src/dependency.rs` | 从 `manifest_json` 解析插件依赖 |
| 安装 | `src/installer.rs` | 安装与落盘 |
| 热加载 | `src/loader.rs` | 热加载入口 |
| 事件 | `src/events.rs` | 插件事件总线 |
| 服务 | `src/plugin_service.rs`（419） | `PluginService::new(global_db)`：`InstallPluginInput` / `PluginStatus` / `PluginWithStatus` |
| 桥接 | `src/plugin_bridge.rs` | 与宿主 / 编辑器的桥接 |
| WASM 适配 | `src/wasm/{plugin_manager,extism,api,host_functions}.rs` | Extism 运行时（wasmtime 底座）+ 宿主函数注入 |
| Sidecar 适配 | `src/sidecar/{manager,driver,client,health_checker,hot_reload_manager}.rs` | 独立进程 + **JSON-RPC**；驱动适配 |
| 驱动发现 | `crates/engine/src/driver/loader.rs` | `WasmDriverDiscovery::plugin_dirs`（默认目录见 §6） |
| 注册表 | `crates/engine/src/persistence/plugin_store.rs` | 插件记录落 `global.sqlite`（含 `manifest_json`） |

## 3. 两种运行形态

| 维度 | WASM（Extism） | Sidecar（独立进程） |
| --- | --- | --- |
| 隔离 | 进程内沙箱（wasmtime），只能通过宿主函数触达外部 | 进程边界隔离，可用任意语言（现为 Go） |
| 通信 | 宿主函数 + Extism ABI | JSON-RPC（`client.rs`），**端口由子进程 stdout 自报** |
| 适用 | 分析类 / 轻量驱动 / 工具 | 需原生依赖、长驻连接、无法编译为 wasm 的驱动 |
| 生命周期 | 随插件激活实例化 / 卸载 | 需健康检查（`health_checker.rs`）、热重载（`hot_reload_manager.rs`）、崩溃清理 |
| 路径风险 | wasmtime 编译缓存默认不在应用目录（待显式配置） | 子进程 `current_dir` / 日志 / 临时文件未约束；**真实占用本地端口** |

## 4. 生命周期

```
install（installer / PluginService + global.sqlite 注册）
  → scan_plugins（PluginManager 扫插件目录）
  → load_plugin（读清单、校验、解析依赖 dependency.rs）
  → activate_plugin（启动运行形态：wasm 实例化 / sidecar 起进程 + 健康检查）
  → 调用（contributes 四类扩展点被宿主消费）
  → deactivate_plugin / unload_plugin（停实例 / 停进程，回收端口与临时文件）
卸载：删插件目录；插件私有数据是否保留由清单声明（默认保留）
```

## 5. 权限模型

- 清单声明 → `PermissionManager::get_required_permissions(manifest)` → 安装/启用时向用户请求（`PermissionGrant` + `GrantStatus`）
- 权限按类别组织（`get_permissions_by_category`），全局单例便于各子系统校验
- **路径权限是重点**：插件可申请的路径能力必须与"只能写自己目录"一致（见 §6），否则沙箱形同虚设

## 6. 路径与进程约束（归三期，设计已定）

完整设计在 `../runtime/data-paths.md` §8，此处只列结论：

```
<RDS_HOME>/plugins/<plugin-id>/       插件本体（wasm / sidecar 二进制 + manifest.json）
<RDS_HOME>/plugin-data/<plugin-id>/   插件私有数据（沙箱内唯一可写区，默认卸载后保留）
<RDS_HOME>/plugin-cache/<plugin-id>/  wasmtime 编译缓存、sidecar 日志与临时文件
<RDS_HOME>/tmp/sidecar/<plugin-id>/   sidecar 进程 current_dir
```

现状需修的三处（实测）：

1. 插件发现目录 = `./plugins`（**相对当前工作目录**）+ `~/.rdatastation/plugins`（写 C 盘）
   → 改为 `paths::plugins_dir()`（`engine/src/driver/loader.rs:135`）。
2. WASM 侧未配置 Extism/wasmtime 缓存目录 → 显式指向 `plugin-cache/<id>`。
3. Sidecar：未设 `current_dir` / 日志 / 临时目录；端口无约束 → 建议保留段 **41000–41999** + 冲突重试 + 退出回收。

## 7. 已知缺口与待定问题

| 项 | 现状 | 处理建议 |
| --- | --- | --- |
| `storage` 模块 | `lib.rs` 的模块文档列了「核心：… storage」，但 `pub mod` 列表里**没有** | 三期补：插件私有数据访问层（对应 §6 的 `plugin-data/<id>`） |
| 插件发现目录 | 依赖 CWD + 写 C 盘 | 见 §6 |
| 前端扩展（wasm 侧栏） | 清单已有 `capabilities.frontend` / `contributes.panel`，宿主侧消费未接线 | 三期做：面板注册表 + 沙箱内 UI 渲染契约 |
| 版本与依赖 | `dependency.rs` 从清单解析，未定版本范围与冲突策略 | 三期做：语义化版本 + 冲突报错口径 |
| 端口/进程回收 | stdout 自报端口，无段位约束 | 见 §6 |
| 安全边界 | 权限模型有结构与授权状态，但缺少"插件能碰哪些宿主 API"的完整清单 | 三期做：宿主函数 / JSON-RPC 方法的权限映射表 |

## 8. 三期计划骨架（不实施，仅排布）

| 编号 | 内容 | 前置 |
| --- | --- | --- |
| P3-a | 路径与进程约束落地（§6，依赖运行时的 `crates/paths` 已就位） | 运行时数据路径改造完成（一/二期） |
| P3-b | `storage` 模块 + 插件私有数据目录 | P3-a |
| P3-c | 权限审批流程 UI（请求/授权/撤销）+ 宿主 API 权限映射表 | P3-a |
| P3-d | 前端扩展：面板注册表 + wasm 侧栏渲染契约 | P3-b |
| P3-e | 热加载与健康检查收敛（wasm 热重载 / sidecar 崩溃重启与端口回收） | P3-a |

## 9. 实现位置映射表

| 设计决策 | 实现位置 |
| --- | --- |
| 清单与扩展点 | `crates/plugin/src/manifest.rs` |
| 权限模型 | `crates/plugin/src/permission.rs` |
| 生命周期 | `crates/plugin/src/manager.rs`、`plugin_service.rs`、`installer.rs`、`loader.rs` |
| WASM 适配 | `crates/plugin/src/wasm/*`（Extism） |
| Sidecar 适配 | `crates/plugin/src/sidecar/*`（JSON-RPC + 健康检查 + 热重载） |
| 驱动发现 | `crates/engine/src/driver/loader.rs` |
| 插件注册表 | `crates/engine/src/persistence/plugin_store.rs` |
| 插件路径与沙箱 | `../runtime/data-paths.md` §8（三期） |
