# 插件系统架构（M9）

状态：**三期（Phase 3）设计文档**。本文档只记录现状与设计，**不实施**；实现排在三期。
路径与进程约束见 `../runtime/data-paths.md` §8（同样归三期）；**全局与项目引用**见本文 §7（2026-09-18 补）。

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

## 7. 全局与项目：三层资源模型与引用设计（2026-09-18 补）

插件与**引擎扩展**（DuckDB 扩展）遵循同一套**三层资源模型**——与设置的三分法
（应用级 / 项目级 / 会话态，见 `settings/settings-architecture.md`）同一个心智：

| 层 | 语义 | 载体 | 谁写 |
| --- | --- | --- | --- |
| **应用级：安装** | 这台机器上有哪些插件 / 引擎扩展 | 本体在 `<RDS_HOME>/{plugins,extensions}/`；注册与状态在 `global.sqlite` | 用户显式动作（安装 / 卸载 / 装扩展） |
| **项目级：引用** | 这个项目用哪些插件 / 扩展 | 项目库 `project.sqlite` | 项目设置（启用 / 禁用 / 配置） |
| **会话态：激活** | 本次运行真正加载了哪些 | 进程内（`PluginManager` / 引擎侧） | 运行时，随打开项目与按需加载 |

三条规则：

1. **引用 ≠ 安装**：项目只记“我要用 X”，**不复制本体**——把项目拷给同事不该带上几十 MB 的二进制。
2. **软引用、不阻断**：引用了但本机没装 → 该插件贡献的能力（驱动 / 面板 / 命令 / 设置 /
   引擎扩展）**如实不可用**（带“项目引用了 X，未安装”的原因与安装入口），**项目照常打开**
   ——与“状态如实、不静默”同一口径（对照：设置模块的“登记≠上页”）。
3. **引擎扩展也走同一模型**：DuckDB 扩展（`mysql` / `mssql` / `oracle_scanner` / `httpfs`…）
   是**引擎扩展**而不是应用插件，但同样“全局装、项目引用”：项目 A 声明要 `mssql`，
   项目 B 什么都不用；首次用到时**显式安装**（不静默联网，见 `duckdb/manager.rs` 的
   `autoinstall_known_extensions = false`），离线环境把 `<RDS_HOME>/extensions/` 预置好即可。

### 7.1 目录分工（应用插件 vs 引擎扩展）

| 目录 | 装什么 | 谁按版本分子目录 | 备注 |
| --- | --- | --- | --- |
| `<RDS_HOME>/plugins/<id>/` | 应用插件本体（wasm / sidecar + manifest） | 我们（按 id） | 见 §6 |
| `<RDS_HOME>/plugin-data/<id>/` | 插件私有数据（沙箱内唯一可写区） | 我们 | §6 |
| `<RDS_HOME>/plugin-cache/<id>/` | wasmtime 编译缓存 / sidecar 日志与临时文件 | 我们 | §6 |
| `<RDS_HOME>/tmp/sidecar/<id>/` | sidecar 工作目录（`current_dir`） | 我们 | §6 |
| `<RDS_HOME>/extensions/` | **DuckDB 引擎扩展**（`*.duckdb_extension`） | **DuckDB 自己**（落 `<内核版本>/`，如 `v1.5.5/`） | 已接线：所有长期存活的 DuckDB 连接统一设 `extension_directory`（`duckdb/manager.rs::configure_connection`） |

**为什么两者不混一个目录**：DuckDB 要求 `extension_directory` 指向**它自己的**扩展目录
（内部按内核版本分子目录），而应用插件是另一个生命周期（manifest / 权限 / 沙箱）。
混在一起会让“卸载插件”与“清扩展缓存”互相牽连，也违反“一个目录一件事”。
**心智统一、物理分开**。

### 7.2 表与命令（建议形状）

`global.sqlite`：

- `plugin_store`：插件注册（现有）；
- **不建引擎扩展的镜像表**（2026-09-18 修正）：扩展“装没装 / 加载没加载”的**真值就在
  DuckDB 自己身上**（`duckdb_extensions()` 的 `installed` / `loaded` 列）——再维护一张表
  只会产生“表说已装、实际没有”的偏差（单一权威）。我们只保留两样东西：
  ① **动作与失败原因**（进程内；跨重启重试本来就合理——环境可能变了）；
  ② **官方 / 社区清单**（常量，用于界面标注与安装入口）。
  “项目引用了哪些扩展”仍归下面 `project_resources`（三期）。

`project.sqlite`（新增，参考 V1 的 `project_used_plugins` + `project_plugin_config`）：

```sql
CREATE TABLE project_resources (
    kind        TEXT NOT NULL,      -- 'plugin' | 'engine_extension'
    id          TEXT NOT NULL,      -- 插件 id 或扩展名（mysql / mssql…）
    version     TEXT,               -- 可选：插件为语义化范围；扩展为空 = 跟内核
    enabled     INTEGER NOT NULL DEFAULT 1,
    config_json TEXT,               -- 项目级配置（插件自己的设置项）
    added_at    TEXT NOT NULL,
    PRIMARY KEY (kind, id)
);
```

**一张表 + `kind` 列**（而不是 V1 的两张业务表 + 一张配置表）：插件与引擎扩展在“引用”
这一层的语义完全一致（启用 / 版本 / 配置 / 未装降级），分开建表只会让 UI 与校验写两遍。
V1 的六条命令（`project_plugin_enable/disable/remove/list/set_config/get_configs`）
对应到 v2 就是 `ProjectResourceService` 的六个方法，**加 `kind` 参数**。

**归属建议（不动依赖方向）**：表与读写 API 归 `crates/project`（项目库的主人，M1）；
`crates/plugin` 只提供“本机装了什么”的查询（M9）；**对账（引用 vs 已装）由宿主层做**
（workbench），这样 M1 不需要依赖 M9。

### 7.3 生命周期（项目视角）

```
打开项目
  → 读 project_resources（内存快照，渲染路径不 I/O）
  → 对账：本机已装？（global.sqlite 的 plugin_store / engine_extensions）
      · 已装 + enabled  → 纳入激活集（会话态）
      · 已装 + disabled → 不激活（如实显示“项目已禁用”）
      · 未装            → 记为“缺失引用”，**不阻断**；在相关入口给原因 + 安装动作
  → 用到时按需激活（驱动 / 面板 / 引擎扩展）
关闭项目 → 失活（wasm 实例 / sidecar 进程 / 扩展连接回收）
```

### 7.4 与 V1 的对照

| 维度 | V1 | V2（本设计） |
| --- | --- | --- |
| 全局注册 | `plugin_store`（global.sqlite） | 同（已有） |
| 项目引用 | `project_used_plugins` + `project_plugin_config` | `project_resources`（一张表 + `kind`，**把引擎扩展也纳入**） |
| 项目命令 | `project_plugin_*` 六条 | `ProjectResourceService` 六个方法 + `kind` 参数 |
| 插件访问 DuckDB | `PluginPermissionLevel{ReadOnly,ReadWrite,Admin}` + `PluginConnection` 沙箱（只有 Admin 能联邦） | **三期再定**；若做，沿用“只读 / 受限镜像”的思路（V1 三档里我们只需要 ReadOnly 与“本地临时对象”两级） |
| 路径 | 无统一根（`./plugins` + `~/.rdatastation`） | `paths::*` 单根 + 离线预置（§6 与本节 7.1） |
| 引擎扩展 | `core/duckdb/plugin.rs` 管的是“插件访问 DuckDB 的权限”，扩展本身无登记 | 状态读 DuckDB 真值（`duckdb_extensions()`）+ 内存失败态；项目引用随 `project_resources`（**不建镜像表**） |

## 8. 已知缺口与待定问题

| 项 | 现状 | 处理建议 |
| --- | --- | --- |
| `storage` 模块 | `lib.rs` 的模块文档列了「核心：… storage」，但 `pub mod` 列表里**没有** | 三期补：插件私有数据访问层（对应 §6 的 `plugin-data/<id>`） |
| 插件发现目录 | 依赖 CWD + 写 C 盘 | 见 §6 |
| 前端扩展（wasm 侧栏） | 清单已有 `capabilities.frontend` / `contributes.panel`，宿主侧消费未接线 | 三期做：面板注册表 + 沙箱内 UI 渲染契约 |
| 版本与依赖 | `dependency.rs` 从清单解析，未定版本范围与冲突策略 | 三期做：语义化版本 + 冲突报错口径 |
| 端口/进程回收 | stdout 自报端口，无段位约束 | 见 §6 |
| 安全边界 | 权限模型有结构与授权状态，但缺少"插件能碰哪些宿主 API"的完整清单 | 三期做：宿主函数 / JSON-RPC 方法的权限映射表 |
| **项目引用** | **V2 无**：只有全局 `plugin_store`（V1 有 `project_used_plugins` + `project_plugin_config` + 六条命令） | 三期做：`project_resources` + `ProjectResourceService`（见 §7） |
| **引擎扩展与项目的关系** | 扩展状态只有 DuckDB 的真值（`duckdb_extensions()`）与内存失败态；项目无法声明“需要哪些扩展” | 三期做：`project_resources` 里的 `kind = 'engine_extension'`（**不建镜像表**，见 §7.2） |

## 9. 三期计划骨架（不实施，仅排布）

| 编号 | 内容 | 前置 |
| --- | --- | --- |
| P3-a | 路径与进程约束落地（§6，依赖运行时的 `crates/paths` 已就位） | 运行时数据路径改造完成（一/二期） |
| P3-b | `storage` 模块 + 插件私有数据目录 | P3-a |
| P3-c | 权限审批流程 UI（请求/授权/撤销）+ 宿主 API 权限映射表 | P3-a |
| P3-d | 前端扩展：面板注册表 + wasm 侧栏渲染契约 | P3-b |
| P3-e | 热加载与健康检查收敛（wasm 热重载 / sidecar 崩溃重启与端口回收） | P3-a |
| P3-f | **项目资源引用**：`project_resources` + `ProjectResourceService` + 未装降级与安装入口（§7） | P3-a、P3-b |
| P3-g | **引擎扩展的项目引用**：`project_resources(kind='engine_extension')` + 显式安装入口 + 离线预置（**不建镜像表**：状态读 `duckdb_extensions()`，§7.2） | P3-a |

## 10. 实现位置映射表

| 设计决策 | 实现位置 |
| --- | --- |
| 清单与扩展点 | `crates/plugin/src/manifest.rs` |
| 权限模型 | `crates/plugin/src/permission.rs` |
| 生命周期 | `crates/plugin/src/manager.rs`、`plugin_service.rs`、`installer.rs`、`loader.rs` |
| WASM 适配 | `crates/plugin/src/wasm/*`（Extism） |
| Sidecar 适配 | `crates/plugin/src/sidecar/*`（JSON-RPC + 健康检查 + 热重载） |
| 驱动发现 | `crates/engine/src/driver/loader.rs` |
| 插件注册表 | `crates/engine/src/persistence/plugin_store.rs` |
| 引擎扩展目录（✅ 已接线） | `crates/engine/src/duckdb/manager.rs::configure_connection`（`extension_directory` = `paths::extensions_dir()`，并关掉静默联网） |
| 项目资源引用（§7） | 待落地：表与 API 归 `crates/project`，对账在宿主层（workbench） |
| 插件路径与沙箱 | `../runtime/data-paths.md` §8（三期） |
