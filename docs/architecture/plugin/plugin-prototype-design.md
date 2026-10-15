# 插件系统（M9）— 原型设计（VS Code 模式）

> 本文给出「按 VS Code 模式」的插件形态原型：**进程与信任边界 / 清单 / API 面 / 激活时机 / 贡献点映射 / 示例插件**。
> 关系：`plugin-architecture.md` 记设计意图，`plugin-dev-plan.md` 记排期与改动清单，本文只定「扩展长什么样」。
>
> **本文不推翻 dev-plan 的 D1（三轨承载）**，而是把其中「面板/命令/设置 = 脚本」那一轨的**具体形态**定下来，并回答「这个方式是否合适」。
> 视觉与尺寸口径仍以 `../ui/`、`../theme/` 为准；本文不定义新的视觉原语。

---

## 1. 一句话形态

> **宿主 = RdataStation 的 GPUI 窗口；扩展 = 独立「扩展宿主进程」里的 JS/TS 模块；扩展不碰渲染路径，只通过 `rds.*` RPC 请求宿主修改宿主自己的 UI 模型。**

对照 VS Code 的原文表述（已核实的公开口径）：

| VS Code 的做法 | 本原型的对应 |
| --- | --- |
| 扩展运行在独立 extension host 进程 | 同：`rds extension host`（一个新进程，不是 GPUI 进程内的 wasm） |
| 扩展**禁止任意 UI**（无 DOM、无自定义样式表），只能通过贡献点 + Webview | 同：只能通过贡献点；**Webview 可提供，但限两种承载**（全屏面板 / 独立窗口，Q8 定案见 §11.2） |
| `contributes` 声明 + `activationEvents` 懒激活 | 同：`contributes.*` + `activation` |
| `engines.vscode` 版本闸（必填、不可 `*`） | 同：`engines.rds`（必填） |
| 扩展与 VS Code **同权**，只有 Workspace Trust 与发布者信任 | **本原型更强**：`rds.*` 是能力门控的，权限可精确到方法（§4.4） |

---

## 2. 进程与信任边界

```mermaid
flowchart TD
    A["rds.exe<br/>宿主：GPUI 窗口 + 全部 UI 状态<br/>（不加载任何插件代码）"]
    B["Extension Host 进程<br/>1 个，承载全部 UI/命令/设置/规则类扩展<br/>JS/TS，无 UI 权限"]
    C["Driver Sidecar 进程<br/>N 个，每个驱动插件一个<br/>native，任意语言"]
    D["DuckDB<br/>进程内分析引擎"]
    A -->|"反向 RPC（请求宿主改自己的模型）"| B
    A -->|"JSON 控制面 + Arrow 数据面<br/>（stdio 分帧）"| C
    A --> D
    B -.->|"无直接数据库访问<br/>无凭据"| A
    C -->|"自带连接与凭据<br/>（安装时明示）"| E["目标数据库"]
```

三条硬约束（**是原型的一部分，不是建议**）：

| # | 约束 | 理由 |
| --- | --- | --- |
| 1 | **驱动插件不在扩展宿主里** | 驱动要原生 socket / TLS / 连接池；且一个驱动崩溃不该拖垮所有扩展 |
| 2 | **扩展宿主不直接访问数据库** | 它只能通过 `rds.connections` / `rds.results` 反向请求宿主 → **凭据不出宿主进程**（守护 dev-plan §4.6.3 的口径） |
| 3 | **扩展代码不进入渲染路径** | 与「`render` 是纯读路径」的既有纪律一致；也让「零裸尺寸 / 零裸色值」两条契约测试继续可守 |

---

## 3. 清单（`plugin.toml`）

沿用既有 `PluginManifest`（TOML，`crates/plugin/src/manifest.rs`），按 VS Code 的 `contributes` 语义增量扩展：

```toml
schema_version = 1

[extension]
id           = "publisher.sql-notebook"     # 反向域名式 id（VS Code 惯例）
version      = "1.2.0"
display_name = "SQL Notebook"
description  = "把查询结果组织成可复跑的笔记本"
publisher    = "example"
license      = "MIT"
engines.rds  = "^0.2"                       # 必填，不允许 "*"
categories   = ["Notebooks", "Data"]

[main]                                       # 扩展宿主的入口（懒加载）
module     = "dist/extension.js"
activation = [
  "onCommand:sqlNotebook.open",              # 命令
  "onConnection:mysql",                      # 连上某类库（≈ onLanguage）
  "onView:sqlNotebook.explorer",             # 展开某视图
  "onInsightRule",                           # 洞察面板被打开
  "onStartupFinished",                       # 启动完成（慎用）
]

# ---------- 静态贡献：宿主启动时只登记"声明"，不加载代码 ----------

[[contributes.commands]]
command  = "sqlNotebook.open"
title    = "打开 SQL Notebook"
category = "Notebook"

[[contributes.commands]]
command = "sqlNotebook.exportCurrent"
title   = "导出当前结果为 Notebook"

[[contributes.menus]]
# 挂载点是宿主的固定枚举（见 §6）
"nav/item/context"    = [{ command = "sqlNotebook.addToNotebook", when = "viewItem == table" }]
"result/grid/context" = [{ command = "sqlNotebook.exportCurrent", when = "result.hasSelection" }]
"title/overflow"      = [{ command = "sqlNotebook.open", group = "navigation" }]

[[contributes.configuration]]
key     = "sqlNotebook.defaultRowLimit"
type    = "number"
default = 1000
scope   = "project"                          # application | project | session（对齐 settings 三分法）
label   = "默认取样行数"

[[contributes.views]]
id       = "sqlNotebook.explorer"
name     = "Notebooks"
location = "left"                            # left | right | center
icon     = "media/notebook.svg"
when     = "projectTrusted"

[[contributes.insightRules]]                 # RdataStation 特有：洞察规则（TOML 或 JS 声明）
[[contributes.mockGenerators]]               # RdataStation 特有：造数生成器
[[contributes.exporters]]                    # 导出格式
[[contributes.drivers]]                      # 驱动（**另起 native 进程**，见 §2 约束 1）

[dependencies]
"publisher.common-utils" = "^1.0"
```

**字段来源与理由**：

| 字段 | 来自 | 为什么必须 |
| --- | --- | --- |
| `id` | VS Code `publisher.name` | 全局唯一 + 可归属（卸载/升级/引用对账都靠它） |
| `engines.rds` | VS Code `engines.vscode` | **必填且不可 `*`**；加载与安装两处都要真的拒绝（现 `check_engine_compatibility` 未被强制） |
| `activation` | VS Code `activationEvents` | 不激活不加载代码——**启动性能的结构性保证** |
| `contributes.*` | VS Code `contributes` | 声明式，宿主启动时零成本登记 |
| `when` 子句 | VS Code `when` 上下文键 | 菜单/视图的可用性条件，避免插件自己判断 |

---

## 4. API 面：`rds` 模块

### 4.1 设计原则

| # | 原则 |
| --- | --- |
| 1 | **窄面**：只 8 个命名空间。VS Code 的 `vscode` 模块有 300+ 成员、十几个命名空间，是十年积累——照抄不可行，也不必要 |
| 2 | **一切创建动作都返回句柄**：`createStatusBarItem()` 返回一个宿主创建的句柄，插件只能改属性。**插件无法凭空画东西** |
| 3 | **一切订阅返回 `Disposable`**：卸载/失活时统一释放（VS Code 的核心模式） |
| 4 | **行集用 Arrow**：`rds.results.getRows(range)` 返回 `RecordBatch`，与 dev-plan §4.5 的数据面一致，不给 JSON 字符串 |
| 5 | **改 UI 是请求，不是命令**：`showInformationMessage` 而不是 `appendToDom` |
| 6 | **重计算下沉 `rds.duckdb`**：扩展要做聚合/变换，就写成 SQL 交给 DuckDB，**不在 JS 里 map/reduce**。这不是绕开限制，而是与产品定位一致（DuckDB 是引擎，JS 是胶水）；也是 Q7 选 QuickJS（无 JIT）之后必须写进 API 文档的纪律 |

### 4.2 命名空间（原型定稿）

```
rds.commands      registerCommand(id, handler) -> Disposable
                  executeCommand(id, ...args)
                  getCommands()

rds.window        showInformationMessage / showWarningMessage / showErrorMessage
                  showQuickPick / showInputBox
                  createStatusBarItem(alignment, priority) -> StatusBarItem
                  withProgress(title, task)

rds.workspace     getConfiguration(section) -> Configuration
                  onDidChangeConfiguration(handler) -> Disposable
                  projectPath / globalStoragePath / logPath

rds.connections   list() / get(id) / getActive()
                  onDidChangeActive / onDidConnect / onDidDisconnect

rds.metadata      getSchemas(conn) / getTables(conn, schema) / getColumns(conn, schema, table)
                  onDidChange(conn)

rds.results       getActive() -> ResultHandle?
                  getRows(handle, range) -> RecordBatch     // Arrow，不是字符串
                  export(handle, format, path)
                  onDidChange(handler)

rds.duckdb        query(sql, params?) -> RecordBatch        // 只读 + 临时表
                  createTempTable(name, batch)               // 只写分析库

rds.log           info / warn / error                        // 进应用日志，带扩展 id 前缀
```

### 4.3 句柄类型（宿主拥有，插件只读/改属性）

```
StatusBarItem   .text / .tooltip / .command / .show() / .hide() / .dispose()
ResultHandle    .columns / .rowCount / .sourceConnection / .isStale
```

### 4.4 权限：比 VS Code 更严（本原型的加分项）

VS Code 的扩展与编辑器**同权**（只有 Workspace Trust 与发布者信任两道门）。本原型可以更严，因为 `rds.*` 是 RPC——**每个方法都能单独门控**：

```toml
[capabilities]
"rds.connections"       = ["list", "get"]        # 不给 getActive 也行
"rds.metadata"          = ["*"]
"rds.results.getRows"   = "project-data"         # 需要项目可见的数据
"rds.duckdb.query"      = "read-only"
"rds.duckdb.createTempTable" = true
"rds.workspace.getConfiguration" = true
"fs.write"              = ["${globalStorage}"]   # 只能写自己的存储目录
"net.http"              = []                     # 扩展宿主默认无网络
```

**默认拒绝**：未声明的命名空间调用直接返回 `capability_denied`（错误码见 dev-plan §4.2.2），宿主在 UI 上给出可读原因。

---

## 5. 激活与生命周期

```
① 扫描   <RDS_HOME>/plugins/<publisher>.<name>/plugin.toml   （目录：paths::plugins_dir，待补）
         → 校验 schema_version / engines.rds（不匹配即拒绝，报出两个版本号）
         → **不运行任何代码**，只登记静态贡献

② 启动   宿主把静态贡献合并进三个注册表：
         CommandRegistry / PanelRegistry / 设置登记表
         （命令面板里能看到插件命令，但扩展代码**还没加载**）

③ 激活   某个 activation 事件被触发
         → 在 Extension Host 进程里加载 module（懒）
         → 调 activate(context)
           context = { subscriptions, extensionId, extensionPath,
                       globalStoragePath, workspaceState, globalState }

④ 运行   扩展只能通过 rds.* 反向 RPC 请求宿主；订阅返回 Disposable

⑤ 失活   deactivate() → 释放全部 subscriptions
         → 超时（本原型取 2s）后强杀该扩展所在宿主；**宿主 UI 不受影响**
```

**崩溃与隔离**：

| 事件 | 表现 | 恢复 |
| --- | --- | --- |
| 某扩展抛异常 | 该扩展的 `handler` 调用返回错误，其余扩展不受影响 | 扩展自己 catch；宿主记日志 |
| Extension Host 进程崩溃 | 全部扩展失活；宿主弹「扩展宿主已崩溃」 | 一键重启扩展宿主；驱动连接不受影响 |
| Driver Sidecar 崩溃 | 只有那个连接的会话失效 | 如实报错（不静默重连）；可手动重启该驱动 |

---

## 6. 贡献点映射表（VS Code ↔ RdataStation）

| VS Code 贡献点 | RdataStation 对应 | 宿主实现位置 | 状态 |
| --- | --- | --- | --- |
| `commands` | 同 | `quick_open/commands.rs` → `CommandRegistry` | 需建（dev-plan P4） |
| `menus` | 挂载点：`nav/item/context`、`result/grid/context`、`editor/title`、`title/overflow`、`statusBar` | 各面板的既有右键菜单 | 需建 |
| `keybindings` | 同（键位冲突由宿主裁决） | app 层 `cx.bind_keys` | 待定（Q9） |
| `configuration` | 设置项（作用域 `application`/`project`/`session`） | `settings` 登记表 + 准入五条 | 需建 |
| `views` / `viewsWelcome` | 左右 Dock 面板 + 中央 tab | `LeftPanel`→`PanelId` + `PanelRegistry`（**未注册 id 显示占位**） | 需建（dev-plan P4） |
| `TreeDataProvider` | 插件自己的树（宿主渲染，插件给节点数据） | `gpui_kit::component::list` 系 | 需建 |
| `WebviewPanel` / `CustomEditor` | **本原型不提供**（见 §11 Q8） | — | ❌ |
| `languages` / LSP / hover / completion | SQL 方言的补全 / 格式化 / hover | editor 补全端口（**当前是欠账**：`editor` 无 `database` 依赖） | 依赖前置 |
| `debuggers` / `tasks` / `terminal` | ❌ 不映射 | — | ❌ |
| `sourceControl` / `testing` / `chat` | ❌ 不映射 | — | ❌ |
| **（VS Code 没有的）** | | | |
| — | `contributes.drivers`（native sidecar） | `engine` 驱动注册面 + dev-plan §4.1 | 需建 |
| — | `contributes.insightRules`（洞察规则） | `crates/insight` 规则加载（TOML 三层作用域已就绪） | 可接 |
| — | `contributes.mockGenerators`（造数生成器） | `crates/mock` 生成器目录（143 变体） | 待定 |
| — | `contributes.exporters`（导出格式） | `editor` 五种导出 | 待定 |

**这张表决定了原型的落地顺序**：`commands` / `menus` / `configuration` / `views` 四项是 VS Code 模型的最小可用集，也正是 dev-plan P4 要建的三张注册表。

---

## 7. 示例插件（完整走一遍所有机制）

**「表大小看板」**：贡献一个左侧面板 + 一个命令 + 一个设置项 + 一个导航树右键项。

```js
// dist/extension.js
const rds = require('rds');

function activate(context) {
  // ① 命令（声明已在 plugin.toml，这里注册实现）
  context.subscriptions.push(
    rds.commands.registerCommand('tableBoard.refresh', async () => {
      const limit = rds.workspace.getConfiguration('tableBoard').get('topN', 20);
      const conn = rds.connections.getActive();
      if (!conn) return rds.window.showInformationMessage('请先连接一个数据源');

      // ② 元数据：宿主已有三级缓存，扩展不直连数据库
      const tables = await rds.metadata.getTables(conn, undefined);

      // ③ 进度：宿主渲染进度条，扩展只上报
      await rds.window.withProgress('统计行数', async (report) => {
        for (const [i, t] of tables.entries()) {
          report(i / tables.length, t.name);
        }
      });

      // ④ 把结果交给宿主渲染（扩展不画 UI）
      await rds.views.update('tableBoard.explorer', {
        title: `Top ${limit} 表`,
        items: tables.slice(0, limit).map(t => ({
          label: t.name, description: t.comment ?? '', id: t.name,
        })),
      });
    })
  );

  // ⑤ 订阅：返回 Disposable，失活时自动释放
  context.subscriptions.push(
    rds.connections.onDidChangeActive(conn => rds.log.info(`活动连接切换到 ${conn?.name}`))
  );
}

function deactivate() {}   // 无需额外清理：全部挂在 context.subscriptions
```

**注意这份示例里被刻意避免的东西**：没有 `div()`、没有样式、没有像素；树是宿主渲染的，扩展只给 `{label, description, id}`。**这就是 VS Code 模型的核心收益。**

> 本节讲的是**扩展宿主侧**（面板数据、命令、设置）。若面板必须自己画（图表、仪表盘这类），走的是 webview 承载 —— 形态、边界与可点的原型见 §11.2。

---

## 8. 与既有设计的接缝

| 既有设计 | 本原型如何接 |
| --- | --- |
| `PluginManifest`（唯一契约，dev-plan D3） | 本文 §3 是它的**扩展**，不是第二份清单 |
| 三层对象模型（P1） | 驱动插件按 §4.1 的 `PluginProcess → DriverInstance → Session` 跑，**不经过扩展宿主** |
| 能力矩阵（dev-plan §4.4） | 驱动能力在 `contributes.drivers` 的 `[capabilities.driver]`；扩展能力在 `[capabilities]` 的 `rds.*` |
| 项目引用对账（dev-plan §4.8） | 扩展与驱动都进 `project_resources(kind=…)`；未装 → 占位面板（「项目引用了 X，未安装」） |
| `render` 纯读路径 + 零裸尺寸/零裸色值 | **本原型天然满足**：扩展代码不在渲染路径，UI 全部由宿主用既有常量与 token 渲染 |
| 凭据口径（dev-plan §4.6.3） | 扩展宿主**拿不到凭据**（它不连库）；只有驱动进程拿得到，且安装时明示 |

---

## 9. 与 gpui-shell 的分工（两个模型互补，不是二选一）

| | VS Code 模型（本文） | gpui-shell 模型（`gpui-kit` 自带） |
| --- | --- | --- |
| 扩展产出 | **对宿主的请求**（改状态栏、挂菜单、给树数据） | **一棵界面描述树**（脚本描述、Rust 材质化） |
| 扩展能否定义视觉 | 不能（用宿主既有控件） | 能（用宿主给的原语 + 主题 token） |
| 宿主原语 | 无（只有贡献点） | `div`/`h_flex`/`Button`/`Input`/`PathBuilder`… |
| 适合 | 覆盖 ~90% 需求：命令、面板数据、设置、通知、状态栏 | 「我需要一个带表格和按钮的自定义界面」 |
| 上游状态 | 需自建（本文即设计） | `publish = false`（未发布 crates.io）；0.7.1 源码与文档已完整（`plugin.rs` 198 符号、`docs/gpui-shell.md` 28 章），门面 `gpui-kit` **不聚合 shell**，要用需 path/vendor；接入与否待 P4 重启时评估 |

**分工建议**：**默认走 VS Code 模型**（能力可控、UI 一致、启动快）；「自定义界面」作为**显式声明的可选能力**，后端可以是 gpui-shell（待发布）或独立的「插件视图区」（若提供 webview）。**不要**让两条路对同一块 UI 都能改——那会产生两套视觉语言。

---

## 10. 明确不做

- 任意 UI（DOM/样式表/自绘）——与 VS Code 同口径，且与「零裸尺寸/零裸色值」契约冲突
- 扩展宿主直接访问数据库或凭据
- 驱动插件跑在扩展宿主里
- 一个 300+ 成员的 `rds` API 面（只做 §4.2 那 8 个命名空间）
- 调试器 / 任务 / 终端 / 源码管理 / 测试 / 聊天类贡献点（产品无此域）
- 让扩展改宿主的布局状态机（面板位置由用户的布局决定，扩展只能声明 `location` 偏好）
- **Node / npm 兼容层（有意不做）**：不提供 `require('fs')`、`node:` 命名空间、npm 依赖解析。理由：本产品的扩展面向**数据与元数据编排**，npm 的富池优势在这里收益有限；代价是 38 MiB 级运行时与构建复杂度（见 §11 Q7）
- **在 JS 里做重计算**：不提供大数组/批量变换的性能承诺；`rds.duckdb` 是唯一的重计算出口（§4.1 原则 6）

---

## 11. 新的待定决策

| # | 决策 | 选项 | 说明 |
| --- | --- | --- | --- |
| **Q7** | **JS 运行时从哪来** | ✅ **已定：(a) 内置 QuickJS（经 `rquickjs`）** —— 见下方小字 | 两条前置已确认：① 重计算交给 DuckDB（可接受）；② 不兼容 npm（可接受，本产品面向数据领域，npm 优势不大） |
| **Q8** | **是否提供 Webview** | ✅ **可提供，但限两种承载**（全屏面板 / 独立窗口）；混排**写得出、但会被原生子窗口盖住**（z-order）→ 实践上不支持 —— 依据见 §11.2 | `gpui-wry` 与 `gpui-kit` **同仓不同 crate**，需单独加依赖且**本仓应锁 `=0.6.1`**（否则顶起整条 gpui 栈）；它与 `gpui-shell`（`publish = false`）**不是同一回事** |
| **Q9** | **键位贡献是否开放** | (a) 开放（宿主裁决冲突）｜(b) 不开放，只允许在命令面板里出现 | VS Code 开放；但你们有「注册了才宣传」的快捷键纪律，冲突裁决规则要先定 |
| **Q10** | **wasm 轨是否保留** | (a) 保留为「纯计算/性能轨」｜(b) 冻结（用 JS 宿主吸收） | 见 §12 |
| **Q11** | **ER 建模视图渲染路线** | webview 轨 **已否决**（drawdb / erd-editor / Mermaid 均需 webview；用户定调：本项目由 tauri 迁来，不引入前端栈）；候选 = (a) **GPUI 原生渲染**（推荐：视图 = 宿主原生 panel，插件贡献视图声明 + 数据）｜ (b) 暂缓 | 见 §11.3；宿主侧接入仍随 D7 暂缓 |

### 11.1 Q7 定案依据（2026-09-20 核实）

**V8（`rusty_v8` v152.2.0，2026-08-20）各平台预编译静态库的**下载体积**（`denoland/rusty_v8` Release 资产，gzip 后）：

| 平台资产 | 体积 |
| --- | --- |
| `librusty_v8_release_x86_64-unknown-linux-gnu.a.gz` | 37.9 MiB |
| `librusty_v8_release_x86_64-pc-windows-msvc.lib.gz` | 38.1 MiB |
| `librusty_v8_release_aarch64-unknown-linux-gnu.a.gz` | 36.7 MiB |
| `librusty_v8_release_aarch64-apple-darwin.a.gz` / `x86_64-apple-darwin` | 38.8 / 38.6 MiB |
| `librusty_v8_debug_x86_64-unknown-linux-gnu.a.gz` / `aarch64-apple-darwin` | 78.9 / 100.4 MiB |

附带：**Windows 只有 MSVC 资产，无 `windows-gnu`**；换 target 或清 target 目录都要重下这一份。

**QuickJS（`rquickjs` 0.14.0，2026-09-18）**：

| 项 | 值 |
| --- | --- |
| 许可 / 活跃度 | **MIT**；436 万总下载 / **195 万近期** / 43 个版本 |
| `rquickjs-sys` crate | **2.13 MB**，内含 **C 源码 82,647 行（21 文件）+ 头文件 13,437 行（60 文件）** → **vendored QuickJS，无外部下载** |
| `bindgen` | **可选 feature**；默认用随 crate 预生成的绑定 → **Windows 只需 C 编译器（MSVC `cl.exe`），不需要 LLVM/libclang** |

**为什么「V8 的强项我们用不上」**（本次调研的关键判断）：

| V8 的优势 | 在本产品的处境 |
| --- | --- |
| npm / Node 生态兼容 | 扩展模型**故意不给 Node 能力**（`rds.*` 能力门控、宿主默认无网络无文件系统、明确不做 Node 兼容层）→ 为用不上的能力付 38 MiB + 链接代价 |
| JIT 性能 | 扩展的活是「元数据编排 + RPC + 少量变换」，不是热循环；**重计算有更好的出口：`rds.duckdb`** |

一句话：**我们不需要一个跑得快的 JS，只需要一个装得下的 JS。**

**未核实**：① `rusty_v8`/`deno_core` 与 `rquickjs` 的**最终二进制净增量**（必须本地实测）；② rquickjs 的 CI 矩阵是否含 Windows 作业（本次抓 `raw.githubusercontent` 报 `missing Content-Type header`，未取到）；③ 冷启动耗时对比数字。

**本地验证（比 CI 矩阵更贴你们的环境）**：

```sh
# A 档：先记基线，再加依赖比体积
cargo build --release -p rds-app && ls -l <产物路径>
# 加 rquickjs 后重跑，比体积差 + 首次构建耗时

# B 档：隔离 target 目录，避免污染现有构建缓存
CARGO_TARGET_DIR=target-v8 cargo build --release -p rds-app
# 观察 rusty_v8 build script 的下载量（~38 MiB）与首次构建耗时
```

### 11.2 Q8 定案依据（2026-09-20 核实）

> 前提纠正：早期稿子把 webview 与 `gpui-shell` 一并列为「上游未就绪」——**不准确**。`gpui-shell` 是 `publish = false`，而 `gpui-wry` **已发布**。

| 项 | 值 |
| --- | --- |
| crate 名 | **`gpui-wry`**（仓库路径 `crates/webview`） |
| 最新版本 / 更新 | **0.6.4** / 2026-09-18（**本仓应锁 `=0.6.1`**，理由见下） |
| 许可 / 发布 | **Apache-2.0**；`publish = true`，docs.rs 有文档 |
| 下载 | 2,686 总 / **2,433 近期**（近 90% 是近期 → 活跃使用中） |
| 依赖 | `wry = { version = "0.53.3", package = "lb-wry" }`（longbridge 的 wry fork，**有版本依赖、非 git pin**） |
| 本仓应锁 | **`=0.6.1`**（理由见下方「版本耦合」） |

**归属澄清：`gpui-kit` 到底支不支持 webview？**（2026-09-20 核实）

| 问题 | 答案 |
| --- | --- |
| 是同一个项目吗 | **是**。`gpui-wry` 的 `repository` 字段 = `github.com/longbridge/gpui-kit/tree/main/crates/webview` —— 与 `gpui-kit` **同仓**（0.5.0 时代该路径挂在 `longbridge/gpui-component` 下，改名后并入 gpui-kit；最早一版发布于 2025-12-08） |
| 是同一个 crate 吗 | **不是**。`gpui-kit 0.7.1` 的依赖只有 `gpui-pre`（=0.3.8）/ `gpui-base` / `gpui-component`(可选) / `gpui-kit-assets`(可选) / `gpui-pre-platform`(平台层) —— **没有 `gpui-wry`**（0.7.1 实测，2026-10-11）。故「用 gpui-kit 就自带 webview」**不成立**，要另加一行依赖 |
| 会是两套 GUI 世界吗 | **不会**。两者都依赖 `gpui-pre`（本仓 lock 是 `0.3.8`）→ `WebViewElement` 直接进现有视图的布局，类型系统同源 |
| `egui-kit` 呢 | crates.io 上**不存在这个名字**（404）。若指 egui（Emil 的 immediate-mode GUI），它**没有官方 webview**，社区做法同自接 `wry`、踩同一个 z-order 问题 —— 与本项目无关，本项目用的是 GPUI |

**版本耦合：这条决定依赖怎么写**（2026-09-20 核实）

| `gpui-wry` | 对 `gpui-pre` 的要求 | 与本仓 lock（`gpui-pre 0.3.8` + `gpui-kit 0.7.1`）|
| --- | --- | --- |
| **0.6.1**（2026-09-09）| `^0.3.1` | ✅ **兼容**（0.3.4 满足）；只增 `lb-wry` 及其平台树，**不动现有 GUI 栈** |
| 0.6.4（2026-09-18，最新）| `^0.3.5` | ⚠️ 会把 `gpui-pre` 从 0.3.4 顶到 0.3.5。`gpui-base`/`gpui-kit` 对 `gpui-pre` 都是 `^0.3.1`（caret），所以**技术上可解析**，但这是**上游没发布过的组合** |

→ **写法**：要用就精确锁一个与当前 `gpui-pre` 兼容的 `gpui-wry`（0.7.1 时代需另行查 `gpui-wry` 最新版本及其对 `gpui-pre` 的要求，勿照抄 0.6.x 结论），**不要写 caret** —— 否则解析漂移并连带顶起整条 gpui 栈。升 gpui-kit 时 `gpui-kit` / `gpui-wry` / `gpui-pre` **三个一起升**（沿用根 `Cargo.toml` 里「表现层同版本」那条纪律）。

**附带实测：升 gpui-kit 到底会动多少东西**（2026-09-20，`cargo update -n`，只预演、不写锁）：

| 预期操作 | 实测结果 |
| --- | --- |
| `cargo update -p gpui-kit` | **连动 29 个包**：`gpui-kit` / `gpui-base` / `gpui-component`(+macros) / `gpui-kit-assets` 0.6.1 → 0.6.4，同时 `gpui-pre*` 整族 0.3.4 → 0.3.5，另新增 `objc2-screen-capture-kit`（仅 macOS） |
| `cargo update -p gpui-pre`（只动它） | **锁纹丝不动**（Locking 0 packages）—— 说明这一代是被上游成组钉住的，单独动其中一个解析不动 |

两个结论：

1. **本仓要升就整组升** —— 正好印证根 `Cargo.toml` 里那条「表现层必须同版本」的注释。
2. **不要跑裸 `cargo update`** —— manifest 写的是 caret（`gpui-kit = "0.6.1"`），裸 update 会**静默**把整条 UI 栈顶到 0.6.4 代。要升就写 `-p gpui-kit`，让这次改动是一份**可 review 的成组变更**。

**未核实**：加 `lb-wry 0.53.3` 后的**二进制净增量与首次构建耗时**（参照 §11.1 的 A/B 档写法本地实测）。渲染侧由系统 WebView2 / WKWebView 承担，**不随我们的产物打包**（wry 的既有事实，非本次实测）。

**API 很薄，但有逃生口**：

```
WebView        new(webview, window, cx) / load_url(url) / show() / hide()
               visible() / bounds() / back() / raw() / handle()
WebViewHandle  raw()
WebViewElement new(...)                     ← 作为 GPUI 元素挂进布局
```

`raw()` 返回底层 `&wry::WebView` → **`evaluate_script` / `load_html` / IPC handler 等能力都在**。所以「limited features」指的是**这层封装薄**，不是能力阉割。

**三条限制的准确归属**：

| 限制 | 归属 | 影响 |
| --- | --- | --- |
| "experimental / limited features" | README 自述 | 上层 API 可能变；但底层 wry 成熟 |
| **只支持 macOS / Windows** | **GPUI 集成层**，不是 wry 层（wry 三平台都支持：WebView2 / WKWebView / webkit2gtk） | Linux 上不可用；但它有替代路径（自接 wry 或等上游） |
| 渲染在 GPUI 窗口之上、会盖住背后元素 | 原生子窗口的本质 | **只在「混排」时是真问题**（见下） |

**按承载分三档（这才是 Q8 的真正答案）**：

| 档 | 用法 | z-order 问题 | 判断 |
| --- | --- | --- | --- |
| ① | **全屏面板**：占满某个 Dock 面板或中央 tab 的全部内容区 | **不存在**（背后没有需要露出的 GPUI 元素） | ✅ 推荐 |
| ② | **独立窗口**：官方建议的形态 | 不存在 | ✅ 推荐（OAuth、独立图表窗） |
| ③ | **与 GPUI 元素混排**：同屏还有下拉 / 菜单 / 对话框要浮在上面 | **真实且难解**——GPUI 的 dialog/overlay 会被原生子窗口盖住 | ❌ **实践上不支持**（不是 API 不允许：`WebViewElement` 本来就能进布局；是**层级不受 GPUI 控制**。宿主若给这一档，必须自己保证浮层不被盖住，做不到就别开） |

**一条必须写清的架构含义**：**webview 里的 JS 与扩展宿主不是一回事。** 它没有 `rds.*`（那是「宿主 ↔ 扩展宿主」的 RPC，走 stdio 帧），要拿数据只能走 `raw()` 暴露的 `evaluate_script` / 自定义 IPC handler。所以自洽的分工只有一种：**webview = 展示层 / 绘图层，编排仍在扩展宿主**。这正好对上「宿主把结果集投喂进图表，图里只做渲染」这条链路（数据面仍按 D4 走 Arrow，见 plugin-dev-plan §4.2.4 与 §4.5）。

**形态原型（可直接打开，不需要 gpui-wry 就能评审）**：`docs/architecture/plugin/prototype/webview-chart-panel.html`

单文件、离线、无构建，把上面几条结论做成能点的东西：X/Y/聚合方式一改就**向宿主发命令**（面板自己不聚合）、导出 PNG 走**能力门控**、主题跟宿主明暗切换、通道断开有横幅。底部「宿主模拟器」的两列日志就是 `evaluate_script`（宿主→面板）与 IPC handler（面板→宿主）在真实现里的位置。

> 2026-09-20 用 Node + 最小 DOM 桩跑过一次冒烟（脚本已删）：握手顺序为 `capabilities → theme → result`；启动结果集 4 列 / 12 行且首行数值有限；改维度后确实走完「面板发 `chart.rerender` → 宿主重算 → 回投 3 行」往返；取消能力后点导出确实回 `-32006 capability_denied`。

**战略意义（比「自定义界面」更重要）**：**webview 是复用 Vega / Plotly / D3 / ECharts 的唯一路径。** README 里「图表 / 仪表盘 / 报表（留给插件生态）」这句话，**没有 webview 就只能靠自绘，而自绘的生态基本为零**。所以 Q8 不只是「要不要让插件画界面」，而是**图表生态要不要的问题**。

**一个需要产品回答的前置：是否支持 Linux？** 代码里三个平台的 `cfg` 分支都有（`engine/src/cache/lru_cache.rs` 三平台、`insight/src/jobs.rs` windows+macos、`project/src/ui.rs` windows），但 README **未声明目标平台**。

- 若 Win + macOS 为主 → webview 可作为**正式能力**，Linux 上如实置灰
- 若要 Linux 同等支持 → webview 只能作为**平台可选能力**，清单 `platforms` 字段（§3 已有）就是为这类情况准备的

**建议**：把 webview 作为**显式声明的可选能力**开放，**限 ① ② 两种承载**；插件在能力声明里写 `requires: webview`，宿主据平台置灰并给出原因（对照 dev-plan §4.2.2 的错误码 `-32006 capability_denied`）。

### 11.3 ER 建模视图插件评估（2026-10-14，方向收窄：webview 轨否决）

**结论（更新）**：ER 图模型设计以插件形式进入**合适，但视图不走 webview**。用户定调：本项目由 tauri 架构迁来，**不引入 webview / 前端栈**（避免 z-order、输入桥接、主题不一致、性能一类前端问题）。替代路线：**GPUI 原生渲染**——视图 = 宿主原生 panel；插件系统退到**数据源 + 视图声明**角色。

**论证**：

- **桌面工具先例清一色原生渲染**（本次核实，非 webview）：

  | 工具 | 技术栈 | ER 图实现 |
  | --- | --- | --- |
  | DBeaver | Java + SWT | Eclipse **GEF/Draw2D**（SWT 图形框架）：实体 = 矩形 figure、关系 = 连线 + crow's foot、拖拽 / 自动布局 |
  | DataGrip（JetBrains 系） | Java + Swing | IntelliJ Platform **Diagram 框架**（`com.intellij.diagram`）：Swing/Java2D 原生渲染，figure + connection 模型 |
  | Navicat / HeidiSQL | C++ Qt / Delphi | 各自原生图形框架 |

- **web 生态图编辑器全部隔着 webview**：drawdb、erd-editor、dbdiagram、Mermaid 都是浏览器应用。**erd-editor 与 drawdb 同判**——TS + DOM + Canvas 2D，QuickJS 扩展宿主跑不了（依赖 DOM）、wasm 轨冻结（Q10），只能走 webview → 一并否决。
- **「以插件形式进入」的成立方式**：视图渲染本身不进插件（无轨可承载自由画布），但插件贡献 `contributes.views` 声明 + 数据变换（`rds.metadata` → ER 输入模型；后续 DDL 生成）。v1 只读渲染甚至可不依赖插件系统（宿主 `MetadataBrowser` 直取），插件是**加分项不是前提**——与 D7 节奏天然对齐。
- **数据源现成**（不变）：P2 的 `meta.*` 五件套（schemas / tables / columns / constraints / triggers）+ 宿主 `MetadataBrowser`；符合硬约束 #1：不直连库、不碰凭据——ER 建模本就只看结构。
- **回程分层**（不变）：正向（元数据 → ER 图）第一期就做；导出 DDL 可先做；**写回源库后置**（涉及凭据 / 权限，与 mock「永不回写源库」纪律对齐）。
- **原生渲染的成本说明**：等于自研一个**小型图编辑器**（实体卡片 + 关系连线 + 布局 + pan/zoom），比 webview 轨集成工程量更大；但没有 webview 的坑，且与 DBeaver / DataGrip 同构，是桌面工具的行业主流路线。「组件选型不手搓」约束适用的是**组件层**（按钮 / 列表仍用 gpui-kit），图视图是宿主面板级能力，二者不冲突。

**候选**（2026-10-14 更新；许可均**未核实**，按用户口径**不阻塞**）：

| 候选 | 形态 | 结论 |
| --- | --- | --- |
| drawdb / erd-editor / Mermaid | web（React / TS / SVG） | ❌ **已否决**：全部需要 webview，用户定调不引入前端栈 |
| **petgraph** | Rust 图库 | ✅ 图结构 + 遍历（关系建模） |
| **自研 ER 分层布局** | Rust | ✅ 按外键引用分层排布，~数百行 |
| **GPUI 原生绘制**（Path / Text / 事件循环） | Rust | ✅ 视图层：实体卡片 + 折线 + crow's foot + pan/zoom |
| graphviz 布局（可选） | C 库绑定 | ⚠️ 布局可借用 `dot`/`neato`，但引入 C 依赖，非必需 |

**附：gpui-kit shell 自带 QuickJS 插件系统**（2026-10-14 本地索引核实）：

`gpui-kit` 的 `shell` crate 有完整插件机制：`PluginManifest`（id / name / version / shell_version / entry / dependencies / capabilities）+ `PluginManager`（discover / load / unload）+ 能力门控（fs / network / storage / clipboard / process，http 精确到 scheme / host / port / methods / paths）。运行时是 **QuickJS**（`engine/quickjs/`），插件 entry 是脚本，在 `Policy` 下执行，构建的是 **GPUI 原生元素树**（`ScriptView` → `materialize`），**不是 webview**。

含义：① RdataStation 扩展宿主（D6：QuickJS + VS Code 模式）与 gpui-kit shell **同一家族**，可作设计参考与未来对齐对象；但 shell 插件系统依附其 shell 框架（窗口 / 主题 / 布局全托管），替换现有 `crates/plugin`（P1 完成度高）等于重写，**不建议**。② 该脚本元素模型是「宿主注册组件 + 脚本组合」，未见 canvas / path 自由绘制元素 → **不适合画 ER**，ER v1 仍走原生 Rust panel。

**落地顺序**：① schema → 渲染模型桥（`meta.*` → 表 / 列 / 外键 → 布局输入）+ 分层布局 —— **已落地（2026-10-15，`crates/database/src/er/`）**；② 只读渲染（实体卡片 + 连线 + 自动布局）双接入面板；③ pan/zoom + 拖拽排布 + 布局持久化；④ DDL 导出；⑤ 写回源库后置。②~⑤ 待做；全文设计见 [`../database/database-er-model-view-design.md`](../database/database-er-model-view-design.md)。

---

## 12. 这个方式是否合适

### 12.1 结论

**合适，但只能借骨架。** 具体来说：

| VS Code 的哪一条 | 对 RdataStation 是否合适 | 判断 |
| --- | --- | --- |
| **声明式 `contributes`** | ✅ 合适，必须抄 | 启动时零成本登记；你们已有 manifest 与三张待建注册表 |
| **`activationEvents` 懒激活** | ✅ 合适，必须抄 | 启动性能的结构性保证；不激活不加载代码 |
| **独立扩展宿主进程** | ✅ **合适，且是前提** | 崩溃隔离 + 不阻塞 UI + API 可独立演进。你们已有 sidecar 基建（进程/协议/健康检查） |
| **扩展禁止任意 UI** | ✅ 合适 | 与「`render` 纯读路径」一致，且继续守住零裸尺寸/零裸色值 |
| **`engines` 版本闸** | ✅ 合适 | 但必须真的拒绝（现 `check_engine_compatibility` 未被强制） |
| **发布者信任 + 市场签名** | ✅ 合适 | dev-plan P5 已规划 Ed25519 + 逐资源 SHA-256 |
| **扩展与宿主同权** | ❌ **不合适**（应更严） | 我们的 `rds.*` 可精确门控 → 比 VS Code 更安全，**不要退回同权** |
| **300+ 成员的 API 面** | ❌ 不合适 | 十年积累；只做 §4.2 的 8 个命名空间 + `proposed` 机制 |
| **in-process Node/Electron** | ❌ 不合适 | 见 Q7；这是本原型最大的成本项 |
| Webview 作为唯一自定义 UI 出口 | ✅ 可用，仍有边界 | 见 Q8 与 §11.2：`gpui-wry` 已发布，但**只限全屏面板 / 独立窗口**；混排会踩 z-order |

### 12.2 三个必须说清的风险

| 风险 | 说明 | 缓解 |
| --- | --- | --- |
| **JS 运行时的体积与分发** | VS Code 靠 Electron 白拿 Node；我们没有 | **已定（Q7 = QuickJS）**；「重计算走 DuckDB」「不兼容 npm」已写进 §4.1 原则 6 与 §10 |
| **`rds` API 一旦发布就不能破坏** | VS Code 的稳定性承诺是生态成立的前提 | 从 v1 起就按「只增不改」设计；破坏性变更走新的 `rds/v2` 命名空间（对照 Zed 的 `since_v*` 目录冻结） |
| **两条 UI 路线产生两套视觉语言** | 若同时开放 gpui-shell 与 Webview | 只允许一条作为「自定义界面」出口（Q8），且必须显式声明能力 |

### 12.3 一个顺带的重要结论：**wasm 轨的必要性下降**

按 VS Code 模型上了 JS 扩展宿主之后，原先给 wasm 轨准备的场景（轻量分析、规则、生成器）大多被 JS 吸收，而且 JS 在这些场景更好（无需编译、生态好、作者多）。wasm 真正剩下的独有价值只有两条：

1. **无 IO 能力 + 可 fuel/epoch 限额**——比扩展宿主进程更硬的沙箱；
2. **同一份制品离线可验**（无依赖解析）。

**建议（待 Q10 拍板）**：把 wasm 轨从「三轨之一」降为**可选性能轨**，并**冻结 `crates/plugin/src/wasm/` 的现状**（不自建、不扩建），等真有需求再启。这样 M9 的范围会明显收窄——**这是 VS Code 模式带来的实际简化**。

---

## 13. 与 dev-plan 的关系（落地顺序）

本原型**不改变** dev-plan §5 的阶段顺序，只把 P4 的内容具体化：

| dev-plan 阶段 | 本原型补充的内容 |
| --- | --- |
| P0 地基 | 无变化（权限骨架、paths、清单字段） |
| P1 sidecar 端到端 | 无变化（驱动进程与扩展宿主互不相干） |
| P2 / P2.5 | 无变化（元数据与 Arrow 直灌） |
| **P4 注册表 + 引用 + 网格** | **本原型 §4（`rds` API 面）与 §5（激活/生命周期）在此落地**：三张注册表 + 扩展宿主进程 + `activate/deactivate` |
| P5 分发与签名 | 补：`contributes` 的 schema 校验、发布者信任、`engines.rds` 强制 |

---

## 14. 四种插件场景示例（2026-10-14）

> 本节把「插件能长成什么样」落到四个具体场景：① **JDBC 驱动插件**（外接 Java 环境）；② **LSP + Python + Markdown**（外接 Python 环境 + 渲染）；③ **ER 建模视图**；④ **gpui-kit shell 插件系统**示例。
> 前三项落在本仓 `crates/plugin`（sidecar 轨，P1 完成度高）；第 ④ 项是 gpui-kit 上游 shell 能力的单独示例，**不替代** `crates/plugin`（§11.3 附注已述，本节给出实现逻辑）。

### 14.1 JDBC 驱动插件（`contributes.drivers` + sidecar JVM）

**形态**：一个 `contributes.drivers`（§3 清单已有）声明的外接驱动；进程模型 = sidecar（`backend.kind = sidecar`），sidecar 内部**自嵌 JVM**（Java + JDBC jar），对外仍走既有分帧协议。

**为什么值得做**：Oracle / MySQL / PostgreSQL 三家的**官方 JDBC 驱动是一等公民**（协议稳定、文档全、版本跟随库端发布），而 Rust 侧原生驱动生态参差（尤其 Oracle 无官方 Rust 驱动）。用 JDBC 桥接，一个 sidecar 就能覆盖三家，且与「驱动插件不在扩展宿主、凭据只进 sidecar」（§2 硬约束 1/2）完全吻合。

**定位（2026-10-14 用户口径）**：JDBC 是**保底（兜底）方案，不是主路径**。第一梯队 = 既有 Rust 原生驱动（P1/P2 已落地）；JDBC 只在原生覆盖不足时兜底（典型：Oracle 无官方 Rust 驱动、小众库长尾）。→ 服务于「多驱动策略」的长尾覆盖；按「兜底」取舍（体积 / 常驻 / 验收口径都不必与原生驱动同级卷）。

**与既有基建的接点**（全部现成，不新增协议）：

| 基建 | 复用方式 |
| --- | --- |
| `backend.kind = sidecar` + `executable`（manifest.rs 已实现） | 声明可执行文件；四道闸（形态→传输→平台→路径）解析 |
| `sidecar/supervisor.rs` / `process.rs` 生命周期 | 进程池、Concurrency、健康检查、热重载、无孤儿退出全复用 |
| 分帧协议（`BackendTransport::Framed`，4B 大端整帧长 + 1B 种类） | **宿主↔sidecar 协议不变**；JDBC 是它内部的一种实现 |
| `DriverCapabilities`（`[capabilities.driver]`） | 能力门控：`driver.transaction` / `cancel` / `cursor` / `arrow` 真门控在 engine 侧 |
| `meta.*` 五件套形状（sidecar/meta.rs） | `meta.catalogs/schemas/objects/object_detail/routine_source` 解析器现成；JDBC 侧按同一形状实现即可 |
| `tests/sidecar_conformance.rs` | **跨语言一致性验收现成参照**：`RDS_SIDECAR_BIN` / `RDS_SIDECAR_DRIVER` 已支持注入外部二进制 → 真 JDBC sidecar 直接跑同一套「导航面答得出来且形状对」 |

**JDBC 特有的工作**（这才是要写的新代码）：

| 项 | 说明 |
| --- | --- |
| **Java 环境探测** | 启动时探测 `JAVA_HOME` / PATH 上的 `java`，解析版本；找不到给可读错误（对照 dev-plan §4.2.2 错误码口径） |
| **jar 管理** | JDBC 驱动 jar（ojdbc / mysql-connector / pgjdbc）随插件分发在 plugin 目录；sidecar 拼 classpath 启动；版本与库端兼容性（Oracle 8~21c、PG 各小版本）由插件自己负责 |
| **凭据只进 sidecar** | 连接参数经宿主转发但**不落宿主日志**；驱动进程持有（§2 约束 2 的延续） |
| **连接池与并发** | 与既有驱动同口径：进程池规格、Concurrency 由 supervisor 管；JVM 内可跑连接池 |

**一致性验收**（呼应 P1 遗留「跨语言 Arrow 互通」）：`sidecar_conformance.rs` 验证的是**协议面**（初始化握手、meta 导航、Arrow 批量、取消、stdin 关闭退出），与实现语言无关 → JDBC sidecar 通过同一套 conformance = 协议一致性达成，不另造验收。

**需拍板（未定，附建议）**：
- JVM 启动成本 vs 常驻：**建议常驻 JVM**——进程池给 JDBC 单独一档（如 Concurrency=1），进程内跑连接池复用；兜底路径不该每次承担数百 ms 启动；
- jar 分发：**建议随插件分发 + manifest checksum 完整性**（签名并入 P5 整体做）；体积（ojdbc 数 MB）后续可做「首次用才拉取、缓存到 data_home」，首版不阻塞；
- Java 版本 / JRE 形态：**建议依赖系统 Java（探测 JAVA_HOME/PATH），不自带 JRE**——兜底方案不值得携带数百 MB JRE；最低 **Java 11**（主流 JDBC 驱动普遍兼容 11+），探测失败给可读错误；jlink 精简 JRE 仅在有离线 / 企业诉求时再考虑；
- 许可：**按用户口径标注即可、不阻塞**（ojdbc / mysql-connector-j / pgjdbc 具体条款留待插件正式打包时核实，此处不承诺细节）。

### 14.2 LSP + Python + Markdown 插件（编辑器的通用模式 / 分析模式）

**场景拆两半**：① **语言服务**——外接 Python 环境跑 LSP server，给 SQL 编辑器（M4 SQL 编辑器所在 feature crate）提供诊断 / 补全 / 跳转；② **渲染**——Markdown 渲染进预览 / 文档视图。

**关键接缝判断：LSP 与本仓分帧协议是两回事**。

| 协议 | 面向 | 传输 | 关系 |
| --- | --- | --- | --- |
| **LSP** | 语言服务（编辑体验） | 行业标准 **JSON-RPC 2.0 over stdio** | 与 D5 自定义二进制分帧**不同协议** |
| **D5 分帧**（`BackendTransport::Framed`） | 数据库驱动 | 4B 整帧长 + 1B 种类 + 内容 | 面向驱动的自有协议 |

**gpui-kit 编辑器已自带 LSP 扩展接口（本地 MCP 索引核实，0.7.1）**：

| 事实 | 位置（0.7.1） |
| --- | --- |
| 编辑器持有 `Lsp` 配置：`EditorState.lsp()` / `lsp_mut()` | `crates/base/src/input/editor/mod.rs` L199-206 |
| `Lsp` 结构体 = 六个 `Rc<dyn Provider>` **trait 对象**（宿主实现逻辑） | `crates/base/src/input/editor/lsp/mod.rs` L39-70 |
| 六个 provider：`CompletionProvider` / `CodeActionProvider` / `HoverProvider` / `DefinitionProvider` / `DocumentColorProvider` / `DocumentRangeSemanticTokensProvider` | `lsp/completions.rs` 等（数据模型直接用 `lsp-types`） |
| 诊断展示模型：`Diagnostic` / `DiagnosticEntry` / `DiagnosticSet` / `DiagnosticSeverity` | `crates/base/src/input/editor/diagnostics.rs` |
| 宿主接法官方范例：`ExampleLspStore`（内存 store + 模拟 lint） | `examples/editor/src/main.rs` |

> ⚠️ 结论：gpui-kit 编辑器自带的是 **LSP provider 接口 + `lsp-types` 数据模型**，**不含 LSP 客户端**（无 JSON-RPC / stdio / initialize / publishDiagnostics 推送）。数据源（真 LSP server 或本地 lint）与 bridge 逻辑由**宿主**提供——`ExampleLspStore` 就是宿主自己起逻辑、按 `lsp-types` 形状回填的示范。

两种接法（**建议选 a**）：

| 方案 | 做法 | 判断 |
| --- | --- | --- |
| **(a) 宿主实现 provider bridge 连 LSP server**（推荐） | **复用 gpui-kit 编辑器接口**（`EditorState.lsp()` 注入 `Lsp`，宿主实现六个 provider trait）；宿主内部起 Python LSP server 走 JSON-RPC 2.0 over stdio，结果按 `lsp-types` 形状回填；诊断由宿主收（publishDiagnostics 或本地 lint）→ 推入 editor `DiagnosticSet`；插件只负责声明 + 环境探测 | ✅ 编辑器侧零改动（接口现成）；只写一个 client adapter；LSP 是行业标准，复用而非自研 |
| **(b) 插件 sidecar 内做适配器** | sidecar 内翻译 LSP ↔ 分帧 | ❌ 多一层、双协议维护，除非要严格走插件隔离边界才考虑 |

> 判断依据：LSP 与 D5 分帧**服务对象不同**（语言服务 vs 数据库驱动），不必强行统一；本仓 `crates/plugin` 侧（grep 核实）目前**无任何 LSP 基建**（真 client / JSON-RPC 均无），但**gpui-kit 编辑器侧接口现成**——因此不是从零，宿主只需补 client adapter 这一块。文档先定方向、代码 P4 再写。

**Python 环境探测与进程形态**：manifest 的 `backend.kind = script` **已实现**（manifest.rs）：`interpreter = "python3"`（按 PATH 解析）+ `executable = 相对插件目录的脚本` → 由 `PluginBackend::command` 解析成 `python3 <script>`，走既有子进程池（`Sidecar` 同款生命周期）。→ **LSP 的 server 进程可声明为 `script` 形态**，不新增后端形态。

**两种模式定义**（用户此前未定义，给出合理定义）：

| 模式 | 内容 | 依赖 |
| --- | --- | --- |
| **通用模式**（编辑体验） | LSP 诊断 / 补全 / 跳转 / 悬停；Markdown 语法高亮与**预览渲染** | LSP server + Markdown 渲染 |
| **分析模式**（数据工作流） | 文档嵌**可复跑 SQL 块**（`rds.results` 联动，查询结果可回填）；分析结论与数据沉淀进文档；结果渲染（表格 / 图表占位） | 通用模式 + 结果区 / 草稿箱联动 |

**Markdown 渲染挂钩**：gpui-kit `TextView`（`crates/base/src/text/text_view.rs`）是文本查看器（字段 `format/text/state/text_view_style/…`）；`TextViewPlugin` trait（`fn setup(self, text_view: TextView) -> TextView`）+ `TextView::plugin(P)` 是它的极简扩展点；0.7.1 测试 `plugin_accepts_text_view_plugins_beyond_markdown` 佐证「markdown 及更广」可扩展。→ **Markdown 预览走 TextView 插件机制**。

> ⚠️ 措辞区分：gpui-kit 的 `TextViewPlugin`（`crates/base`，渲染扩展）与本仓 `crates/plugin`（sidecar 插件系统）**是两回事**，不要混用术语。

**需拍板（未定，附建议）**：
- 接法：**定 (a) 宿主 provider bridge**（复用编辑器接口，宿主写 client adapter）；
- LSP server 选型：**建议「按语言声明 + 探测」机制，不锁单一 server**；Python 环境内 v1 先接成熟可跑的（jedi-language-server / pylsp，服务 Python + Markdown）；**SQL 诊断 v1 用本地 lint（如 sqlfluff）作「通用模式」最小可用集**——SQL 专用 LSP（sqls / sqlls / sql-language-server）非 Python，留待后置（具体清单正式做时再核实）；
- 模式切换：**建议不显式切换**——同一视图按内容自动判定（分析模式 = 通用模式超集），至多顶栏一个只读 mode 指示器；
- Markdown 渲染粒度：**建议 v1 走 TextView 插件做轻量预览**（语法高亮 + 代码块）；富文档（表格 / 图表 / 可复跑结果区）归入分析模式、P4 后置，与 ER（§14.3 原生面板）保持同轨判断。

**代码落地（五刀已完工，`crates/editor/src/lsp/` + `crates/workbench/src/services/editor_lsp.rs`）**：

| 刀 | 内容 | 位置 | 状态 |
| --- | --- | --- | --- |
| 一 | 分帧（`Content-Length` 头帧编解码，纯函数）+ 最小 client（握手 / 请求-响应关联 / `didOpen·didChange` / `publishDiagnostics`→事件 / 停机无孤儿） | `frame.rs` / `client.rs` | ✅（9 + 5 项测试） |
| 二 | provider bridge：`LspCompletionProvider`（`completions()` 后台 `call` 转发）+ 诊断翻译纯函数 + `install/uninstall` 挂点 | `bridge.rs` | ✅（8 项测试） |
| 三 | 宿主装配：`LspPort` 端口（宿主注入「文档 → 服务」+ 按 `uri` 认领事件）+ `choose_completion` 三选一（SQL 本地 / 通用分析 LSP / 无）+ `didOpen/didChange/didClose` 全文同步 + 事件轮询并入三源诊断（执行 / 词法 / LSP）+ `IoError` 降级摘服务 | `host.rs` + `view/host.rs` | ✅（6 + 1 项测试） |
| 四 | **宿主起真 server**：`client.rs` 加 `from_process`（`ProcessIo` 拆读写两半）；workbench `editor_lsp.rs` 探测 Python 环境 → spawn `python -m jedi_language_server` → 后台 `initialize`（不阻塞主线程）→ `LspPort` 实现（反查文档模式/路径定语言，`service()` 就绪才给）+ 事件按 `uri` 认领 / `IoError` 广播；装配点 `WorkbenchView::assemble` | `client.rs` + `editor_lsp.rs` + `view.rs::assemble` | ✅（5 项测试 + 616/616 + 147/147） |
| 五 | **状态栏 LSP 管理器 + 生命周期**：`LspPort::wants`（「想要 ≠ 就绪」——被停 / 崩溃后保持轻轮询，**重启即自动重连**）；面板泵每拍重问宿主 + 崩溃清僵尸会话（`same` 判据并上 `Arc::ptr_eq`）；`WorkbenchLsp` 改**按需启动 + 空闲回收（`reap_if_idle`）+ 手动启停 + 崩溃探测（`mark_dead`）**；状态栏指示 + 管理器浮层（启动 / 停止）+ 1 s 回收泵 | `lsp/host.rs` / `shared.rs` / `view/host.rs` + `editor_lsp.rs` + `view.rs` / `ui.rs` | ✅（新增纯函数测试 + 全量回归） |

**遗留（未定，后续待做）**：
- **server 生命周期**：已落地「**关最后一个需要服务的面板 → 空闲回收停 server**」（视图 1 s 泵 `reap_if_idle`，只停服务、下次需要自动重启）+ **状态栏 LSP 管理器手动启停**（`start` / `stop`）+ **崩溃探测**（读线程断 / 握手失败 → `mark_dead`：广播死讯、摘句柄、待手动重启，避免崩溃热循环）。**仍留**：`shutdown()` 未挂应用退出路径（v1 靠 `LspClient::Drop` 关读线程 + 写侧 EOF 兜底）；生命周期默认策略（按需启动 / 空闲 1 s 回收 / 崩溃不自动重启）可后续做成可配项；
- **多语言 server 声明**：v1 硬编码 `python -m jedi_language_server` 只服务 Python；设计目标是「按语言声明 + 探测」多 server（jedi / pylsp / 未来 markdown server），需等插件 manifest 的 `backend.kind = script` 落地后把命令来源换成插件声明；
- **多面板共享**：`drain_for` 按 `uri` 认领已保证事件不丢，但尚无窗口级集成测试（editor crate 无窗口测试基建，纯 `#[test]` 覆盖到端口契约与折叠逻辑）；
- **UTF-16 编码**：跟随内核 char 数编码（bridge 注释已述），与 LSP 规范在非 BMP 字符上有已知偏差，暂不另造转换；
- **其余 provider 槽位**：hover / definition / codeAction / semantic tokens 未实现（`Lsp` 六个槽位只接了 completion + 诊断）；Markdown 的 LSP 未接（v1 只 Python，`.md` 文档无服务）。

### 14.3 ER 建模视图（浓缩，详见 §11.3）

**结论**（§11.3 已定案，这里只引用）：以插件形式进入**合适**；webview 轨**否决**（drawdb / erd-editor / Mermaid 均需 webview，本项目由 tauri 迁来、不引入前端栈）；路线 = **GPUI 原生渲染**，插件退到「数据源 + 视图声明」角色。

**数据流**（一句话链路）：

```text
meta.* 五件套（catalogs/schemas/objects/object_detail/routine_source）
  → 宿主 MetadataBrowser（已存在，P2）
  → ER 输入模型（表 / 列 / 主外键，仅结构，不碰凭据——硬约束 #1）
  → 分层布局（petgraph 或自研 ~数百行，按外键引用分层）
  → GPUI 原生绘制（Path / Text：实体卡片 + 折线 + crow's foot + pan/zoom）
```

**插件角色**：`contributes.views` 声明 + 数据变换（`rds.metadata` → ER 输入模型）；v1 只读渲染甚至可不依赖插件系统（宿主直取），插件是**加分项不是前提**（§11.3）。落地顺序五步见 §11.3。

**落地（2026-10-15）**：第一期骨架已落地——ER 输入模型 + 「元数据 → 模型」桥 + 分层布局（纯函数，带单测，`crates/database/src/er/{model.rs,layout.rs}`，`cargo test -p rds-database --lib` 133/133 通过）；渲染与面板装配为下一片。设计全文（含模块归属判定、桥规则、布局算法、开放项、实现位置映射表）见 [`../database/database-er-model-view-design.md`](../database/database-er-model-view-design.md)。

### 14.4 gpui-kit shell 插件系统示例（查询结果看板 / 仪表盘卡片）

**场景选型**：shell 插件 = 宿主注册组件 + QuickJS 脚本**组合元素树**（无 canvas / path 自由绘制）→ 适合「**查询结果数据看板**」：脚本用 Button / List / Table 组合出卡片，订阅 rds 事件刷新。**不适合画 ER**（§11.3 附注已述，ER 走 §14.3 原生面板）。

**实现逻辑**（锚定 0.7.1 API，本地索引核实）：

| 步骤 | 0.7.1 API | 说明 |
| --- | --- | --- |
| 1 | `PluginManifest::parse/read` | `id/name/version/shell_version/entry/dependencies/capabilities` |
| 2 | `PluginManager::new`（或 `with_data_home`）→ `discover()` → `available()/loaded()` | 字段 `data_home` + `catalog` + `discovered`；`directories()` |
| 3 | 能力门控 `CapabilitiesFile::default/grant/validate_placeholders/validate_network` | http 精确到 scheme / host / port / methods / paths |
| 4 | `ShellRuntime::load(root, window, cx) -> Entity<ShellRoot>`（或 `try_load` / `check`） | 挂进窗口 |
| 5 | QuickJS 脚本在 `Policy` 下执行 | 构建 **GPUI 原生元素树**，非 webview |
| 6 | `ScriptView::render` | 主题同步 → dirty 时 `rebuild` → `materialize(&self.runtime, snapshot, window, cx)`；**构建失败保留最后好快照** + `error_banner`；从未成功 → `error_overlay` |
| 7 | 宿主订阅 rds 事件刷新看板 | 查询结果变更 → 重新渲染 |

**集成边界（重要）**：

| 点 | 结论 |
| --- | --- |
| 门面 crate `kit` 只 re-export `open_window` / `init`，**不聚合 shell** | 要用 shell 需以 **path 直取 `gpui-kit/crates/shell`**（0.7.1）或 vendor |
| shell 依附其 shell 框架（窗口 / 主题 / 布局全托管） | 替换本仓 `crates/plugin`（P1 完成度高）等于重写，**不替代**；只作设计参考与未来对齐对象 |
| 三版本（0.7.1 / 0.7.2 / 0.7.3）shell 都在 | 本仓 lock 是 **0.7.1**，引用按 0.7.1 |
| 0.7.3 升级 | **暂不升**（结论不变），原因见 §10 与既有口径 |

**需拍板（未定，附建议）**：**建议暂不做**（无近期真实诉求时）——保留为设计参考；若做，先做**独立宿主面板**（复用现有布局 / 组件，成本低），真 shell 插件系统仅作未来对齐对象、不进 P4。
