# 插件系统（M9）· 模块入口

> **从哪开始读**：先看下面 §2 的三份文档，再看 §1 的一句话形态。代码地图与硬约束在 §4 / §5。
> 本模块的视觉稿在 `prototype/`（五张，覆盖每种承载），自查命令见 §6。

---

## 1. 一句话形态

> **宿主 = RdataStation 的 GPUI 窗口；扩展 = 独立「扩展宿主进程」里的 JS/TS 模块；驱动 = 独立 sidecar 进程（native）。扩展不碰渲染路径，只通过 `rds.*` RPC 请求宿主改宿主自己的 UI 模型。**

三条硬约束（是设计的一部分，不是建议）：

| # | 约束 | 理由 |
| --- | --- | --- |
| 1 | 驱动插件**不在**扩展宿主里 | 驱动要原生 socket/TLS/连接池；一个驱动崩溃不该拖垮所有扩展 |
| 2 | 扩展宿主**不直接访问数据库** | 只能反向请求宿主 → **凭据不出宿主进程** |
| 3 | 扩展代码**不进入渲染路径** | 与「`render` 是纯读路径」一致，也让「零裸尺寸 / 零裸色值」两条契约继续可守 |

## 2. 文档地图

| 文档 | 回答什么 | 什么时候读 |
| --- | --- | --- |
| `plugin-architecture.md` | **设计意图**：最初为什么这么想 | 想知道"为什么有 sidecar / wasm 两轨"时 |
| `plugin-dev-plan.md` | **做什么、做到哪**：现状盘点 / 决策 D1–D6 / 与既有设计的优劣势 / 阶段 P0–P5 / 验收 / 风险 | **动手前必读**；改动清单在 §5.1 |
| `plugin-prototype-design.md` | **长什么样**：进程与信任边界 / 清单字段 / `rds` API 面 / 激活时机 / 贡献点映射 / 示例插件 / Q7–Q10 定案依据 | 写插件、写宿主侧注册表、评审形态时 |
| `plugin-user-guide.md` | **怎么用**：面向使用者的入口与操作、面向作者的从零写一个插件 | 交付/试用/写插件时 |
| `prototype/*.html` | **可交互形态稿**（五张，含插件入口） | 评审界面时；索引见 `prototype/README.md` |

`../ui/`、`../theme/`、`../layout/` 仍是**视觉与尺寸的权威**；本文档集不定义新的视觉原语。

## 3. 已定 / 未定

**已定（可依赖）**

| # | 决策 |
| --- | --- |
| D1 | 三轨：驱动 = sidecar（native 子进程）；面板/命令/设置 = **独立扩展宿主进程**（VS Code 模式）；分析/规则 = wasm（**建议冻结**，见 Q10） |
| D2 | 边界按 trait 画：凡需 `MetadataBrowser` 全量元数据、真事务、真取消、游标的 → sidecar |
| D3 | 一个契约（`PluginManifest`）+ `backend.kind` 分派；不允许两份清单/两套权限语义 |
| D4 | 数据面 Arrow / 控制面 JSON，混在**字节流层**（1 字节 `kind`）；阈值 `inline_json_max_rows=200` / `256 KiB` |
| D5 | stdio + 二进制分帧（不是本地端口；Arrow IPC 是二进制） |
| D6 | 扩展宿主运行时 = 内置 QuickJS（`rquickjs`）；**有意不兼容 npm**；重计算下沉 `rds.duckdb` |
| D7 | **宿主侧接入暂缓**：`PluginService` 现 0 调用方；等插件完成度达标（P1 真机三项 + P2 收尾）后**另行评估**是否接入 workbench；期间不写 P4 接线代码 |
| Q7 | ✅ = D6 |
| Q8 | ✅ webview **可提供，但限全屏面板 / 独立窗口**；与 GPUI 元素混排**实践上不支持**（z-order）；本仓要接就锁 `gpui-wry = "=0.6.1"` |
| — | 错误码表见 `plugin-dev-plan.md` §4.2.2（`-32001`…`-32008`；能力拒绝是 `-32006 capability_denied`） |

**未拍板（别当成已有）**

| # | 待定 | 影响 |
| --- | --- | --- |
| Q1 | 项目引用表：迁移到 `project_resources(kind,id,version,enabled,enforce,config_json)` vs 沿用现有两表 | P0 是否含一次迁移（现无生产调用方，建议迁） |
| Q9 | 键位贡献是否开放 | 命令面板之外的可见性 |
| Q10 | wasm 轨：**已定案**（2026-10-16，方案 A）降为**可选性能轨**：仅保留「一次性渲染 / 导出」实证（`tools/plugin-render-mermaid`，Mermaid 文本 → SVG）；**不再扩建** wasm 轨（冻结口径不变，等真有需求再启） | wasm 轨场景收敛为纯计算 / 导出 |
| Q11 | ER 建模视图渲染路线：**已定案**（2026-10-16，方案 A）webview 轨**已否决**（用户定调不引入前端栈）；主路线 = **GPUI 原生渲染**（`crates/database/src/er/` 已启动）；**Mermaid 降为导出轨**——mermaid-rs-renderer 纯 Rust 可编 wasm（原否决前提「需 webview」已被推翻），仅服务一次性渲染/导出，不承载交互画布 | 交互画布走原生，一次性产出走导出轨；宿主侧接入仍随 D7 暂缓 |

## 4. 代码地图（`crates/plugin/`，现状实测）

| 文件 | 现状 |
| --- | --- |
| `manifest.rs` / `model.rs` / `permission.rs` | 清单与权限骨架（P0 已扩到**四轨**：`Frontend`/`Wasm` 门控 + `Sidecar`/`Driver` 展示轨，见 `PermissionType::is_gating`）；**P1 已落 `[backend]` 段**（`PluginBackend` → `command()` 四道闸 + `process_spec()` 接进程池；口径见 dev-plan §4.3.1）；**P2.6 已落能力声明**：顶层 `api`（能力面命名空间，借鉴 navop driver.json）+ `methods`（sidecar RPC method 白名单，安装期校验，见 `sidecar/method.rs`）；`plugin-prototype-design.md` §3 是它的**增量扩展**，不是第二份契约 |
| `plugin_service.rs` | **项目级 6 方法已实现**；表 `project_used_plugins` / `project_plugin_config` 已建（`migrations/project_meta/001_init.sql:112/121`） |
| `manager.rs` / `installer.rs` / `dependency.rs` | 安装与装载骨架；**生命周期 API 已补全**（2026-10-15）：`deactivate_plugin` Sidecar 分支先停进程、`uninstall_plugin`（磁盘卸载：依赖检查 → 停用 → 删 `install_dir/<id>/` → 清 global.db 记录 → 事件，**保留 plugin-data**）、`enable_plugin` / `disable_plugin`（DB `is_enabled` 为权威状态，运行中禁用立刻停用）、`scan_and_diff_plugins`（热插拔 diff）、`reload_plugins`（VS Code Reload Window 口径）；`load_plugin_from_dir` 同步登记形态索引（热插拔路径此前漏登记，activate 会 not_found） |
| ~~`sidecar/manager.rs`~~（约 290 行） | ✅ **P1 删除**：一 manager 一进程 + 从 stdout 读端口 + `reqwest` 健康检查，与 D5（stdio 分帧）相反；功能已由 `process.rs`（起/收）+ `supervisor.rs`（三层生命周期）取代 |
| ~~`sidecar/client.rs`~~（约 240 行） | ✅ **P1 删除**：HTTP/端口 + 零鉴权，且自带**第二份 JSON-RPC 信封**（`JsonRpcRequest`/`JsonRpcResponse`/`QueryParams`…）—— 正是 §3.5「契约重复两份」的坑；信封现在只有一份（`conn.rs` 的调用 + `driver.rs` 的方法表）。删它同时去掉了 `rds-plugin` 的 `reqwest` 依赖 |
| `sidecar/proto.rs` | ✅ **P1 协议层已落地**：帧（4B 大端长度 + 1B kind，**`total_len` 含头 5 字节**）+ 增量解码器 + `read_frame`/`write_frame`（async，含短读与“先校验再分配”的测试）+ 版本闸 + 内联阈值 + 错误码表；15 条单测。**P3.5 加第三 kind**：`KIND_STREAM = 0x03`（SSH shell/forward 双向字节流，与 Arrow 附件正交）+ `Frame::stream` / `stream_eof`（通道 id 超 `u16::MAX` 构造前拒绝）+ `STREAM_FLAG_DATA` / `STREAM_FLAG_EOF` → 18 条单测 |
| `sidecar/router.rs` | ✅ **P1 附件语义已落地**：`Router`（消费帧 → 事件：在飞登记 / 扣住未收齐的响应 / 错位上报 / 断线交还）+ `encode_response_with_arrow`（sidecar 侧切帧）；两者互为逆运算，有往返测试；15 条单测。**P3 前半加反向请求识别**：带 `id` + `method` 的帧按 JSON-RPC 判据判为 `InboundRequest`（响应帧没有 method；即使 id 撞上在飞也不误判）→ 17 条单测。**P3.5 加流帧路由**：`RouterEvent::StreamData` / `StreamEof` + `StreamPayload` / `decode_stream_payload`（错位 = Issue 不 panic）+ `on_stream_frame`（流帧不碰 pending 附件装配，可随时插队）→ 22 条单测 |
| `sidecar/lifecycle.rs` | ✅ **P1 决策内核已落地**（sans-io，零 I/O）：三层对象模型 `PluginProcess → DriverInstance → Session` 的规则全部在此 —— 进程按 plugin_id 去重、`max_instances`、serial 排队与 `QUEUE_MAX_LEN`、ping 连续 2 次判死、空闲 30min 回收、崩溃**不静默重连**（需手动重启）；排队项**连 driver 一起记**（多 driver 插件放行时才知道该开哪个）；`RejectReason` 有可读文案；17 条单测逐条对应 §4.1 五条规则 |
| `sidecar/conn.rs` | ✅ **P1 异步客户端已落地**：`SidecarConn::spawn(reader, writer)` 起三个任务（调用方 / driver / 读侧）；**在飞状态只有一份**（Router + id→oneshot 都在 driver 任务）；`call` 带超时且超时后显式 `Abandon`（迟到响应会报成 Issue）；`initialize` 内置版本闸；`shutdown` 只发命令（真正的回收靠“丢写侧 → 对端 EOF 自退”）；11 条单测。**P3 前半加反向请求**：`ConnEvent::InboundRequest` + `SidecarConn::reply` / `reply_error`（写 JSON-RPC 响应帧，不登记在飞）→ 13 条单测。**P3.5 加流通道**：`ConnEvent::StreamData` / `StreamEof` + `DriverCommand::WriteStream` + `write_stream` / `write_eof`（下行宿主 → 插件；驱动写帧失败走既有收摊路径）→ 15 条单测 |
| `sidecar/method.rs` | ✅ **P2.6 协议 method 常量表已落地**（借鉴 navop `extension-protocol/method.rs`）：全部 wire method 字符串集中成常量（业务代码零字面量，打错字编译期即挂）+ `is_known` / `is_allowed_declaration`（协议已知或 `x.` 私有）/ `namespace`；`PluginManifest::validate` 用它卡 sidecar 插件声明的 `methods`（安装期拒绝拼错的能力承诺）；6 条单测 |
| `sidecar/host.rs` | ✅ **P3 前半（反向 Host API 面）已落地**（借鉴 navop `extension-protocol/src/host.rs`）：`host.*` 七个方法常量 + `SecretRef` 引用制 + params/result 类型（`request_credential` / `secret.resolve` base64 一次性值 / `notify` / `storage.get|set` / `log` / `ssh.open_tunnel`）+ `HostApiProvider` trait（未实现的方法默认回 `Unsupported`，协议面就绪 ≠ 宿主已接线）+ `HostApiHandler`（方法路由 + **方法级 glob 权限闸**，`prefix.*` 点号并入通配）；15 条单测 |
| `sidecar/host_provider.rs` | ✅ **宿主默认实现**（2026-10-15）：`storage.get/set` 真落盘（每插件 `paths::plugin_data_dir(id)/host-storage/<ns>/<key>.json`，字符集校验防穿越、临时文件+rename 原子写、ttl 过期懒清理）+ `log` 走 tracing（module 作结构化字段）；`request_credential` / `notify` / `ssh.open_tunnel` 保持 `Unsupported`（宿主没有对应设施就不假装有，可读错误优于谜之超时）；6 条单测 |
| `sidecar/runtime.rs` | ✅ **运行时循环（宿主接线主干）**（2026-10-15）：固定拍频驱动 supervisor（`drain_events` → 分发 → `sweep_idle` → `ping_all`，同一次入口一个 `now`）；`InboundRequest` 按 `(plugin_id, index)` 拿连接句柄回帧，权限 = 清单 `permissions.host`（插件未加载 = 空权限 → 默认拒绝），其余事件如实打日志（转 UI 通知是 P4）；4 条单测。**热插拔挂载**（2026-10-15）：每个 tick 先跑 `scan_and_diff_plugins`（新目录自动加载/激活、目录消失自动停用、清单变化置 needs_reload） |
| `sidecar/ssh.rs` | ✅ **正向 SSH 协议方法面**（2026-10-15）：`x.ssh.*` 五方法（connect / exec / shell / forward / close）+ 参数/结果类型（凭据走 SecretRef 引用、exec 一次一答、shell/forward 的通道契约就绪）——SSH sidecar 插件的驱动协议，走 `x.` 私有前缀（安装期声明可过，不污染协议命名空间）；与反向 `host.ssh.open_tunnel`（宿主是引擎）方向互补；10 条单测。**字节流帧第二刀已落地**（2026-10-15）：`KIND_STREAM=0x03` 承载 shell stdout / stdin 与 forward 的持续双向流——`proto.rs` 新 kind + 构造器（通道 id 长度域 u16 BE，超长拒绝）+ `router.rs` 流事件（数据 / EOF，与 Arrow 附件正交、可插队）+ `conn.rs` 流通道（上行事件 / 下行 `write_stream`/`write_eof`）+ `supervisor.rs` 事件泵透传（按代过滤）+ `runtime.rs` 显式分发（debug 只记元信息不记数据本体） |
| `render.rs` | ✅ **渲染计算服务**（2026-10-15，借鉴 dodo `mermaid source → SVG`）：wasm 插件在 `manifest.capabilities.wasm.render_document` 声明渲染函数（`function` 默认 `render`、`formats`、`output_format` 默认 `svg`），宿主 `render_document(manager, plugin_id, source, format)` 把源文本交给插件、按 UTF-8 收产物（SVG/HTML）；`renderer_for` 纯函数按声明顺序选第一个匹配格式的渲染器（`formats` 空 = 任意）；纯计算、不需要 host function；6 条单测。**边界**：真实 wasm 资产（mermaid-rs-renderer 编 wasm）属 P4 真机验收 |
| `sidecar/hot_reload_manager.rs` | ✅ **热重载薄封装**（2026-10-15，原 2 字节空文件）：转发 `reload_plugins` / `scan_and_diff_plugins` / `pending_reloads` 到 `PluginManager`（真实实现都在 manager，本模块只是保留既有引用点的门面） |
| `sidecar/process.rs` | ✅ **P1 进程层已落地**：`SpawnSpec`（程序 / 参数 / 环境 / `current_dir` / 日志，目录一律来自 `paths::*`）+ `SidecarProcess::spawn`（三管道接入 `SidecarConn`，stderr 落 `plugin-cache/<id>/sidecar.log`）+ `retire(grace)`（**先丢连接关 stdin → 对端见 EOF 自退 → 到点强杀**）；id 先过 `validate_plugin_id` 再建目录；4 条单测 |
| `sidecar/supervisor.rs` | ✅ **P1 运行时接线已落地**：`Deployment`（清单 → 进程规格 + 命令）+ `SidecarSupervisor` —— 执行决策内核的动作（起 / 收 / 心跳 / 空闲回收）、把 `Decision::Open` 兑现成真的 `session.open`、把崩溃 / 判死 / 通知如实转成宿主事件。**事件落地是显式的**（`drain_events`，不在后台任务里碰 registry，免得给所有状态套锁）；旧代连接的遗留事件按 generation 丢弃（否则重启后新实例会被旧断开误伤）；起不来会告诉内核（不留僵尸 `Starting`）；9 条单测。**P3 前半加反向请求透传**：`SupervisorEvent::InboundRequest`（事件泵如实带上 plugin_id/index/generation，旧代事件同样按代丢弃）+ 新事件泵测试 → 8 条单测。**生命周期加 `shutdown_plugin(id)`**（2026-10-15）：只停一个插件的全部实例（同 `shutdown_all` 顺序：先内核摘表、会话如实作废，再收进程）——卸载安全的前提是进程停干净、Windows 才删得掉 exe。**P3.5 加流事件透传**：`SupervisorEvent::{StreamData,StreamEof}` 带上 plugin_id/index/generation，事件泵映射 + `drain_events` 按代过滤（旧连接遗留的流分片不转发）→ 10 条单测（新增：事件泵透传带上下文、无 live 实例时流事件不外报） |
| `sidecar/driver.rs` | ✅ **P1 驱动桥已落地**（含**接引擎 `Database` trait** 的那一层：`SidecarDatabase` —— `query` / `query_with_params` / `query_with_cancel`（**真取消**：令牌响就把 `query.cancel` 递过去）/ `ping` / `meta`，结果**只填 `batches`**（架构 §12 #21，内联 JSON 也在这里补成 `RecordBatch`）；连接持**弱引用**，会话收掉后明确报「连接不可用」；错误按域映射 —— SQL 错带字节偏移光标位、缺能力说「不支持」、事务未接线**明确报不支持**）：`SessionDriver`（会话之上的调用：`driver.describe` / `query.execute` / `query.fetch` / `query.cancel` / `session.ping`）+ `QueryPage` / `PageData`（内联 JSON 或 **Arrow 批**，承载方式对消费方不可见）+ `DriverError`（按错误码分流：`is_cancelled` / `is_capability_denied` / `is_sql_error`）；Arrow 附件在宿主侧解成 `RecordBatch`（可直接喂 DuckDB）；4 条单测 |
| `tests/driver_bridge_real_process.rs` | ✅ **P1 数据面端到端**（靶子同上，用 arrow crate 真编 IPC 流）：小结果内联 / **3000 行 Arrow 过进程边界**（逐值 + `rds.*` metadata + `Timestamp(us, UTC)` 都对得上）/ 逼走内联那条路的行数一致 / 取消 5s 的慢查询（<1s 收场，`-32004`）/ SQL 错带得出 `sqlstate`；6 条集成测试 |
| `sidecar/factory.rs` | ✅ **P1 注册路径已落地**：`SidecarDriverFactory`（实现 `DriverFactory`：`descriptor()` 来自清单，`create()` = 起进程 → 握手 → 开会话 → describe → `SidecarDatabase`）+ `register_sidecar_drivers()`（把插件声明的驱动注册进 `DriverRegistry`，幂等）。连接配置**原样**交给驱动进程；排队超时会明确失败并撤掉排队；句柄丢掉 = 关连接（`Drop` 把会话交还内核）；2 条单测 |
| `tests/factory_real_process.rs` | ✅ **P1 工厂路径端到端**（靶子同上）：装一个「插件目录」（二进制 + 清单）→ 注册 → `DriverRegistry::get` → `create(配置)` → 查询 3000 行 → 丢掉句柄后会话被交还 / 串行第二条连接排队并超时（且不留幽灵会话）/ 二进制不在时进连接域并说清原因 / 取消仍然真取消；4 条集成测试 |
| `tests/database_real_process.rs` | ✅ **P1 接引擎的端到端**（靶子同上）：3000 行经 `Database::query` 进 `QueryResult.batches` 且 `to_rows()` 值对得上 / 内联那条线格式也进 `batches` / 取消令牌 → 真取消（<1s 收场）/ SQL 错带 1 基字符→字节偏移的光标位 / 事务未接线明确报不支持 / `meta()` 说得出支持 Arrow / 会话收掉后句柄明确失效；7 条集成测试 |
| `tests/supervisor_real_process.rs` | ✅ **P1 真进程端到端**（靶子同上）：开会话过进程边界 / 串行排队与自动放行 / 放行失败如实作废且不卡住实例 / 崩溃要求人工重启 / 心跳判死 / 空闲回收后旧事件不误伤新实例 / 起不来不留僵尸 `Starting` / 收摊全部作废；8 条集成测试。**生命周期加 `shutdown_plugin` 只停单插件**（2026-10-15）：双插件各起实例，只停 A，断言 B 的进程 / 会话不受影响 → 9 条集成测试 |
| `tests/lifecycle.rs` | ✅ **生命周期端到端**（2026-10-15 新建）：热插拔（目录出现→自动加载激活、消失→停用移除、清单变化→needs_reload）、磁盘卸载（目录删除 + 索引清理 + 事件 + 幂等 + id 安全校验）、启停（disable/enable 事件）、全量重载（清快照重扫全部重新加载激活）；6 条集成测试 |
| `tests/fixture/sidecar.rs`（`[[bin]] rds-sidecar-fixture`） | ✅ **测试靶子**：**独立的**帧编解码对端（两侧不共用实现，见 `dev-plan` §6.1），并用 arrow crate 真的编出 IPC 流。脚本化 SQL：`select rows=<n>` / `hold_ms=<n>`（可取消的慢查询）/ `fail`；开关 `--ignore-eof` / `--exit-ms=<n>` / `--force-inline`。**不连真库** —— 真库那一步属实机验收 |
| `tests/spawn_real_process.rs` | ✅ **P1 进程层端到端**：起进程 → 握手 → 调用 → 收摊、超时不断连、崩溃交还在飞调用并给出退出码、**没调用也能发现它死了**、不守 EOF 约定时强杀、stderr 落盘；8 条集成测试 |
| `sidecar/{health_checker,hot_reload_manager}.rs` | **2 字节**空文件（P1 健康检查会落在这里） |
| ~~`sidecar/driver.rs`~~（311 行） | ✅ **P0 删除**：未编译过，且传输假设（HTTP/单端口/JSON 行）与 D5/D4 相反 → 驱动桥 **P1 新建** |
| ~~`storage.rs`~~（107 行） | ✅ **P0 删除**：未编译、全仓零引用、`flush_to_disk()` 是 TODO 空壳 |
| ~~`wasm/host_functions.rs`~~（98 行） | ✅ **P0 删除**（含 `wasm/mod.rs` 的 `pub mod` 声明）：4 个函数全部无条件返回错误且零调用，签名也不对。口径：**现在不提供任何 host function**，P3 重建 |
| `commands.rs` / `host.rs` / `model.rs` / `plugin_view.rs` | 各 3 行占位，未在 `lib.rs` 声明；P4 接注册表时才有归属 |
| `plugin_bridge.rs` / `events.rs` | 宿主侧桥接与事件总线（插件视图骨架 `plugin_view.rs` 是上面一行那个占位，尚未实现） |

全局表名是 **`plugins`**（`migrations/global/001_init.sql:128`），不叫 `plugin_store`。

## 5. 硬约束（写代码前先过一遍）

1. **不外泄凭据**：扩展宿主与 webview 都拿不到凭据；只有驱动进程拿得到，且安装时明示。
2. **能力必须门控**：未声明的调用返回 `-32006 capability_denied` 并给出可读原因；不允许"静默退化"。
3. **两条权限轨语义不通用**：扩展轨（`rds.*` 方法级）与驱动轨（`spawn.child_process` / `net.connect` / `fs.read_plugin_data` / `env.inherit`）**不互相折算**。
4. **`engines.rds` 必填且不可 `*`**，加载与安装**两处**都要真的拒绝（`ManifestParser::parse` 已强制调用 `check_engine_compatibility`；2026-10-15 起 `"*"` 也真的被拒绝——此前放行）。
5. **不把 Arrow 塞进 JSON**：不做 base64（+33% 体积、内存双份、不可流式）；正确做法是帧级 `kind` 分流。
6. **路径统一走 `paths`**：`plugins_dir` / `plugin_dir` / `plugin_data_dir` / `plugin_cache_dir` / `sidecar_work_dir` / `plugin_registry_dir` 已于 P0 补齐（含 `validate_plugin_id` 白名单：**id 由第三方清单给，拼路径前必须先校验**）；注意仓里已有的 `extensions_dir()` 是 **DuckDB 扩展**的目录，别混用。
7. **插件根目录 = `<RDS_HOME>/plugins/`**（不是 `~/.rds/...`；`RDS_HOME` 默认是安装目录，可被环境变量覆盖）。
8. **`render` 保持纯读**：插件相关状态不得在渲染期做 I/O；宿主渲染插件的面板也必须走既有控件与 token（零裸尺寸/零裸色值）。
9. **升级纪律**：manifest 里 `gpui-kit` 是 caret，**不要跑裸 `cargo update`**（会静默把整条 UI 栈换代）；要升用 `cargo update -p gpui-kit`，见 `plugin-prototype-design.md` §11.2。

## 6. 命令

```sh
# 插件 crate 不在 default-members 里，必须显式 -p
cargo check -p rds-plugin
cargo test  -p rds-plugin

# 上面这条会连 tests/spawn_real_process.rs 一起跑：它**真的起进程**
# （rds-sidecar-fixture，见 §4 表末行），不是 tokio::io::duplex 那种内存对端
cargo test -p rds-plugin --test spawn_real_process

# 全仓（别名自带 -j 2 与 RUST_MIN_STACK）
cargo check-all
cargo test-all

# 原型自查（可选，需要 node）
cd docs/architecture/plugin/prototype && node check-prototypes.mjs
```

> 本仓**无 CI**；`cargo fmt --all --check` 历史上有大量存量差异，别把全仓格式化混进功能提交。

## 7. 当前进度

见 `plugin-dev-plan.md` §11，**进度口径以它为准**。**P0 已完成**（2026-09-20）：`paths` 六个函数 + 插件 id 白名单 + 权限四轨 + 删三个死文件 + 修 `client.rs` 反向判据 + 文档清理。

**P1 代码面已齐**（十块 = `sidecar/` 八个模块 proto / router / lifecycle / conn / process / supervisor / driver / factory + `manifest.rs` 的 `[backend]` 段 + 集成测试集）：`cargo test -p rds-plugin` **106/106** 单测 + **44/44** 真进程集成测试。协议 / 进程 / 内核 / 运行时 / 驱动桥 / 接引擎 / 注册路径全通。剩下的是**要在真机上才做得了**的三项：① 拿 PostgreSQL 包一层 sidecar 做靶子（真库 + 真 Arrow）；② 跨语言 Arrow 互通（Go `arrow-go` 或 Python `pyarrow` 写、我们读 —— 本仓当前环境拉不到这些依赖）；③ 把这两条固化成验收脚本。

**P2 代码面已齐**（四刀：`meta.*` 元数据面 + `DriverCapability` 能力门控 + 导航树相关）：`cargo test -p rds-plugin` **114/114** 单测 + **61/61** 集成测试。其中导航层的「置灰」视觉没算完成，随导航层一起进实机验收。

**P2.5 第 1 刀（Arrow 直灌底座）已落地**：`cargo test -p rds-engine --lib` **483/483**。第 2/3 刀未做 —— 卡点是 `ResultEntry` 目前不带 Arrow。

**P2.6 协议 method/api 白名单已落地**（2026-10-15，借鉴 navop 双仓库调研结论）：`sidecar/method.rs` 集中 method 常量 + 声明白名单；`PluginManifest` 增顶层 `api`（能力面命名空间，小写标识符白名单）与 `methods`（安装期 `is_allowed_declaration` 校验）。`cargo test -p rds-plugin` **124/124** 单测 + **61/61** 集成测试。这一步是 P4 市场「扩展声明什么能力」的前置 —— 后续 Redis / MongoDB / SSH 隧道 / 终端等 sidecar 扩展各占一个 `api` 命名空间。

**P3 前半（反向 Host API 面）已落地**（2026-10-15，借鉴 navop `extension-protocol/src/host.rs` + 用户插件化路线决策）：`sidecar/host.rs` 定义 `host.*` 方法面（凭据 SecretRef 引用制 / 通知 / 存储 / 日志 / SSH 隧道）+ 方法级 glob 权限闸；`router.rs` 按 JSON-RPC 判据识别反向请求（带 id + method），`conn.rs` 增 `reply` / `reply_error`，`supervisor.rs` 事件泵透传 `InboundRequest`（按代过滤）。

**宿主接线已闭环**（2026-10-15）：`manifest.rs` 的 `permissions.host` 承载权限声明；`host_provider.rs` 实现 storage（真落盘 + ttl）与 log（tracing）；`runtime.rs` 运行时循环把事件泵的 `InboundRequest` 路由到 handler 并回帧（实例已收掉则丢弃）；`init_plugin_manager` 在 app 启动时接线（`crates/app/src/main.rs::init_global_system`，失败不阻断启动）并自动 spawn 循环。**仍保持 `Unsupported`**（诚实口径）：`request_credential` / `secret.resolve`（宿主还没有通用 secret store）、`notify`（通知按钮要 UI 回调通道，P4 接界面）、`ssh.open_tunnel`（宿主还没有 SSH 连接引擎）。`cargo test -p rds-plugin` **155/155** 单测 + **61/61** 集成测试。下一步：SSH sidecar 协议方法（`x.ssh.*` + 字节流帧）或 wasm render-document（P3 后半）。

**插件生命周期（P4 前置）已落地**（2026-10-15，VS Code 口径）：六态状态机补全最后三态 —— `shutdown_plugin(id)`（supervisor 只停一个插件的全部实例，会话如实作废）；`deactivate_plugin` Sidecar 分支先停进程（卸载删目录的前提）；`uninstall_plugin` 磁盘卸载（依赖反向索引 `check_dependents` 拒绝被引用者、保留 `plugin-data`、清 global.db、事件）；`enable/disable_plugin`（DB `is_enabled` 权威状态 + 事件）；热插拔 = `scan_and_diff_plugins` 挂在运行时循环（目录即插件：新目录自动加载激活、消失自动停用移除、清单 hash 变化置 `needs_reload` **不热生效**）+ `reload_plugins` 全量重载；`hot_reload_manager.rs` 空壳落为薄封装。`cargo test -p rds-plugin` **155/155** 单测 + **75/75** 集成测试。**未接**（诚实列出）：`install_package` 无调用方（安装 UI 是 P4）；`request_credential` / `notify` / `ssh.open_tunnel` 仍 `Unsupported`；`enable/disable` 的持久化在 DB、UI 开关是 P4；依赖拒绝在 DB 未初始化时保守放行（卸载只删目录）。

**SSH sidecar 轨 · 协议方法面（第一刀）已落地**（2026-10-15）：`sidecar/ssh.rs` 定义正向 `x.ssh.*` 五方法（connect 建会话凭据走 SecretRef / exec 一次一答 / shell+forward 通道契约就绪 / close 幂等），类型即契约，SSH 插件照此实现即可与宿主桥对接；`x.` 私有前缀天然通过安装期声明校验。`cargo test -p rds-plugin` **165/165** 单测 + **75/75** 集成。

**SSH sidecar 轨 · 字节流帧（第二刀）已落地**（2026-10-15）：`KIND_STREAM = 0x03` 让 shell/forward 成为**持续双向字节流**（不是一次一答）。payload 布局 `[u16 BE 通道 id 长][channel_id UTF-8][u8 flags][流数据]`，flags `0x00`=数据 / `0x01`=EOF；方向由发送者隐含（插件→宿主=上行，宿主→插件=下行）；与 Arrow 附件（KIND_ARROW）**正交**——流帧随时可插队，不进入「响应扣住等附件」的装配。五文件改动：`proto.rs`（新 kind + `Frame::stream`/`stream_eof`，通道 id 超 `u16::MAX` 构造前拒绝）、`router.rs`（`StreamData`/`StreamEof` 事件 + `decode_stream_payload`，坏 payload = Issue 不 panic）、`conn.rs`（`ConnEvent` 流两变体 + `DriverCommand::WriteStream` + `write_stream`/`write_eof`）、`supervisor.rs`（事件泵按代透传，旧连接遗留流分片不转发）、`runtime.rs`（显式分发，debug 日志**只记元信息**不记数据本体——流数据可能大且敏感）。`cargo test -p rds-plugin` **183/183** 单测 + **75/75** 集成（supervisor 新增 2 条：事件泵透传带上下文、无 live 实例时流事件不外报）。**宿主侧消费未接**（P4）：终端面板订阅 `StreamData` 渲染 shell 输出并回写 stdin、端口转发面板订阅通道数据——协议面已就绪，宿主 UI 是下一段工作。

**渲染计算轨（wasm render-document）第一刀已落地**（2026-10-15）：`manifest.capabilities.wasm.render_document` 声明段（函数名 / formats / output_format）+ `render.rs` 渲染服务（`renderer_for` 按格式选渲染器 + `render_document` 调 wasm 插件收产物 + `resolve_spec` 契约预览），纯计算、无 host function 依赖。`cargo test -p rds-plugin` **171/171** 单测 + **75/75** 集成。

**渲染计算轨第二刀（wasm 实证 + 定位更正）已落地**（2026-10-16，commit 4b2274d1）：`tools/plugin-render-mermaid`（mermaid-rs-renderer 0.3.1 编 wasm，Extism PDK 1.4）——JSON 信封契约（source + theme.preset/overrides，非 JSON 退化纯文本）+ 嵌入 DejaVu Sans 子集（200KB）+ 全链路实证 12/12（5 图类型 / CJK / 信封 / 退化 / 明暗 / 语法错）。wasm 2.59MB，首调 26ms。宿主 `render.rs` 信封辅助 + `extism.rs` host_log no-op（panic hook 诊断通道）。`cargo test -p rds-plugin` **186/186** 单测 + 集成全绿。**定位更正（用户拍板方案 A）**：本刀产物属「**一次性渲染 / 导出轨**」（Mermaid 文本 → SVG 预览 / 导出 / 文档嵌入），**不是** ER 绘图交互主路线——主路线是 Q11 定案的 GPUI 原生渲染（`crates/database/src/er/` 已启动）；wasm 轨维持「不再扩建」口径。详见 `plugin-dev-plan.md` §11.4 + `plugin-prototype-design.md` §11.3.1。

**P3（wasm host function 面）、P4（注册表与界面）、P5（生态）未开始**。当前 wasm 插件拿不到任何宿主能力（与「默认拒绝」一致）。

HTTP 旧路径（`client.rs` + 旧 `manager.rs`）已删除，`rds-plugin` 不再依赖 `reqwest`。
验收仍是「能连 → 能查 3000 行（Arrow 到宿主）→ 能取消 → **宿主退出无孤儿进程**」（后者靠一条协议级约定：宿主持有 stdin 管道，sidecar 见 EOF 即退，见 dev-plan §4.2.1）。
