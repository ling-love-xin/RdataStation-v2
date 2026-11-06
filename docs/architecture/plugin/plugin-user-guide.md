# 插件系统（M9）· 使用手册

> **读者**：① 使用者（装 / 启 / 排障）② 插件作者（从零写一个插件）。
> 设计理念见 `plugin-prototype-design.md`；任务与排期见 `plugin-dev-plan.md`；模块入口见 `README.md`。
>
> ⚠️ **时效声明（2026-11-07）**：现在**已能照着操作**的是驱动轨与插件面板的最小闭环——
> 面板可列出已装插件、启用/停用、卸载，以及从 `.zip` / `.tar.gz` 或本地目录安装；
> 安装期会校验清单、声明入口、`checksums.json` 与开发残留。驱动作者按 §2.8 可自检。
>
> **尚未落地**：扩展宿主（QuickJS）与 `rds` API（§2.3 是原型口径）、插件详情页 / 搜索 /
> 市场 / 项目引用对账、命令面板里的插件命令、`signature.json` 签名与信任链、权限逐项授予。
> §1.2 的完整面板草图与 §3 里标「待 P4/P5」的条目是**目标口径**，现在不能原样走通。

---

## 第一部分 · 使用者

### 1.1 入口

| 入口 | 位置 | 行为 |
| --- | --- | --- |
| 打开插件面板 | 左活动栏第 4 个图标（插件） | 左 Dock 展开为「插件」面板 |
| 安装插件 | 面板头「安装插件」 | 选 `.zip` / `.tar.gz`；后台解包安装，完成后刷新列表 |
| 从目录导入 | 面板头「从目录」 | 选含 `plugin.toml` 的目录（开发目录 / 已解压的包） |
| 启用 / 停用 | 条目第一行行尾 | 写 global.db `is_enabled` 并即时停/启进程，然后刷新列表 |
| 卸载 | 条目第二行行尾 | 确认框 → 依赖检查 → 停进程 → 删 `plugins/<id>/`（保留 `plugin-data/<id>`） |
| 刷新 | 面板头 ⟳ | 重新拉取已装插件清单（同时看 `pending_reloads`） |
| 重载 | 变更提示行「重载 N」 | 仅在有插件文件变更时出现；停全部插件 → 重新扫描加载并激活（DB 停用的除外） |
| 命令面板 / Quick Open / 插件详情 | — | **未落地**（P4）；插件贡献的命令目前不会出现在任何面板 |

> 安装入口**没有** `.rdsx` 格式。识别的只有 `.zip`、`.tar.gz` 与目录。
> 包根按「`plugin.toml` 在根层，或包内只套了一层插件目录」两种形态识别
> （后者正是 `tools/zip-dir.ps1` 的产物）；套了多层、或一个包里放了多个插件目录，会报错拒绝。

### 1.2 界面导览（当前实现）

```
┌ 左侧 Dock（240px）──────────────────────────────┐
│ 插件              [安装插件] [从目录] [⟳]        │  ← 面板头
├────────────────────────────────────────────────┤
│ Oracle JDBC Driver  v0.1.0  已激活       [停用]  │  ← 第一行：名称/版本/状态/启停
│   rds.driver.oracle                     [卸载]  │  ← 第二行：id/卸载
│ Hello 驱动  v0.1.0  已停用               [启用]  │
│   com.example.hello-driver              [卸载]  │
├────────────────────────────────────────────────┤
│ 已安装 Hello 驱动 v0.1.0（com.example.hello-driver）│  ← 回执行（成功绿）
└────────────────────────────────────────────────┘
```

失败原因单独显示为红色错误行（与回执行互斥），不会混进绿色回执。

状态只有四种：`已激活`（success）/ `已加载`（info）/ `已停用`（muted）/ `出错`（danger）。
安装进行中会在回执行位置显示 `正在安装 …`（info），期间两个安装按钮置灰。

> 下方完整面板草图（搜索框、三档筛选、插件详情四区、状态栏宿主状态）是 **P4 目标**，
> 当前没有这些控件。

```
┌ 左侧 Dock（240px）──────────────────────────────┐
│ 插件                    [⟳] [⋯]                │  ← 目标口径（未实现）
├────────────────────────────────────────────────┤
│ [🔍 搜索已安装 / 市场…]                          │
│ [ 已安装 3 ] [ 待更新 1 ] [ 市场 ]                │  ← 三档
├────────────────────────────────────────────────┤
│ ▾ 已安装                                        │
│   [SQL] SQL 方言增强  0.4.1   [可更新]  [禁用]   │  ← 两行式条目
│   [TB]  表大小看板    1.0.0   [已禁用]  [启用]   │
│   ⚠ 本项目引用了它贡献的面板，但它现在是禁用状态    │  ← 对账行（不静默）
│        [重新启用] [从项目移除引用]                │
│   [JD]  Oracle JDBC 桥 1.2.0 [native 驱动] [禁用]│
├────────────────────────────────────────────────┤
│ [从磁盘安装…]                         [目录]     │
└────────────────────────────────────────────────┘

中央「插件详情」页分四区（未实现）：
  基本信息 → 贡献点 → 权限（扩展轨 / 驱动轨两张表）→ 项目引用 → 运行状态
```

### 1.3 典型流程

**装一个插件（从磁盘）**（已实现）

1. 点「安装插件」选 `.zip` / `.tar.gz`，或点「从目录」选一个含 `plugin.toml` 的目录。
2. 安装前来源检查：路径存在、格式可识别；**选中 `plugins/<id>` 自身会被拒绝**
   （安装是"先删目标再复制"，源=目标等于先把源删掉）。
3. 解包到临时目录（zip 拒绝路径穿越），定位包根（根层清单，或唯一一层插件目录），
   随后 `validate_plugin`：
   `plugin.toml` 存在 → 清单解析与 `validate()` → 声明入口真实存在 →
   `checksums.json` 校验（**缺失放行；带该文件的包在每次加载时还会再校验一次**）→
   拒绝把入口声明在开发残留目录里。
4. 清单有 `[[dependencies]]` 时先对账：缺必装依赖直接拒绝并点名 `id@version`，
   此时旧插件仍在运行、目录没动。
5. 复制到暂存目录（`tmp/rds-plugin-stage-*`）再整体换入 `plugins/<id>/`（目录名 = `plugin.id`；
   `target/` / `.git` / `node_modules` 等开发残留不复制，`src/` 属于包内容照常装）。
   **覆盖安装（重装 / 升级）先停运行中的插件**：删旧目录失败会报可读原因，不会把新包叠在
   旧文件上；换入 / 登记失败会把旧插件恢复运行。
6. 登记 global.db（一个 code 只留一行，沿用原来的停用状态与首次安装时间；清单依赖同事务
   写入 `plugin_dependencies`）→ 尝试加载 →
   已启用则立即激活（sidecar 驱动当场注册）→ 刷新列表。回执按真实结果分四种：
   「已安装并启用 …」「已安装 …（保持停用）」「已安装 …，但加载或激活失败，详见日志」
   「已安装 …」（未拿到运行时状态时的兜底）。安装成功但加载 / 激活失败时目录与登记保留，
   原因进 `tracing::warn` 日志。

> 未实现：`signature.json`（Ed25519 / 发布者信任链）、安装前"逐项列出权限并确认"。
> 当前只有 `checksums.json` 的 SHA-256 完整性校验（覆盖 `plugin.toml` 与声明入口，
> 缺文件或哈希不符会拒绝），它不是签名，也不能证明发布者身份。

**启用 / 停用**（已实现持久化）

- 停用：停掉该插件的 sidecar 进程，`is_enabled = false` 写入 global.db；启用反向执行并立即激活。
- 从宿主 UI 撤下贡献点（视图 / 菜单 / 命令）要等 P4 注册表；现在没有贡献点可撤。

**授权与撤销**（未实现）

- 逐项授予/撤销权限、安装页权限确认、`-32006 capability_denied` 的可读回执 UI 都属于 P4。
- 现行只有 `[permissions]` 的**声明与展示口径**（§2.4）和 `[permissions.host]` 的运行时门控。

**卸载**（已实现）

- 依赖检查：被其他插件引用时拒绝，并列出引用者；确认框说明"删安装目录、保留用户数据"。
- 项目引用对账（「项目引用 · 未安装」分组）未实现（P4）。

### 1.4 FAQ

> 以下是设计口径问答；涉及命令面板、详情页、webview、扩展宿主的部分属 P4，
> 当前**还不能操作**。现在可验证的排障口径见 §2.6。

**Q：命令面板里能看到插件命令，但点了没反应？**
A：命令是**启动时静态登记**的（零成本），代码是**首次激活时懒加载**的。第一次点击会先加载模块并跑 `activate()`，可能有一两百毫秒延迟；失败会弹错误而不是无声。看应用日志（`target` 前缀是插件 id）；状态栏的插件日志入口属 P4。

**Q：为什么某个面板显示「未安装 / 未启用」而不是空白？**
A：这是**故意的**（项目引用对账）。空白的含义是"这个功能本来就没有"，而占位的含义是"有，但当前不可用"。两者不能混。

**Q：为什么插件功能和产品其余部分配色一致？它不是第三方做的吗？**
A：插件的面板数据由宿主用自己的控件渲染；走 webview 的图表也必须从宿主取主题角色值。所以配色/尺寸天然一致，插件也无法做出"另一个产品"的观感。

**Q：webview 面板能不能一边看图表一边弹宿主的对话框？**
A：不能。webview 是原生子窗口，层级不受 GPUI 控制，会盖住宿主的 dialog/overlay。因此 webview 只给两种承载：**全屏面板**或**独立窗口** —— 后者反而是最舒服的用法（可以拖到第二块屏）。

**Q：插件崩了会怎样？**
A：三种情况分开算：① 某次命令抛异常 → 只有那次调用失败（扩展轨，P4）；② 扩展宿主进程崩溃 → 所有扩展失活，宿主 UI 照常，可一键重启（P4）；③ 驱动进程崩溃 → 只有那个连接失效，如实报错（不静默重连），可手动重启该驱动（**已实现**）。

---

## 第二部分 · 插件作者

### 2.1 最小插件（按现行代码）

当前**能端到端跑通**的作者路径是驱动轨（sidecar / script，协议见 §2.8）。最小包：

```
my-plugin/
├── plugin.toml          # 清单（唯一契约，文件名固定）
├── bin/agent.py         # 进程入口（相对路径由清单指向；新包放 bin/）
└── media/icon.svg       # 可选展示资产，不参与执行
```

```toml
api     = "database"                       # 顶层键：必须在任何 [表] 头之前
methods = ["driver.describe", "query.execute", "query.fetch", "query.cancel", "meta.catalogs"]

[plugin]
id          = "com.example.hello-driver"   # 目录名必须等于它
name        = "Hello 驱动"
version     = "0.1.0"
publisher   = "example"
description = "最小驱动插件"
license     = "MIT"

[plugin.engines]
rdatastation = ">=0.1"                     # 必填；不支持 "*"

[backend]                                  # 怎么跑：sidecar / script / wasm
kind          = "script"
runtime       = "python"                   # java / python：宿主三级查找解释器
executable    = "bin/agent.py"             # 相对插件目录、POSIX 分隔
max_instances = 1

[permissions]                              # 声明与展示口径，见 §2.4
sidecar     = ["spawn.child_process", "net.connect", "fs.read_plugin_data", "credentials.database"]
driver      = ["driver.query", "driver.metadata"]
credentials = "database"

[capabilities.driver]
concurrency = "serial"                     # serial / parallel

[[contributes.drivers]]
id           = "hello"
display_name = "Hello DB"
default_port = 9999
```

`bin/agent.py` 要实现 §2.8 的握手与 `driver.describe`；入口位置规则见
`plugin-layout.md` §3–§4。打包成 `.zip` / `.tar.gz`（`plugin.toml` 在根层，或只套一层
插件目录）或直接把目录交给面板都能安装。

> **原型字段不要照抄**：早期原型里的 `schema_version` / `[extension]` / `[main]` /
> `[capabilities]` 权限表在现行代码里**都不存在**；`contributes.views` / `insightRules` /
> `mockGenerators` / `exporters` 也还没有对应结构体——serde 忽略未知键，写了**不报错、
> 也不生效**。前端扩展（`[capabilities.frontend]`）的清单形态能解析，但扩展宿主与
> `rds` API 属 P4，目前没有可运行的前端插件。

### 2.2 清单字段速查（现行结构）

来源：`crates/plugin/src/manifest.rs`（`PluginManifest` 及其子结构）。

| 字段 | 必填 | 说明 |
| --- | --- | --- |
| `[plugin]` `id` / `name` / `version` | ✅ | `id` 反向域名式且全局唯一；安装目录名必须等于 `id` |
| `plugin.publisher` / `description` / `icon` / `homepage` / `license` | — | `icon` 是包内相对路径（当前无消费方，P4 插件图标再用） |
| `[plugin.engines]` `rdatastation` | ✅ | 版本闸，如 `">=0.1"`；不支持 `*` |
| `[backend]` `kind` | 二选一* | `sidecar`（原生进程）/ `script`（解释器+脚本）/ `wasm`（进程内沙箱） |
| `backend.executable` | 子进程形态必填 | 相对插件目录的入口，如 `bin/agent`；Windows 会自动补 `.exe` |
| `backend.interpreter` | script 且无 `runtime` 时必填 | 如 `python3`；有 `runtime` 时由宿主解析后替换 |
| `backend.runtime` | — | `java` / `python` / `rust`；声明后宿主按「用户指定 → 环境变量/PATH → `third_party/`」查找 |
| `backend.interpreter_args` | — | script 型专用（Java 的 `-jar` 位）；sidecar 写它会被拒 |
| `backend.driver_jars` | — | 仅 `runtime = "java"`；绝对路径（文件或目录），经 `RDS_DRIVER_JARS` 注入 Java 壳 |
| `backend.transport` | — | 只支持 `framed`；写 `jsonl` 在解析/启动期得到明确拒绝 |
| `backend.protocol` / `platforms` / `max_instances` | — | 协议标识（诊断用）/ 平台矩阵（空=不阻挡）/ 实例上限（默认 1） |
| `api` | — | 能力面命名空间（如 `database`）；小写字母开头，只含小写/数字/连字符 |
| `methods` | — | 声明的 RPC method；必须是协议已知 method 或 `x.` 私有前缀，否则安装期拒绝 |
| `[capabilities.frontend]` `entry` | 前端形态必填 | 前端模块相对路径；**运行未落地**（P4） |
| `[capabilities.wasm]` `entry` | wasm 形态必填 | wasm 模块相对路径；`kind = "wasm"` 必须同时有它 |
| `capabilities.wasm.max_memory_mb` / `max_cpu_time_ms` / `allowed_host_functions` | — | 沙箱限额与 host function 白名单 |
| `[capabilities.wasm.render_document]` | — | 渲染计算轨：`function` / `formats` / `output_format` |
| `[capabilities.driver]` `concurrency` | — | `serial`（默认）/ `parallel` |
| `[permissions]` `sidecar` / `driver` / `frontend` / `wasm` | — | 声明与展示口径（不门控，见 §2.4） |
| `permissions.credentials` | — | `"database"` = 明确声明会收到数据库密码 |
| `permissions.host` | — | 反向 Host API 点分 glob（如 `secrets.read.*`），**运行时默认拒绝** |
| `[[contributes.commands]]` | — | `id` / `title` / `category` / `icon` / `shortcut`（登记与展示未落地，P4） |
| `[[contributes.panels]]` | — | `id` / `title` / `location` / `icon` / `order`（面板注册未落地，P4） |
| `[[contributes.drivers]]` | — | `id` / `display_name` / `default_port` / `connection_schema` / `url_template` / `features` |
| `[[contributes.settings]]` | — | `key` / `type` / `default` / `label` / `description` |
| `contributes.menus` | — | 目前是不限结构的 JSON 值，宿主不消费 |
| `[[dependencies]]` | — | `id` / `version`；用于卸载前的依赖检查 |

\* 清单必须至少声明一种运行形态：`[backend]` / `[capabilities.frontend]` / `[capabilities.wasm]` 之一。

> 未知键不会被拒绝（除 `checksums.json` 那种显式 `deny_unknown_fields` 的结构），
> 所以原型清单里的旧键会"解析通过但不生效"——这比报错更难查，别照抄旧样例。
> TOML 语法提醒：顶层 `api` / `methods` 必须写在任何 `[表]` 头之前。

### 2.3 `rds` API 面（原型口径，未实现）

> 本节是 P4 扩展宿主的目标 API：当前代码里没有 `rds` 全局对象，也没有 QuickJS 宿主。
> 驱动作者请直接用 §2.8 的协议；下表只用于对齐后续实现。

| 命名空间 | 主要成员 |
| --- | --- |
| `rds.commands` | `registerCommand(id, handler) -> Disposable` · `executeCommand` · `getCommands` |
| `rds.window` | `showInformationMessage` / `showWarningMessage` / `showErrorMessage` · `showQuickPick` / `showInputBox` · `createStatusBarItem` · `withProgress(title, task)` |
| `rds.workspace` | `getConfiguration(section)` · `onDidChangeConfiguration` · `projectPath` / `globalStoragePath` / `logPath` |
| `rds.connections` | `list` / `get` / `getActive` · `onDidChangeActive` / `onDidConnect` / `onDidDisconnect` |
| `rds.metadata` | `getSchemas` / `getTables` / `getColumns` · `onDidChange` |
| `rds.results` | `getActive` · `getRows(handle, range) -> RecordBatch`（**Arrow，不是字符串**） · `export` · `onDidChange` |
| `rds.duckdb` | `query(sql, params?) -> RecordBatch` · `createTempTable(name, batch)` |
| `rds.log` | `info` / `warn` / `error`（带插件 id 前缀进应用日志） |

两条必须记住的纪律：

1. **重计算交给 `rds.duckdb`**，不要在 JS 里 `map`/`reduce` 大数组 —— 扩展宿主是 QuickJS（无 JIT），它适合做编排，不适合做计算。
2. **改 UI 是"请求宿主"，不是"命令宿主"**：你只能创建宿主给你的句柄（如状态栏项）并改它的属性，无法凭空画出界面。

**错误码**（宿主返回的错误里带 `code`）：`-32001 driver_not_supported` · `-32002 session_not_found` · `-32003 sql_error` · `-32004 cancelled` · `-32005 timeout` · **`-32006 capability_denied`** · `-32007 protocol_version_mismatch` · `-32008 resource_limit`。

### 2.4 权限与凭据声明（现行口径）

权限分两类：

1. **声明 / 展示轨**（`[permissions]` 的 `sidecar` / `driver` / `frontend` / `wasm`）：
   安装信息里逐项明示给用户，**当前不门控**；真正的门控在 engine 的
   `DriverCapabilities` 与下面的 `permissions.host`。
2. **运行时门控**（`[permissions.host]`）：点分 glob，默认拒绝，由
   `sidecar::host::HostApiHandler` 校验。

```toml
[permissions]
sidecar = [
  "spawn.child_process",     # 起子进程
  "net.connect",             # 连网络
  "fs.read_plugin_data",     # 读 plugin-data/<id>
  "env.inherit",             # 继承宿主环境变量
  "credentials.database",    # 会收到数据库凭据
]
driver = [
  "driver.query", "driver.metadata", "driver.transaction", "driver.cancel",
  "driver.cursor", "driver.arrow",
]
credentials = "database"     # 单独字段：明确声明"会拿到数据库密码"
host = ["secrets.read.*", "host.ssh_tunnel"]   # 反向 Host API，默认拒绝
```

- 只申请要用的项：这份清单是给用户看的，"为什么要联网"要在安装/详情页能回答（详情页 UI 属 P4）。
- `credentials = "database"` 是显式同意书；没声明就去取凭据，会在 Host API 层被拒。
- 原型稿里的 `[capabilities]` 权限映射（`"rds.metadata" = ["*"]`）与逐项授予 UI 均**未实现**。

### 2.5 什么时候必须做成驱动插件（sidecar）

| 你的需求 | 该走哪一轨 |
| --- | --- |
| 只是编排元数据 / 发通知 / 改状态栏 / 给面板喂数据 | 扩展轨（JS） |
| 需要**真事务**（begin/commit/rollback 跨多语句） | **sidecar** |
| 需要**真取消**（长查询可中断，不是关连接） | **sidecar** |
| 需要**流式游标**（边读边回，结果集不进内存） | **sidecar** |
| 需要**全量元数据浏览**（catalogs/schemas/tables/columns 一次给全） | **sidecar** |
| 需要原生协议（JDBC / 专有 wire protocol / 原生 TLS 连接池） | **sidecar** |
| 纯计算、无 IO、要最硬的沙箱 | wasm 轨：**渲染计算（`render-document`）已落地**（真实样例 `tools/plugin-render-mermaid`）；驱动型 wasm 仍无计划 |

sidecar 的代价要提前知道：它**自己拿得到凭据**（清单里必须声明
`credentials = "database"`，见 §2.4；安装页逐项明示属 P4）、要自己管进程健康、
它崩了对应连接就失效。

要动手写一个？见 §2.8（协议要点 + 自检命令 + 五条硬性约定）。

### 2.6 调试

| 想看什么 | 怎么看 |
| --- | --- |
| 插件日志 | 应用日志按 `target` 前缀筛插件 id；sidecar 的 stderr 落 `plugin-cache/<id>/sidecar.log`（宿主替你落盘） |
| 驱动起不来 | 先看 `[backend]`：kind / executable / interpreter / runtime / platforms；启动失败给的是可读原因，不是"进程启动失败" |
| 加载失败 | 安装成功但加载失败时目录与登记保留，原因进 `tracing::warn`；改完开发目录后文件会被热插拔发现（面板出现「N 个插件文件有变更」），点「重载 N」生效 |
| 权限被拒 | sidecar 调用未声明的 host method 会在运行时被拒，错误里带缺哪一项 |
| 大结果没回来 | 确认 `initialize` 的 `protocol = 1`、`driver.describe` 的 `capabilities`；>200 行 / 256 KiB 必须走 Arrow 附件 |
| 命令 / 面板没数据 | 前端轨未实现（P4）；插件**没有**命令贡献与自定义面板，宿主只有内置的插件列表面板 |

### 2.7 发布前检查清单

- [ ] `plugin.engines.rdatastation` 填了且不是 `*`
- [ ] `[backend]` 自洽：script 有 `interpreter` 或 `runtime`；sidecar 不写 `interpreter_args`；
      `driver_jars` 只出现在 `runtime = "java"`
- [ ] 所有声明入口都在包内、相对路径、POSIX 分隔，且**真实存在**（安装期会逐项点名缺失）
- [ ] 压缩包根层有 `plugin.toml`，或只套一层插件目录（多层 / 一包多插件会被安装器拒绝）
- [ ] 权限最小化：`sidecar` / `driver` 只列要用的；`credentials = "database"` 只在确实要密码时声明
- [ ] `checksums.json` 算法为 `sha256`，覆盖 `plugin.toml` 与所有声明入口
      （多平台包按**解析后**的 `bin/<platform>/agent` 算），存在的 `plugin.icon` /
      `connection_schema` 也要列；缺失会放行，但发布包应当带上
- [ ] 包里不带 `target/`、`.git/`、`node_modules/` 等开发残留（安装器会跳过，
      入口声明在残留目录里会被直接拒绝）
- [ ] 契约里的资源消耗与取消都实现：大结果走 Arrow 附件、`query.cancel` 真中断、
      **stdin 见 EOF 立即退出**（§2.8 五条）
- [ ] 上游前端的贡献点（命令 / 面板 / 设置页）暂不生效——P4 注册表落地前不要写进发布说明

---

### 2.8 写一个 sidecar 驱动（Go / Python / 任何语言）

sidecar 是**独立进程**：宿主起它、用 stdio 与它说 JSON-RPC + Arrow，它自己去连真库。
完整协议与理由在 `plugin-dev-plan.md` §4.2；这里只讲**动手要做什么**。

```text
宿主                              你的 sidecar
 ├─ stdin  ──帧──▶  initialize / driver.describe / session.open /
 │                  query.execute / query.fetch / query.cancel / session.close
 └─ stdout ◀─帧──  响应（结果大时：JSON 头 + 紧跟 N 个 Arrow IPC 帧）
    stderr ──▶   plugin-cache/<id>/sidecar.log（宿主替你落盘）
```

> method 字符串的**权威清单**在 `crates/plugin/src/sidecar/method.rs` 的常量表
> （与 dev-plan §4.2.2 方法表一一对应）；清单里声明 `methods` 时，拼写不在白名单
> 里的会在安装期被拒（协议已知 method 或 `x.` 私有前缀，见 `is_allowed_declaration`）。

帧格式（5 字节头，`total_len` **含头**）：

```text
[u32 大端 total_len][u8 kind][payload]
  kind 0x01 = JSON-RPC 2.0 消息（UTF-8）
  kind 0x02 = Arrow IPC stream 分片
```

**必须守住的五条**（每条都有对应的自动检查）：

1. `initialize` 里 `protocol` 报 `1` —— 不一致宿主**当场拒绝加载**；
2. `driver.describe` 给得出 `display_name` 与 `capabilities`（**没说的一律按不支持**）；
3. 超过 200 行（或 256 KiB）的结果**走 Arrow 附件**：JSON 头里写
   `attachments:[{id:0,kind:"arrow-ipc-stream",frames:N}]`，随后紧跟 N 个 `0x02` 帧；
4. `query.cancel` 要**真的中断**在跑的那条查询，让它以 `-32004` 收场；
5. **stdin 见 EOF 立即退出** —— 宿主死了就自己收场。宿主不依赖平台相关的杀进程组，
   就靠这一条；不守它，用户机器上会留后台进程。

Arrow 的兼容子集（§4.2.4）：little-endian、`LargeUtf8`/`LargeBinary`（64 位 offset）、
时间归一化到 `Timestamp(us, UTC)`、每页一条**自洽** stream（自带 schema）、类型映射写在
field metadata 的 `rds.*` 键上（`rds.type_raw` / `rds.canonical` / `rds.nullable` / `rds.format`）。

小结果**可以内联**（≤200 行且 ≤256 KiB）：JSON 头里给 `rows: [{"列名": 值}, …]` ——
**对象，不是位置数组**。内联只是线格式，宿主对两种承载方式一视同仁。

自检（同一个二进制，换 `RDS_SIDECAR_BIN` 就跑你的）：

```sh
cargo test -p rds-plugin --test sidecar_conformance -- --nocapture

RDS_SIDECAR_BIN=./your-sidecar RDS_SIDECAR_DRIVER=postgres \
RDS_SIDECAR_PARAMS='{"host":"127.0.0.1","port":5432,"database":"demo","username":"me","password":"…"}' \
RDS_SIDECAR_SQL_BIG='select * from generate_series(1, 3000)' \
RDS_SIDECAR_SQL_SLOW='select pg_sleep(5)' \
cargo test -p rds-plugin --test sidecar_conformance -- --nocapture
```

跑通那五条 = P1 的退出标准（能连 → 3000 行 Arrow → 能取消 → 无孤儿进程）；
同一个文件里还有**第 6 条**（导航面：`meta.*` 五件答得出来且形状对）= P2 的退出标准。
加上 `--nocapture` 会把你这边实际答出来的 catalog / schema / 对象数 / 首列的列名与类型打出来，
照着看就知道卡在哪一环。库是空的也算过 —— 那时只验「方法答得出来」并打印说明。

**导航要用的 `meta.*`（P2）**：五件 —— `meta.catalogs` / `meta.schemas` / `meta.objects` /
`meta.object_detail` / `meta.routine_source`。少一件，导航树或属性面板就少一块。三条最容易踩的：

1. `meta.objects` **一次给全、含 `kind`**（`table` / `view` / `materialized_view` / `procedure` /
   `function` / `sequence` / `trigger`）—— 五个文件夹共用这一次内省，别按文件夹分家；
2. **列一定带 `type_raw`，能给就给 `canonical`**（`DECIMAL` / `TIMESTAMP` …）：属性面板显示原始类型，
   程序按 `canonical` 判类型（少了它，mock 与质量分只能猜首词，最后全落成文本）；
3. 能力没声明的东西**别报**：`sequences = false` 就不要在 `meta.objects` 里给序列，
   被点名的调用（`meta.routine_source` 等）如实回 `-32006 capability_denied` ——
   宿主据此置灰并说明，而不是把「不支持」画成「没有」。

   最关键的一条能力是 **`schemas`**：它决定导航给不给你 schema 层。
   - `schemas = true`（PostgreSQL 那类）：`meta.catalogs` 给库、`meta.schemas` 给 schema；
   - `schemas = false`（MySQL / SQLite / DuckDB 那类单层库）：**`meta.catalogs` 要给得出容器**
     （把 schema 名当 catalog 回），`meta.schemas` 返回空，宿主把 catalog 名当 `schema` 传给你
     （`meta.objects` 收到 `schema == catalog`，认它就是）。

   另两个会被门控的：**`cancel`**（没声明时用户点「中断」只能听到「没能停下来」）、
   **`transactions`**（没声明时开事务会被明确拒绝）。

连不上库请用 `-32009 connect_failed`（别用 `-32003`）：界面要把它与「SQL 写错了」分开呈现。
清单怎么写（`[backend]` + `[[contributes.drivers]]`）见 §2.2 与 `plugin-dev-plan.md` §4.3。

---

## 第三部分 · 验收清单

### 3.1 现在能逐条走通（2026-11-07）

- [ ] 装一个本地插件（`.zip` / `.tar.gz` / 目录）：面板回执「已安装 …」，列表出现该插件
- [ ] `plugin.engines.rdatastation` 不匹配：安装期拒绝，报出两个版本号
- [ ] 删掉清单声明的入口：安装期拒绝并**逐个点名**缺失文件
- [ ] 篡改 `checksums.json` 里的哈希：安装期拒绝并指出文件
- [ ] 把 `plugins/<id>` 自身选作安装源：拒绝（避免先删源再复制）
- [ ] 卸载被其他插件依赖的插件：拒绝并列出引用者
- [ ] 停用 / 启用：进程当下停/启，`is_enabled` 持久化，重启应用后状态保持
- [ ] 删除后 `plugin-data/<id>` 仍在，重装后数据可继续用
- [ ] 驱动轨自检（§2.8）：`cargo test -p rds-plugin --test sidecar_conformance` 五条 + 导航面
- [ ] 长查询取消：驱动侧 `query.cancel` 真中断并以 `-32004` 收场（sidecar 测试覆盖）

### 3.2 待 P4 / P5（原型口径，现在走不通）

- [ ] 装未签名插件：拒绝安装并说明哪一项校验失败（`signature.json` 与信任链未实现）
- [ ] 安装前列出每一项权限与用途 → 用户确认（权限页未实现）
- [ ] 装完命令出现在命令面板 / Quick Open（扩展宿主与注册表未实现）
- [ ] 插件详情页四区、搜索、市场、可更新标记（未实现）
- [ ] 项目引用了被禁用/未安装插件时显示占位与两个出路（引用对账未实现）
- [ ] 杀掉扩展宿主：全部插件失活、宿主可一键重启（扩展宿主不存在）
- [ ] 杀掉驱动进程：只有该连接失效、可手动重启（驱动侧已有；UI 入口未做）
- [ ] webview 面板全屏 / 独立窗口、不与 GPUI 元素混排（未实现）
- [ ] 主题切明暗：插件**面板**已随主题（走主题 token）；webview 面板要等 P4
