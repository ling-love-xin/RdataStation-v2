# 插件系统（M9）· 使用手册

> **读者**：① 使用者（装 / 启 / 授权 / 排障）② 插件作者（从零写一个插件）。
> 设计理念见 `plugin-prototype-design.md`；任务与排期见 `plugin-dev-plan.md`；模块入口见 `README.md`。
>
> ⚠️ **时效声明（2026-09-20）**：插件系统**尚未落地**（P0 地基刚完成，P1 未开始）。本仓现状是“契约与目录就位 + 其余仍是骨架”（`crates/plugin` 约 4395 行；P0 已删掉三个不参与编译/零调用的文件，见 `plugin-dev-plan.md` §1.2）。
> 所以**本手册描述的是目标形态，现在还不能照着操作** —— 它同时是给实现的验收口径：下面的每一步，实现完成后都应能原样走通。

---

## 第一部分 · 使用者

### 1.1 入口

| 入口 | 位置 | 行为 |
| --- | --- | --- |
| 打开插件面板 | 左活动栏第 4 个图标（插件） | 左 Dock 展开为「插件」面板 |
| 命令面板 | `Ctrl+Shift+P`，输入命令标题 | 执行插件贡献的命令（命令面板里能看到 ≠ 插件代码已加载） |
| Quick Open | `Ctrl+P`，输入 `>` 前缀 | 同上（只匹配命令） |
| 插件详情 | 面板里点任一条目 | 中央区打开「插件 · <名称>」tab |
| 从磁盘安装 | 面板底部按钮 | 选目录或 `.rdsx` 包，校验签名后安装 |

### 1.2 界面导览

```
┌ 左侧 Dock（240px）──────────────────────────────┐
│ 插件                    [⟳] [⋯]                │  ← 面板头
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

中央「插件详情」页分四区：
  基本信息 → 贡献点 → 权限（扩展轨 / 驱动轨两张表）→ 项目引用 → 运行状态
```

状态栏右侧显示扩展宿主状态（内存 / 已激活插件数）；它**可点**，用于「重启扩展宿主」与查看插件日志。

### 1.3 典型流程

**装一个插件（从磁盘）**
1. 面板底部「从磁盘安装…」→ 选目录或 `.rdsx`。
2. 宿主读 `plugin.toml`：校验 `schema_version` / `engines.rds`（不满足**当场拒绝**并报出两个版本号）。
3. 校验签名：Ed25519 + 逐资源 SHA-256。发布者未验证时会**显式标注**（不是只靠颜色）。
4. 列出它申请的**每一项**权限与用途 → 你确认。
5. 安装位置：`<RDS_HOME>/plugins/<publisher>.<name>/`。

**启用 / 禁用**
- 禁用会从宿主 UI 上撤下它的贡献点（视图、菜单项、命令）。
- **若当前项目引用了它贡献的面板**，面板不会消失，而是显示占位：「本项目引用了 X，未启用」+ 两个出路（重新启用 / 从项目移除引用）。

**授权与撤销**
- 权限可在插件详情里**逐项**授予/撤销（扩展轨与驱动轨分开）。
- 撤销后相关功能以 `-32006 capability_denied` 失败，并给出可读原因 —— **不会静默失效**。

**卸载**
- 卸载后同样走引用对账：插件不在了，但项目引用还在 → 列表里保留在「项目引用 · 未安装」分组，给出「重新安装 / 移除引用」。

### 1.4 FAQ

**Q：命令面板里能看到插件命令，但点了没反应？**
A：命令是**启动时静态登记**的（零成本），代码是**首次激活时懒加载**的。第一次点击会先加载模块并跑 `activate()`，可能有一两百毫秒延迟；失败会弹错误而不是无声。看日志：状态栏右侧 → 插件日志。

**Q：为什么某个面板显示「未安装 / 未启用」而不是空白？**
A：这是**故意的**（项目引用对账）。空白的含义是"这个功能本来就没有"，而占位的含义是"有，但当前不可用"。两者不能混。

**Q：为什么插件功能和产品其余部分配色一致？它不是第三方做的吗？**
A：插件的面板数据由宿主用自己的控件渲染；走 webview 的图表也必须从宿主取主题角色值。所以配色/尺寸天然一致，插件也无法做出"另一个产品"的观感。

**Q：webview 面板能不能一边看图表一边弹宿主的对话框？**
A：不能。webview 是原生子窗口，层级不受 GPUI 控制，会盖住宿主的 dialog/overlay。因此 webview 只给两种承载：**全屏面板**或**独立窗口** —— 后者反而是最舒服的用法（可以拖到第二块屏）。

**Q：插件崩了会怎样？**
A：三种情况分开算：① 某次命令抛异常 → 只有那次调用失败；② 扩展宿主进程崩溃 → 所有扩展失活，宿主 UI 照常，可一键重启；③ 驱动进程崩溃 → 只有那个连接失效，如实报错（不静默重连），可手动重启该驱动。

---

## 第二部分 · 插件作者

### 2.1 最小插件

```
my-plugin/
├── plugin.toml          # 清单（唯一契约）
├── dist/extension.js    # 入口（懒加载；QuickJS，不是 Node）
└── media/icon.svg
```

```toml
schema_version = 1

[extension]
id           = "example.table-board"
version      = "1.0.0"
display_name = "表大小看板"
description  = "把库里最大的 N 张表列成面板"
publisher    = "example"
license      = "MIT"
engines.rds  = "^0.2"          # 必填，不允许 "*"

[main]
module     = "dist/extension.js"
activation = ["onView:example.tableBoard.explorer", "onCommand:example.tableBoard.refresh"]

[[contributes.commands]]
command = "example.tableBoard.refresh"
title   = "重算表大小"

[[contributes.views]]
id       = "example.tableBoard.explorer"
name     = "表大小看板"
location = "left"

[[contributes.configuration]]
key     = "example.tableBoard.topN"
type    = "number"
default = 20
scope   = "project"

[capabilities]
"rds.metadata"   = ["*"]
"rds.views"      = true
```

```js
// dist/extension.js —— 注意：没有 DOM、没有样式、没有像素
function activate(context) {
  context.subscriptions.push(
    rds.commands.registerCommand('example.tableBoard.refresh', async () => {
      const topN = rds.workspace.getConfiguration('example.tableBoard').get('topN', 20);
      const conn = rds.connections.getActive();
      if (!conn) return rds.window.showInformationMessage('请先连接一个数据源');

      const tables = await rds.metadata.getTables(conn, undefined);
      await rds.views.update('example.tableBoard.explorer', {
        title: `Top ${topN} 表`,
        items: tables.slice(0, topN).map(t => ({ id: t.name, label: t.name, description: t.rowCount })),
      });
    })
  );
}
function deactivate() {}   // 全部挂在 context.subscriptions，无需额外清理
```

### 2.2 清单字段速查

| 字段 | 必填 | 说明 |
| --- | --- | --- |
| `schema_version` | ✅ | 清单格式版本 |
| `extension.id` | ✅ | 反向域名式，全局唯一；卸载/升级/引用对账都靠它 |
| `extension.engines.rds` | ✅ | 版本闸，**不可 `*`**；加载与安装两处都强制 |
| `main.module` | ✅ | 扩展宿主入口（懒加载） |
| `main.activation` | ✅ | `onCommand:` / `onConnection:` / `onView:` / `onInsightRule` / `onStartupFinished`（`onStartupFinished` 慎用） |
| `contributes.commands` | — | `command` / `title` / `category` |
| `contributes.menus` | — | 挂载点：`nav/item/context`、`result/grid/context`、`editor/title`、`title/overflow`、`statusBar` |
| `contributes.configuration` | — | `key` / `type` / `default` / `scope`(`application`\|`project`\|`session`) / `label` |
| `contributes.views` | — | `id` / `name` / `location`(`left`\|`right`\|`center`) / `icon` / `when` |
| `contributes.drivers` | — | **声明驱动（会另起 native 进程）**，见 §2.5 |
| `contributes.insightRules` / `mockGenerators` / `exporters` | — | RdataStation 特有 |
| `[capabilities]` | — | 权限声明（默认拒绝） |
| `[dependencies]` | — | 插件间依赖，如 `"example.common-utils" = "^1.0"` |

### 2.3 `rds` API 面（8 个命名空间，就这么多）

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

### 2.4 权限声明（默认拒绝）

```toml
[capabilities]
"rds.connections"            = ["list", "get"]     # 不给 getActive 也行 —— 可精确到方法
"rds.metadata"               = ["*"]
"rds.results.getRows"        = "project-data"      # 需要项目可见的数据
"rds.duckdb.query"           = "read-only"
"rds.duckdb.createTempTable" = true
"rds.workspace.getConfiguration" = true
"fs.write"                   = ["${globalStorage}"] # 只能写自己的存储目录
"net.http"                   = []                   # 扩展宿主默认无网络
```

未声明的调用返回 `-32006 capability_denied`。**申请得越少越好**：详情页里每一项权限都会展示给用户，用户会看到"这个插件为什么要联网"。

### 2.5 什么时候必须做成驱动插件（sidecar）

| 你的需求 | 该走哪一轨 |
| --- | --- |
| 只是编排元数据 / 发通知 / 改状态栏 / 给面板喂数据 | 扩展轨（JS） |
| 需要**真事务**（begin/commit/rollback 跨多语句） | **sidecar** |
| 需要**真取消**（长查询可中断，不是关连接） | **sidecar** |
| 需要**流式游标**（边读边回，结果集不进内存） | **sidecar** |
| 需要**全量元数据浏览**（catalogs/schemas/tables/columns 一次给全） | **sidecar** |
| 需要原生协议（JDBC / 专有 wire protocol / 原生 TLS 连接池） | **sidecar** |
| 纯计算、无 IO、要最硬的沙箱 | wasm 轨（**当前建议冻结**，先别做） |

sidecar 的代价要提前知道：它**自己拿得到凭据**（安装时明示）、要自己管进程健康、它崩了对应连接就失效。

要动手写一个？见 §2.8（协议要点 + 自检命令 + 五条硬性约定）。

### 2.6 调试

| 想看什么 | 怎么看 |
| --- | --- |
| 插件日志 | 状态栏右侧 → 插件日志（`target` 前缀是插件 id） |
| 命令没出现 | 查清单 `contributes.commands` 与 `engines.rds`；静态登记失败会在日志里报原因 |
| 激活失败 | 重启扩展宿主（状态栏右侧）后重跑；`activate()` 抛的异常会进日志 |
| 权限被拒 | 报错里是 `-32006`，`data.required` 写了缺哪一项；在详情页授予或改清单 |
| 面板没数据 | 先确认视图 `id` 与 `rds.views.update` 里的一致；未连接时宿主会显示欢迎态而不是空白 |

### 2.7 发布前检查清单

- [ ] `engines.rds` 填了且不是 `*`
- [ ] 权限最小化：`net.http` / `fs.write` 若无必要就**不申请**
- [ ] 没有在 JS 里做大数组计算（改成 `rds.duckdb.query`）
- [ ] 大结果集没有走 JSON 内联通道（阈值 200 行 / 256 KiB 由宿主判定）
- [ ] `deactivate()` 能干净释放（订阅全挂 `context.subscriptions`）
- [ ] 包已签名（Ed25519 + 逐资源 SHA-256），且逐资源校验通过
- [ ] 面板在**未连接 / 未安装 / 宿主重启**三种降级下都不会让用户看到"空白"或假数据

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

跑通那五条 = P1 的退出标准（能连 → 3000 行 Arrow → 能取消 → 无孤儿进程）。
清单怎么写（`[backend]` + `[[contributes.drivers]]`）见 §2.2 与 `plugin-dev-plan.md` §4.3。

---

## 第三部分 · 验收清单（实现完成后逐条走一遍）

- [ ] 装一个本地插件：签名校验通过 → 列权限 → 装完命令出现在命令面板
- [ ] 断网环境下装未签名插件：**拒绝安装**并说明哪一项校验失败
- [ ] `engines.rds` 不匹配：安装期拒绝，报出两个版本号
- [ ] 删掉某个权限：该功能报 `-32006` 并给出可读原因（不是静默）
- [ ] 禁用被项目引用的插件：面板显示占位 + 两个出路（不是空白、不是崩）
- [ ] 卸载插件：进入「项目引用 · 未安装」分组，可重新安装或移除引用
- [ ] 杀掉扩展宿主：全部插件失活，宿主 UI 与已打开结果集不受影响，可一键重启
- [ ] 杀掉驱动进程：只有该连接失效，如实报错，可重启
- [ ] webview 面板：全屏面板与独立窗口都能开；**不与 GPUI 元素混排**
- [ ] 主题切明暗：插件面板配色随宿主变化
- [ ] 长查询：点取消能真中断（`-32004`），宿主不卡
