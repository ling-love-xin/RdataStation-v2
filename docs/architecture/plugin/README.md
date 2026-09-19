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
| Q7 | ✅ = D6 |
| Q8 | ✅ webview **可提供，但限全屏面板 / 独立窗口**；与 GPUI 元素混排**实践上不支持**（z-order）；本仓要接就锁 `gpui-wry = "=0.6.1"` |
| — | 错误码表见 `plugin-dev-plan.md` §4.2.2（`-32001`…`-32008`；能力拒绝是 `-32006 capability_denied`） |

**未拍板（别当成已有）**

| # | 待定 | 影响 |
| --- | --- | --- |
| Q1 | 项目引用表：迁移到 `project_resources(kind,id,version,enabled,enforce,config_json)` vs 沿用现有两表 | P0 是否含一次迁移（现无生产调用方，建议迁） |
| Q9 | 键位贡献是否开放 | 命令面板之外的可见性 |
| Q10 | wasm 轨：保留为「纯计算/性能轨」vs 冻结 | M9 范围大小（Q7 定了之后建议冻结） |

## 4. 代码地图（`crates/plugin/`，现状实测）

| 文件 | 现状 |
| --- | --- |
| `manifest.rs` / `model.rs` / `permission.rs` | 清单与权限骨架（P0 已扩到**四轨**：`Frontend`/`Wasm` 门控 + `Sidecar`/`Driver` 展示轨，见 `PermissionType::is_gating`）；`plugin-prototype-design.md` §3 是它的**增量扩展**，不是第二份契约 |
| `plugin_service.rs` | **项目级 6 方法已实现**；表 `project_used_plugins` / `project_plugin_config` 已建（`migrations/project_meta/001_init.sql:112/121`） |
| `manager.rs` / `loader.rs` / `installer.rs` / `dependency.rs` | 安装与装载骨架 |
| `sidecar/manager.rs` | 现状是「一 manager 一进程 + 单 `port`」，要演进成 `PluginProcess → DriverInstance → Session` 三层 |
| `sidecar/client.rs` | P0 已修「判成功/失败取反」的 bug（抽成 `parse_rpc_response` + 4 条单测）；但**传输本身仍是 HTTP/端口 + 零鉴权**，与 D5 相反 → P1 换成走 `proto` 的 stdio 客户端 |
| `sidecar/proto.rs` | ✅ **P1 协议层已落地**：帧（4B 大端长度 + 1B kind，**`total_len` 含头 5 字节**）+ 增量解码器 + `read_frame`/`write_frame`（async，含短读与“先校验再分配”的测试）+ 版本闸 + 内联阈值 + 错误码表；15 条单测 |
| `sidecar/router.rs` | ✅ **P1 附件语义已落地**：`Router`（消费帧 → 事件：在飞登记 / 扣住未收齐的响应 / 错位上报 / 断线交还）+ `encode_response_with_arrow`（sidecar 侧切帧）；两者互为逆运算，有往返测试；15 条单测 |
| `sidecar/lifecycle.rs` | ✅ **P1 决策内核已落地**（sans-io，零 I/O）：三层对象模型 `PluginProcess → DriverInstance → Session` 的规则全部在此 —— 进程按 plugin_id 去重、`max_instances`、serial 排队与 `QUEUE_MAX_LEN`、ping 连续 2 次判死、空闲 30min 回收、崩溃**不静默重连**（需手动重启）；15 条单测逐条对应 §4.1 五条规则 |
| `sidecar/conn.rs` | ✅ **P1 异步客户端已落地**：`SidecarConn::spawn(reader, writer)` 起三个任务（调用方 / driver / 读侧）；**在飞状态只有一份**（Router + id→oneshot 都在 driver 任务）；`call` 带超时且超时后显式 `Abandon`（迟到响应会报成 Issue）；`initialize` 内置版本闸；`shutdown` 只发命令（真正的回收靠“丢写侧 → 对端 EOF 自退”）；12 条单测 |
| `sidecar/{health_checker,hot_reload_manager}.rs` | **0 字节**空文件（P1 健康检查会落在这里） |
| ~~`sidecar/driver.rs`~~（311 行） | ✅ **P0 删除**：未编译过，且传输假设（HTTP/单端口/JSON 行）与 D5/D4 相反 → 驱动桥 **P1 新建** |
| ~~`storage.rs`~~（107 行） | ✅ **P0 删除**：未编译、全仓零引用、`flush_to_disk()` 是 TODO 空壳 |
| ~~`wasm/host_functions.rs`~~（98 行） | ✅ **P0 删除**（含 `wasm/mod.rs` 的 `pub mod` 声明）：4 个函数全部无条件返回错误且零调用，签名也不对。口径：**现在不提供任何 host function**，P3 重建 |
| `commands.rs` / `host.rs` / `model.rs` / `plugin_view.rs` | 各 3 行占位，未在 `lib.rs` 声明；P4 接注册表时才有归属 |
| `plugin_view.rs` / `plugin_bridge.rs` / `events.rs` / `commands.rs` | 宿主侧桥接与视图骨架 |

全局表名是 **`plugins`**（`migrations/global/001_init.sql:128`），不叫 `plugin_store`。

## 5. 硬约束（写代码前先过一遍）

1. **不外泄凭据**：扩展宿主与 webview 都拿不到凭据；只有驱动进程拿得到，且安装时明示。
2. **能力必须门控**：未声明的调用返回 `-32006 capability_denied` 并给出可读原因；不允许"静默退化"。
3. **两条权限轨语义不通用**：扩展轨（`rds.*` 方法级）与驱动轨（`spawn.child_process` / `net.connect` / `fs.read_plugin_data` / `env.inherit`）**不互相折算**。
4. **`engines.rds` 必填且不可 `*`**，加载与安装**两处**都要真的拒绝（现状 `check_engine_compatibility` 未被强制）。
5. **不把 Arrow 塞进 JSON**：不做 base64（+33% 体积、内存双份、不可流式）；正确做法是帧级 `kind` 分流。
6. **路径统一走 `paths`**：`plugins_dir` / `plugin_data_dir` / `plugin_cache_dir` / `sidecar_work_dir` **尚未存在**，要用先补；注意仓里已有的 `extensions_dir()` 是 **DuckDB 扩展**的目录，别混用。
7. **插件根目录 = `<RDS_HOME>/plugins/`**（不是 `~/.rds/...`；`RDS_HOME` 默认是安装目录，可被环境变量覆盖）。
8. **`render` 保持纯读**：插件相关状态不得在渲染期做 I/O；宿主渲染插件的面板也必须走既有控件与 token（零裸尺寸/零裸色值）。
9. **升级纪律**：manifest 里 `gpui-kit` 是 caret，**不要跑裸 `cargo update`**（会静默把整条 UI 栈换代）；要升用 `cargo update -p gpui-kit`，见 `plugin-prototype-design.md` §11.2。

## 6. 命令

```sh
# 插件 crate 不在 default-members 里，必须显式 -p
cargo check -p rds-plugin
cargo test  -p rds-plugin

# 全仓（别名自带 -j 2 与 RUST_MIN_STACK）
cargo check-all
cargo test-all

# 原型自查（可选，需要 node）
cd docs/architecture/plugin/prototype && node check-prototypes.mjs
```

> 本仓**无 CI**；`cargo fmt --all --check` 历史上有大量存量差异，别把全仓格式化混进功能提交。

## 7. 当前进度

见 `plugin-dev-plan.md` §11。**P0 已完成**（2026-09-20）：`paths` 六个函数 + 插件 id 白名单 + 权限四轨 + 删三个死文件 + 修 `client.rs` 反向判据 + 文档清理。

**P1 进行中**：四块已落地（`sidecar/proto.rs` 帧与流读写 / `sidecar/router.rs` 附件语义 / `sidecar/lifecycle.rs` 决策内核 / `sidecar/conn.rs` 异步客户端，P1 共 57 条新单测）。
接着要做的：把决策内核接到 I/O（`SidecarManager` 补 `current_dir`/日志/进程组回收）· 拿 PostgreSQL 包一层 sidecar 做靶子 · 删 `client.rs`（HTTP 旧路径）。
验收仍是「能连 → 能查 3000 行（Arrow 到宿主）→ 能取消 → **宿主退出无孤儿进程**」（后者靠一条协议级约定：宿主持有 stdin 管道，sidecar 见 EOF 即退，见 dev-plan §4.2.1）。
