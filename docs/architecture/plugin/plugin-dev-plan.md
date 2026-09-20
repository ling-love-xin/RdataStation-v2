# 插件系统（M9）— 开发方案

> 做什么、做到哪：现状盘点 / 决策记录 / 与既有设计的对照 / 目标形态与契约 / 阶段任务 / 验收与测试场景 / 风险 / 验证命令 / 实现位置映射 / 进度记录。
> 设计与硬约束见 `plugin-architecture.md`（含路径 §6、三层资源模型 §7）与 `../runtime/data-paths.md` §8；UI 尺寸与取色见 `../ui/`、`../theme/`。
>
> **状态口径**：本文档首次建立时 M9 **整包无调用方**（见 §1）。本文档把「能落地的部分」与「等上游的部分」分开排期，不做「全都要」的计划。

---

## 1. 现状盘点（本轮开始前）

### 1.1 已有资产（复用，不重做）

| 层 | 状态 | 证据 |
| --- | --- | --- |
| 清单类型 | ✅ 结构齐备（四类 contributes + capabilities + permissions + dependencies） | `crates/plugin/src/manifest.rs`（575 行，`PluginManifest::validate` / `check_engine_compatibility`） |
| 权限骨架 | ⚠️ 只有 `PermissionType::{Frontend, Wasm}` | `crates/plugin/src/permission.rs:13-18`；`PermissionGrant` / `GrantStatus` / 全局单例可用 |
| 服务面 | ✅ 全局 11 方法 + **项目级 6 方法** | `crates/plugin/src/plugin_service.rs:81-419`（`enable_plugin_in_project:233` 等） |
| 生命周期 | ⚠️ 结构齐、无调用方 | `crates/plugin/src/manager.rs:57-327`（`scan/load/activate/deactivate/unload/list` + 全局 `init_plugin_manager`） |
| 安装 | ⚠️ 可复用 | `crates/plugin/src/installer.rs:32`（`detect_format`）、`:56`（`install_package`） |
| 全局表 | ✅ 已建 | `plugins` / `plugin_dependencies` / `plugin_global_config`（`crates/engine/migrations/global/001_init.sql:128/147/156`） |
| **项目引用表** | ✅ **已建**（文档称「V2 无」，见 §10） | `project_used_plugins(plugin_code, plugin_version, enabled, required)` + `project_plugin_config`（`crates/engine/migrations/project_meta/001_init.sql:112/121`） |
| 驱动接入面 | ✅ 契约完整 | `DriverFactory`（`engine/src/driver/registry/mod.rs:52-69`）、`Database` 必需 4 法（`traits.rs:315-509`）、`MetadataBrowser` 必需 4 法（`traits.rs:130-202`） |
| 驱动槽位 | ⚠️ 空壳 | `engine/src/driver/wasm/driver.rs`、`engine/src/driver/jdbc/driver.rs`（全 `NotSupported`）；`DriverKind` 八值已声明（`registry/descriptors.rs:117-126`：`Native/Jdbc/Odbc/Wasm/Adbc/Http/Python/Js`） |
| Arrow 数据通路 | ✅ **引擎侧已通** | `QueryResult.batches: Vec<RecordBatch>`（`shared/src/models.rs:31`）；六个内置驱动均产真 `RecordBatch`；`shared/src/arrow.rs` 有 `ArrowHandler`/`ArrowBatchStream` |
| 宿主导出（Quick Open / 面板枚举） | ⚠️ 硬编码 | `workbench_shell/src/model.rs:22-63`（`LeftPanel`/`RightPanel` + `const ALL`）；`workbench/src/panels/mod.rs:343-348` 直接 match |

### 1.2 死代码（要么补、要么删）

| 文件 | 行数 | 问题 | P0 处置（2026-09-20） |
| --- | --- | --- | --- |
| `crates/plugin/src/sidecar/driver.rs` | 311 | **`sidecar/mod.rs` 只声明 `client`/`health_checker`/`hot_reload_manager`/`manager`，没有 `pub mod driver`** → 不参与编译；且实现旧版 `DriverFactory`（`id/name/kind/default_port/create_pool`），v2 是 `descriptor/create` | ✅ **删除**。理由：它的传输假设（HTTP + 单端口 + JSON 行）与 D5/D4（stdio 分帧 + Arrow）**相反**，不是“补个声明”能救；P1 要建的三层对象模型也不在它基础上长。驱动桥 **P1 新建**（git 历史保留） |
| `crates/plugin/src/storage.rs` | 107 | 未在 `lib.rs` 声明 | ✅ **删除**。全仓零引用；`flush_to_disk()` 是 TODO 空壳、`get_storage_path()` 从未被用过（磁盘路径根本没写过）；设计的落点是 `paths::plugin_data_dir(id)` + 设置登记表 + `project_resources.config_json` |
| `crates/plugin/src/{commands,host,model,plugin_view}.rs` | 各 3 | 未声明；这正是「宿主端口 + 自带视图」该补的位置 | ⬜ 保留不动（P4 接注册表时才有归属） |
| `crates/plugin/src/sidecar/{health_checker,hot_reload_manager}.rs` | **0 字节** | 已声明但空 | ⬜ 保留（P1 健康检查会落在这里） |
| `crates/plugin/src/wasm/host_functions.rs` | 98 | 4 个 host function（`plugin_db_query`/`plugin_db_metadata`/`plugin_duckdb_query`/`plugin_duckdb_load`）**全部无条件返回错误** | ✅ **整文件删除**（含 `wasm/mod.rs` 的 `pub mod` 声明）。两个原因：① 全仓零调用（`register_all` 没有任何调用方）；② 签名就是错的 —— Extism 读宿主内存要走 `host_fn!` + `Memory`，不是 Val 数组。口径：**现在不提供任何 host function**（wasm 插件拿不到宿主能力，与“默认拒绝”一致），真面在 P3 按 Q4 收敛后重建 |

### 1.3 缺口（本方案要补的）

| 缺口 | 证据 |
| --- | --- |
| `paths` 无插件目录函数 | `crates/paths/src/lib.rs` 只有 `home/config_dir/data_dir/log_dir/temp_dir/extensions_dir`；无 `plugins_dir` / `plugin_data_dir` / `plugin_cache_dir` / `sidecar_work_dir` |
| 权限无 sidecar 类 | `permission.rs:13-18` → ✅ P0 已加 `Sidecar` / `Driver` 两轨（含 `is_gating()` 与两份测试） |
| **wasm 无真实限额** | `ResourceLimits{max_memory_mb:512, max_cpu_time_ms:30000}`（`wasm/plugin_manager.rs:14-42`）检的是 `update_resource_usage()` 自报值；`WasmRuntimeConfig{max_execution_time_ms:30000}`（`wasm/mod.rs:127-149`）是另一套，两者均未落到 Extism/wasmtime |
| ~~sidecar 传输不宜~~ | ~~`sidecar/client.rs:90-100` 走 `reqwest` 打 `http://localhost:{port}`，**零鉴权**，端口由子进程 stdout 自报（`sidecar/manager.rs:120-133`），`stderr(inherit)`、无 `current_dir`~~ → ✅ **P1 两个文件已删**（功能由 `process.rs` + `conn.rs` + `supervisor.rs` 取代） |
| **sidecar 无法取消** | 一问一答 HTTP POST 无法在执行中途通知子进程；而 `Database::query_with_cancel` 是必需方法（`traits.rs:339-344`）、编辑器有「中断」入口 |
| **Arrow 跨进程结构性丢失** | `QueryResult` 的手写 `Serialize` 只输出 6 个字段、**不含 `batches`**（`shared/src/models.rs:355-370`）→ 原「sidecar 返回 `QueryResult`」这条路只剩 `rows: Vec<Vec<Value>>`。这正是 `sidecar/driver.rs:41` 写 `supports_arrow: false` 的原因（诚实但受限） |
| 面板/命令/设置无贡献点 | `workbench_shell/src/model.rs:22-63`、`workbench/src/quick_open/commands.rs:79-85`、`plugin/src/plugin_view.rs`（3 行占位） |
| 凭据口径未定义 | 见 §4.6.3 |

### 1.4 链路断点（Arrow 相关，本方案的核心动因）

| 位置 | 现状 |
| --- | --- |
| **唯一压缩点** | `workbench/src/services/editor_exec.rs:1089-1106` `fn to_data`：`result.to_rows()` → `fn cell_text`（`:1133-1135`）= `Value::to_string()`。三条查询路径（源库+加速 `:155-177`、联邦 `:295-317`、源库分段 `:537-541`）**全经此处** |
| 落点 | `QueryData.rows: Vec<Vec<String>>`（`editor/src/execution.rs:421-435`）→ `ResultEntry.rows`（`editor/src/store.rs:60-64`）；**`ResultEntry` 无 `column_types`，`QueryResult.column_types` 在此丢弃** |
| 类型退化 | `arrow_value_at` 的兜底把 `Decimal`/`Date32-64`/`Time`/`Timestamp`/`Interval`/`List`/`Struct`/`Map` 一律降为 `Value::Text`（`shared/src/models.rs:346-351`）；`UInt64` 溢出 i64 → Text（`:317-324`）；**`Value::Bytes` 的 `Display` 是 `{:?}` 字节数组**（`:422`），blob 到网格/导出变成 `[137, 80, …]` |
| **二次分析桥** | `AnalysisRequest.rows: Vec<Vec<String>>`（`editor/src/analysis.rs:31-40`）→ 逐格 `cell_json`（`editor_exec.rs:782-788`，`"NULL"`→null，其余全 `Value::String`）→ `create_temp_table_internal`（`engine/src/duckdb/duckdb_service.rs:38-101`）：**`infer_type` 整列字符串推断 + 逐行参数化 `INSERT INTO … VALUES (?…)`**；回程经 JSON → `json_cell_text` 又变字符串（`editor_exec.rs:791-797`） |
| 结果网格消费面 | 渲染取 `String`；本地筛选是小写子串匹配（`editor/src/view/results/grid.rs:288-310`）；本地排序靠 `parse::<f64>()` 猜类型、失败回退字符串比较（`:423-437`） |
| 失效路径 | `execution_service.rs:9` `re_execute_with_filter`（+ workbench 包装 `result_service.rs:25`）**全仓零调用**（含测试）；其内 `:53` 的 `extract_rows_from_serialized` 读 `json_value["batches"]`，而 `QueryResult::serialize` 已排除 `batches` → **恒返回空行**。**不影响用户**（无人调用），属零调用死代码 |
| 本地筛选/排序**不经临时表** | `editor_exec.rs:405 run_filtered` / `:467 run_sorted_down` 走 **B14「重写 SQL 下发」**（`engine::sql::rewrite_with_filter` / `rewrite_with_order`），不经过 `create_temp_table_internal` → **与 Arrow 无关，不必改** |
| `supports_arrow` 无人门控 | `engine/src/driver/capability.rs:261-270` 把 `MetaBit::Arrow` 收进 `META_BITS_WITHOUT_UI_KEY`，注释：**该位目前只用于插件通信声明**——即该位当初就是为插件预留的 |

---

## 2. 决策记录

### 2.1 已定（本方案立论）

| # | 决策 | 依据 |
| --- | --- | --- |
| **D1** | **三轨承载**：驱动 = sidecar（native 子进程）；面板/命令/设置 = **独立扩展宿主进程（VS Code 模式）**；分析/规则/纯数据变换 = wasm（**是否冻结待 Q10**） | WASI 的 socket/HTTP/TLS 未标准化；Tabularis（16 插件）与 DBX（Agent）的驱动都是原生子进程；`gpui-shell` 明确**否决 wasm 做 UI**（"Every call crosses a serialization boundary"）。扩展形态与 `rds` API 面见 `plugin-prototype-design.md` |
| **D2** | **两轨边界按 trait 要求画，不按插件大小画**：凡需要 `MetadataBrowser` 全量元数据、真事务、真取消、游标的，一律 sidecar | `Database`/`MetadataBrowser` 的形状是**进程内驱动**的形状 |
| **D3** | **一个契约多个后端**：`PluginManifest` 是唯一对外契约，内部以 `backend.kind` 分派；**不允许两份 manifest、两套权限语义** | 避免「wasm + sidecar + 脚本」三套宿主并存 |
| **D4** | **数据面走 Arrow，控制面走 JSON** | 见 §4.5；`QueryResult.batches` 已是权威通路，跨进程只需换序列化方式 |
| **D5** | **传输改用 stdio + 二进制分帧**，不用本地端口 | 免端口冲突、免 Windows 防火墙弹窗、生命周期天然绑定；且 Arrow IPC 是二进制，不能走 newline-delimited JSON |
| **D6** | **扩展宿主运行时 = 内置 QuickJS（`rquickjs`）**；**有意不兼容 npm**；重计算下沉 `rds.duckdb` | `plugin-prototype-design.md` §11.1：V8 预编译库每平台 36.7–38.8 MiB（Windows 仅 MSVC 资产）且其两大优势（npm 兼容 / JIT）在本模型里都用不上；QuickJS 为 MIT、vendored C（8.2 万行）、`bindgen` 可选（Windows 只需 `cl.exe`） |

### 2.2 待拍板

| # | 决策 | 选项 | 建议 |
| --- | --- | --- | --- |
| Q1 | 项目引用表 | (a) 迁移到单表 `project_resources`（含 `kind`）｜(b) 沿用现有两表 | **(a)**：现两表是 V1 形状且无法容纳引擎扩展；且当前**无生产调用方**，改动成本最低 |
| Q2 | 驱动的 `Database` 面 | (a) 全语义进协议｜(b) 窄 trait + 明确降级 | **(a) 分批**：P1 进 query/cancel/session；事务与游标按能力声明，未支持者**置灰并说明** |
| Q3 | `editor` 如何拿到 Arrow | (a) 直接依赖 `arrow`｜(b) `shared::ResultSet` 薄封装｜(c) trait 对象 | **(b)**：保持 `editor` 依赖薄（现仅 `engine`/`insight`/`shared` 依赖 arrow） |
| Q4 | wasm host functions | (a) 实现那 4 个｜(b) 删除，只留纯计算 + `kv.*` + `duckdb.read_only` | **(b)**：与「wasm 只做纯数据变换」的边界一致 |
| Q5 | 网格改造范围 | (a) 只换数据源｜(b) 顺带类型化筛选/排序 | **(b)**：否则「Arrow 到面板」只剩传输收益 |
| Q6 | **Arrow 的落点范围** | (a) 只到分析引擎（sidecar↔宿主 + DuckDB 直灌，面板仍取字符串）｜(b) 再加编辑器惰性取 `cell_text`（筛选/排序暂不改）｜(c) 全到面板（含筛选/排序/导出） | **(a) 立刻做（=P2.5），(c) 放到 P4 并视收益再定**。三者都不改协议，因为承载与消费已解耦（§4.5.3） |

### 2.3 明确不做（non-goals）

wasm 驱动 · 插件直接绘制 GPUI（只产出声明 + 数据）· 插件自己下载运行时（JRE/Python 内置不在范围）· sidecar 的运行期权限弹窗 · ODBC/ADBC 主干 · **事务内取消**（引擎现状，`sql_service.rs:790-793`，文档写明）· 把 Arrow 送进前端 JSON 契约（`batches` 上的 `#[specta(skip)]` 是有意的）。

---

## 3. 与既有设计的对照（优劣势）

> 对照口径：**既有设计** = `plugin-architecture.md` 的设计意图 + 当前代码状态；**本方案** = 本文档 §4。本节记录「为什么改」，避免后续重复讨论。

### 3.1 先分清比较的性质

两处差异**不是「优 vs 次优」，而是「可行 vs 不可行」**：

| # | 项 | 说明 |
| --- | --- | --- |
| 1 | **Arrow 跨进程** | `QueryResult` 的 `Serialize` 硬编码 6 个字段、不含 `batches`（`models.rs:355-370`）。既有设计「sidecar 返回 `QueryResult`」在结构上**丢 Arrow**，只剩 `rows`。要 Arrow 就必须改协议，没有微调选项 |
| 2 | **真取消** | 一问一答 HTTP POST 物理上无法在执行中途通知子进程。而 `query_with_cancel` 是 `Database` 必需方法、编辑器有「中断」按钮。既有设计只能永远返回「不支持取消」 |

### 3.2 优势

| 维度 | 既有设计 | 本方案 | 收益 |
| --- | --- | --- | --- |
| 数据保真 | 到编辑器压成字符串；`Decimal`/`Timestamp`/`Binary` 退化（blob 变 `[137, 80, …]`） | Arrow 到面板，类型化格式化 | mock 造数不再全落 Text；insight 质量分不再误报；结果网格能正确显示 decimal/时间/blob |
| 类型映射 | 无机制；mock 靠猜首词（`mock/src/schema_map.rs:20-73`） | Arrow schema field metadata 携带 `rds.type_raw`/`rds.canonical` | 类型契约随数据走，不靠字符串猜 |
| 二次分析 | 逐行 `INSERT INTO … VALUES (?…)` + 整列字符串推断 | Arrow 直灌 DuckDB（`register_record_batch` / `Appender`） | 兑现 README 卖点，去掉最大瓶颈 |
| 渲染成本 | 1000 行 × N 列**全量** `to_string()` | 可见区 + 预取窗口惰性格式化 | 行集越大收益越大 |
| 排序/筛选 | `parse::<f64>()` 猜类型、失败回退字符串比较 | 按 Arrow 类型分派比较 | 数值/时间列排序正确 |
| 取消 | 不可能 | 带外 `query.cancel` | 长查询可中断 |
| 分段续取 | 「包装 SQL 重跑」，`has_more` = 恰好 1000 行（`editor_exec.rs:396-397`）→ **恰好 1000 行会永不终止地追加重复行** | `cursor_id` + 驱动真实回答 `has_more` | 修既有坑，且大结果不必重跑源库 |
| 进程模型 | 一 manager 一进程（`sidecar/manager.rs:90-99`：`status != Stopped` 即报错）、单 `port` | `PluginProcess → DriverInstance → Session` 三层 | 支撑「多语言服务 × 多驱动 × 多连接」 |
| 能力缺失 | `DataSourceMeta` 7 位只有 2 位门控（`supports_federated`/`supports_transaction`） | 能力矩阵真门控 + `capability_denied` 错误码 | 不再「能连上但处处静默退化」，对齐「状态如实」口径 |
| 传输安全 | localhost 端口 + **零鉴权** + 端口自报 | stdio 管道（无监听端口） | 本机无第二个入口 |
| 路径纪律 | `./plugins`（随 CWD 漂移）+ `~/.rdatastation`（写 C 盘） | `paths::*` 单根 + symlink 防御 | 符合 `data-paths.md` §8 |
| 项目引用 | 两张 V1 表（无 `kind`，无法容纳引擎扩展） | `project_resources(kind, …)` | 引擎扩展与插件同一模型（原文档 §7.3 的设计意图） |
| 面板/命令/设置 | 面板是硬编码枚举；插件面板是 `render_plugin_placeholder` | 三个注册表 + **未注册 id 占位** | 一次改动同时解决插件面板与「项目引用缺失不阻断」 |
| 顺带修掉 | — | blob `{:?}`、恰好 1000 行 `has_more`、`execution_service.rs:41-54` 恒 0 行死路径、`client.rs:127-155` 反向判据 | 见 §6 |

### 3.3 代价（诚实列）

| 代价 | 说明 | 缓解 |
| --- | --- | --- |
| **改动面显著更大** | Arrow 到面板要动 `editor/src/store.rs`（`ResultEntry`）、`view/results/grid.rs`（渲染/筛选/排序）、`host.rs`（导出取行）、`to_tsv`、`mock_generator.rs:407-417` | 按 §5 顺序做：先拿 §4.5.2 二次分析的收益（改动局部），再动网格 |
| **实现成本** | 二进制分帧要自写（`jsonrpsee` 开箱只带 http/ws transport）；Arrow 兼容子集要守门 | 分帧约 200 行，LSP 有参考；子集清单写进协议版本 |
| **调试更难** | 二进制帧 + Arrow IPC 出错时不如 JSON 肉眼可看 | 提供诊断子命令（dump 帧 / 打印 schema）；**保留 `--transport http` 仅作调试通道**（绑 `127.0.0.1` + 短期 token） |
| **跨进程无零拷贝** | 不同地址空间只能 IPC 序列化 | 认清收益是「有类型 + 列式 + 可直接喂 DuckDB」，不是零拷贝（零拷贝只在进程内，`duckdb ↔ arrow` 已有） |
| **前期更慢** | 既有设计「能连能查」约 1 周；本方案 P0+P1 约 3 周 | 但 P1 结束时契约已被一次真实校验 |
| **新增测试门槛** | golden IPC 跨语言互操作 + 类型保真往返 | 本仓目前**无 CI**，这是新增的门（§6） |
| **依赖面微增** | `engine`/`insight`/`shared` → 加 `editor` 侧的 `ResultSet` 使用 | 用 (Q3-b) 薄封装，不新增 crate 依赖 |

### 3.4 既有设计中**值得保留**的三点

1. **HTTP 调试通道的便利**——`curl` 可打、日志可读。保留为 `--transport http`（P5 之后可选实现），仅调试。
2. **`PermissionGrant` / `GrantStatus` 骨架**（`permission.rs`）——wasm 轨真用得上，不重写。
3. **`PluginService` 的方法面**（全局 11 + 项目 6）——只换表，不换签名。

### 3.5 外部参考实现：Rdata-Sidecar（2026-09-20 读）

用户早前的 Go sidecar 原型（`github.com/ling-love-xin/Rdata-Sidecar`，13 篇文档 / 5268 行；已克隆到项目外 `../Rdata-Sidecar-ref` 供查阅）：
单二进制 + 进程内 HTTP JSON-RPC，8 个连接器（gaussdb / mysql / oceanbase / mssql / oracle / postgres / sqlite）+ JDBC 桥 + LSP 服务。
它在**传输与数据面上与我们的 D4/D5 正好相反**，但有几处机制值得抄，也有几处必须引以为戒。

**值得借鉴（带依据）**

| # | 它的做法 | 依据（文件:行） | 对应我们的 |
| --- | --- | --- | --- |
| 1 | **错误码按模块切号段**：标准 -327xx；JDBC -32000~-32019、LSP -32020~-32039、Connectors -32040~-32059，统一 `SidecarError{code,message,data}` | `internal/common/errors.go:10-39` | §4.2.2 的方法表。**建议**：扩展宿主与驱动各自的号段分开，别都挤在 -32001…-32008（两边会撞号） |
| 2 | **能力是数据，不是布尔**：`DriverMetadata{Capabilities, Priority, IsBuiltin, DriverType}`，`list_available` 一次返全表 | `internal/drivers/types.go:106-116`、`internal/connectorsvc/service.go:151-158` | D2 的判据来源；`driver.describe` 的返回形状 |
| 3 | **同 DBType 多驱动按 `Priority` 降级**：postgres-native=100 / mysql-jdbc=50 | `internal/drivers/manager.go:93-106`、`docs/jdbc-fallback-strategy.md` §2 | 「内置 → sidecar → wasm」降级链可直接用这个形状 |
| 4 | **模板方法基类**：`Connect` 固定 `buildDSN → ConfigurePool → Ping → Store`，子类只实现 `buildDSN` | `internal/drivers/base_driver.go:120-145` | 三层对象模型里 `DriverInstance` 的骨架 |
| 5 | **池参数按 DBType 查表并写清语义**：SQLite `MaxOpenConns:1`「不支持并发写入」，TestConnection 对它跳过 `ConnMaxLifetime` | `internal/pool/manager.go:49-56`、`base_driver.go:164-167` | `DriverCapability` 的**真门控**（并发写 / 事务） |
| 6 | **连接句柄**：UUID → `*sql.DB` 的 `ConnectionStore`，connectionID 跨 RPC 传 | `internal/drivers/pool.go:12-32` | Session 生命周期（我们多一层 `PluginProcess`） |
| 7 | **序列化集中且显式降级**：列类型取不到就退回列名；`[]byte` 文本→string、二进制→base64、time→RFC3339Nano | `internal/drivers/serializer.go` | D4 的类型映射要覆盖同一集合（**但我们额外要求 blob 不变 base64 字符串**） |
| 8 | **handler 注册表** `map[string]HandlerFunc` + `RegisterHandler` | `internal/server/server.go:21,41-46` | 方法表注册方式 |

**别学（同样带依据）**

| # | 它的坑 | 依据 |
| --- | --- | --- |
| 1 | **声明与实现不符**：`Capabilities` 里写了 `transactions`，但驱动接口没有 Begin/Commit/Rollback，全仓搜不到 `sql.Tx` | `internal/connectorapi/types.go:13-40` |
| 2 | **全量缓冲、无流式**：请求一次解码、响应一次编码，所有行读进 `[][]interface{}`；它自己的文档也写着 "No Streaming Responses" | `internal/server/server.go:93-98,132-134`、`docs/architecture.md` §12.1 |
| 3 | 错误分类靠**子串匹配**（`containsAny(errStr, {"connection","ping","dial"})`）；契约靠运行期 `panic` 兜底 | `internal/drivers/errors.go:31-40`、`base_driver.go:187` |
| 4 | 契约**重复两份**（`connectorapi/types.go` 与 `drivers/types.go` 逐字重复同一套接口） | 两文件 |
| 5 | **未实现的驱动仍被 `list_available` 当正常驱动返回**；`sanitizeURL` 只截断 50 字符、不真脱敏 | `internal/connectorsvc/service.go:41-105`、`internal/jdbc/service.go:152-158` |
| 6 | 测试空转：用零值 `&sql.DB{}` 调 `Store`；5 个测试文件均不连真实库 | `internal/drivers/pool_test.go:18` |
| 7 | 进程回收只管 SIGINT/SIGTERM，**无父进程死亡检测**（无看门狗） | `main.go:78-105` | ✅ 这正是 P1 验收里「宿主退出无孤儿进程」要补的那一项 |

**与 D1/D4/D5 的冲突（明确不采纳，并记下它的取舍）**

| 冲突 | 它的做法 | 它自己文档承认的代价 |
| --- | --- | --- |
| **D5 传输** | TCP + HTTP `POST /rpc`，端口由子进程 `stdout` 自报 | **零鉴权、假设本机通信可信**；分帧完全交给 HTTP/JSON，**装不下 Arrow** |
| **D4 数据面** | 零 Arrow、零二进制帧；`QueryResult.Rows [][]interface{}`，二进制降级成 base64 字符串 | 大结果集与 blob 都吃亏（正是我们把 P2.5 提前的理由） |
| **D1 隔离** | 单进程多驱动（8 个驱动同进程），隔离只到「sidecar vs 主进程」一层 | 一个驱动阻塞会拖垮同进程的 JDBC / LSP；也没有 `PluginProcess → DriverInstance → Session` |
| **D2 元数据** | 只有 `ListTables(schema)` + `GetTableSchema`；postgres 把 schema 硬编码 `public` | 够不上「全量元数据浏览」，导航树建不起来 |

一句话：**它把“跟 Rust 说话”这件事做通了（值得参考），把“把数据搬对”这件事跳过了（我们全部重做）。**

---

## 4. 目标形态

### 4.1 三层对象模型

```
PluginProcess              一个可执行文件的一个运行实例（按 plugin_id 去重、复用）
  └─ DriverInstance        该进程内注册的一个 driver id（一个插件可有多个 driver）
       └─ Session          一次连接 = 进程内一个会话句柄（session_id）
```

| # | 规则 |
| --- | --- |
| 1 | 进程按 `plugin_id` 去重，**不是每连接一进程**；`max_instances` 由清单声明（默认 1），需要并行时按上限起多实例、session 轮询分配。**两句话要合起来读**（实现口径，`lifecycle::Registry::acquire`）：并行 → 一条进程能服务多会话，**共享**（挑会话最少的）；串行 → 一条进程一次只服务一个会话，**空着的直接用、都忙且还有额度就起新实例、额度到顶才排队** |
| 2 | 一个插件 = 一个进程 = 一个或多个 driver（由 `contributes.drivers` 决定） |
| 3 | 并发由清单声明：`concurrency = "serial"｜"parallel"`（家在 `[capabilities.driver]`，§4.3.1）。`serial` 时宿主在该进程上排队（JDBC 单 `Connection` 属此类）；**排队项连 driver 一起记** —— 一个插件声明多个驱动是常态，放行时才知道该开哪一个 |
| 4 | 空闲回收与 `ConnectionManager` 对齐（30min，`connection_manager.rs:765-826`）；进程退出 → 全部 session 失效 → **如实报错，不静默重连** |
| 5 | 崩溃检测走 `session.ping`（默认 30s 间隔，连续 2 次失败 → 标记 `Error` + 通知 + 允许手动重启） |

### 4.2 传输与协议

#### 4.2.1 帧格式

```
Frame = [u32 BE total_len][u8 kind][payload]
  kind 0x01 = JSON-RPC 2.0 消息（UTF-8）
  kind 0x02 = Arrow IPC stream 分片
```

响应若声明附件，则其后紧跟对应数量的 `0x02` 帧：

```jsonc
{"jsonrpc":"2.0","id":7,"result":{
  "columns":[{"name":"amount","type_raw":"numeric(38,10)","nullable":true}],
  "row_count":1000, "affected_rows":null,
  "has_more":true, "cursor_id":"c-1", "truncated":false,
  "attachments":[{"id":0,"kind":"arrow-ipc-stream","frames":1}],
  "notices":[]
}}
```

对照：DBX 的 `stdio-framed` 是 5 字节头（1 字节 kind + 4 字节长度）上限 64 MiB，思路一致且已有上线验证。

**两处原文留白的口径**（实现时才暴露，已写进 `crates/plugin/src/sidecar/proto.rs`）：

| 项 | 取值 | 理由 |
| --- | --- | --- |
| `total_len` | **含头 5 字节**的整帧长度（= `5 + payload.len()`） | 取 "total" 的字面含义。两种读法都能自洽，但差 5 字节会把整条流**永久解错位**，所以必须写死 |
| 单帧上限 | **64 MiB**，超过即拒；且**先校验声明长度、再分配载荷缓冲** | 长度前缀来自对端：不设限等于让对方用一个 `u32::MAX` 让我们预分配 4 GiB |

**孤儿进程防线（协议级，P1 验收项）**：宿主与 sidecar 之间那条 **stdin 管道由宿主持有**；
**sidecar 必须把 stdin EOF 当作「宿主已死，立即退出」**。这样宿主无论怎么死（崩溃、被 kill、
任务管理器结束进程），管道都会关闭 → sidecar 自己退出，**不需要在宿主侧做平台相关的“杀子进程组”**
（Windows Job Object / Linux PDEATHSIG 都不必）。
参考实现（Rdata-Sidecar）只处理了 SIGINT/SIGTERM、没有父进程死亡检测 —— 这正是我们要补的那一项（§3.5）。

**附件语义的实现口径**（`crates/plugin/src/sidecar/router.rs`；两个方向同一条规则：宿主侧 `Router` 消费帧、sidecar 侧 `encode_response_with_arrow` 切帧，两者互为逆运算，有往返测试）：

| # | 口径 | 不这么做会怎样 |
| --- | --- | --- |
| 1 | **响应在附件收齐之前不交付**，但“在飞”的定义要跟着改：**被附件扣住的响应仍算在飞** | 若一收到响应就把 id 移出在飞集合，连接断掉时那个 id 交还不了 → 调用方**永远等一个不会来的响应** |
| 2 | `frames: 0` 的声明必须**立即**交付（空结果集仍有 schema） | 若靠“下一帧到来”触发推进，就永远不会完成 |
| 3 | 任何错位都要产出 `Issue` / `UnknownError`，不 panic 也不静默丢 | 孤儿附件帧、等附件时先来 JSON 帧、未知 `kind`、未知错误码、超时后姗姗来迟的响应 —— 这些正是最难查的一类 bug |

#### 4.2.1.1 混合的三层，代价差一个量级

JSON 与 Arrow 必然共存（控制面 vs 数据面）。区别在**混在哪一层**：

| 混的层次 | 做法 | 代价 |
| --- | --- | --- |
| **字节流层**（本方案） | 同一管道，用 1 字节 `kind` 区分；各自保持原生格式 | **零额外编码**；可分别 dump（JSON 帧直接可读，Arrow 帧落 `.arrow` 用 `pyarrow` 打开） |
| 消息层 | Arrow 转 base64 塞进 JSON 字段 | **+33% 体积**；构造期内存峰值双份（原始 bytes + base64 串）；不可流式；出问题要同时排查两层编码 |
| 结构层 | 一个 struct 里既有 JSON 字段又有 arrow 字段 | 不可行：序列化契约只能二选一（`QueryResult` 的 `Serialize` 排除 `batches` 即此类约束的体现） |

#### 4.2.1.2 阈值：小结果不走 IPC

Arrow 编解码不是免费的，小结果（元数据、DDL、`SELECT 1`、属性面板探针）用 JSON 更划算：

```
inline_json_max_rows  = 200
inline_json_max_bytes = 256 KiB
```

规则：满足两者 → 结果以 `rows` 字段内联（JSON）；否则走 `attachments` + Arrow 附件。
上限只影响**线格式**，不影响上层：`ResultSet` 同时提供 `from_batches` 与 `from_json_rows`（§4.5.3），**承载方式对消费方不可见**。

#### 4.2.2 RPC 方法表

**握手与进程**
```
initialize({protocol, host:{name,version}, plugin_config}) -> {protocol, driver_ids[], runtime{}}
shutdown() -> {}
ping() -> {pong}
```

**驱动与会话**
```
driver.describe(driver_id) -> DriverDescriptor
session.open({driver_id, params}) -> {session_id, server_version}
session.close({session_id})
session.ping({session_id})
```

**查询与游标**
```
query.execute({session_id, sql, params?, max_rows, timeout_ms}) -> QueryPage
query.fetch({session_id, cursor_id, offset, limit}) -> QueryPage
query.cancel({session_id, request_id})
```

**事务**
```
tx.begin({session_id}) -> {tx_id}
tx.commit({tx_id})
tx.rollback({tx_id})
```

**元数据**
```
meta.catalogs({session_id}) -> [string]
meta.schemas({session_id, catalog}) -> [SchemaInfo]
meta.objects({session_id, catalog, schema}) -> [ObjectInfo]      // 必须一次给全，含 kind
meta.object_detail({session_id, catalog, schema, object}) -> ObjectDetail
meta.routine_source({session_id, catalog, schema, name}) -> string?
```

**通知（插件 → 宿主）**
```
log({level, message})
progress({request_id, phase, done, total})
```

**错误码**（挂在 JSON-RPC error 之上）

```
-32001 driver_not_supported      -32002 session_not_found
-32003 sql_error {code, sqlstate, position}
-32004 cancelled                 -32005 timeout
-32006 capability_denied         -32007 protocol_version_mismatch
-32008 resource_limit            -32009 connect_failed
```

> `capability_denied` 单独成码：让「驱动不支持 X」与「出错了」在 UI 上能分开呈现。

**方法表的实现口径**（P1 落地，`crates/plugin/src/sidecar/supervisor.rs`）：

| 项 | 口径 | 理由 |
| --- | --- | --- |
| `session_id` 谁发号 | **宿主生成**，在 `session.open({session_id, driver_id, params})` 里带下去，sidecar 回声 | 会话句柄是宿主的资源名；否则一个会话要维护两张 id 表（宿主一张、对端一张），迟早对不上 |
| 进程存活探针 | 用**进程级** `ping()`，不是 `session.ping` | 空闲实例没有会话，而它恰恰是最需要探活的那种（上面规则 5 原文写的是 `session.ping`，按本口径订正） |
| 连接参数怎么传 | `session.open` 的 `params` 是**嵌套**的驱动参数（`{dsn, user, password…}`），与 `session_id`/`driver_id` 平级 | 驱动参数是「这个驱动自己的事」，不该与协议字段混在一层 |
| `request_id` 谁发号 | **宿主生成**，在 `query.execute` 与 `query.fetch` 里带下去，`query.cancel` 用它取消 | 与 `session_id` 同一条理由：句柄是宿主的资源名，对端只能回声 |
| 内联 JSON 里的时间 | 用 **RFC3339 字符串**（如 `2023-11-14T22:13:20.000001Z`）；Arrow 侧才是 `Timestamp(us, UTC)` | JSON 没有时间类型；两条承载方式的行值语义一致，但**线表示**必然不同 —— 消费方按 `PageData` 的形态取值 |
| `rows` / `attachments` 谁优先 | 有附件就以附件为准（`attachments` 是数据面）；两者都没有 = 空结果集，**但 `columns` 必须在** | 「空结果集仍有 schema」是硬口径：少了 schema，网格与类型映射都无从下手 |
| 内联行是什么形状 | **对象**，列名做键（`[{"id":1,…}]`）；**位置数组不接受**（当场报 `Protocol`） | 位置数组要调用方自己维护列序，错一列就静默串值；对象自描述、缺键按 null |
| 内联行的类型 | 宿主**按 JSON 值的形状**逐列推断（全整数→Int64、全数值→Float64、全布尔→Boolean，其余→LargeUtf8）；同列混排退化成文本 | 内联只带 `type_raw` 文本，不带宽带类型信息；**带宽带类型的是 Arrow 那条路**（schema 带 `rds.*`）。宁可显示成文本，也不要静默丢值 |
| 取消是真取消还是关连接 | **真取消**：宿主把 `query.cancel` 递到驱动进程，由它中断查询并回 `-32004`，宿主继续等这次调用收场 | 把 future 丢掉只是这边不等了，对端那条查询还在跑（原生驱动现在就是这么干的）；这恰是 sidecar 轨的优势之一（§3.2） |
| 连接配置怎么传 | 宿主把 `DriverConnectionConfig` **原样序列化**成 `session.open` 的 `params`（主机 / 端口 / 库 / 用户 / 口令 / 文件路径 / 连接方式 / 选项 / 超时 / 驱动属性都在里面） | 别在中间再做一次有损翻译：插件要的就是宿主收集到的那份，怎么用到它自己的库里由它决定 |
| 表单字段谁定 | P1 由宿主给**标准五件套 + 文件路径**，且**一律不标必填**；清单里的 `connection_schema` 还没翻成 `DriverField`（P2 做） | 宿主并不知道某个驱动到底需要哪几项；标了必填反而挡住合法配置（靠 options 拼 DSN 的驱动）。宁可让插件报错 —— 它比宿主清楚 |
| 串行驱动排队的上限 | 第二条连接最多等 **30s**（`DEFAULT_QUEUE_WAIT`），等不到就**撤掉排队并如实报超时** | 不能让用户挂在一个永远不会来的连接上；撤排队这条同时要求内核能撤掉「还在队列里」的会话（P1 修过：`Registry::release` 会从队列里摘掉它） |
| 句柄丢掉 = 关连接 | `SidecarDatabase` 带 `owner` 时在 `Drop` 里把会话交还内核（`tokio::spawn` 一个关闭任务）；不带 `owner` 的是借用式用法，由调用方安排 | 会话是宿主的资源：不交还就会一直占着那个串行实例，后续连接全排住 | SQL 错 → `DatabaseError::Query`（`position` 由 1 基**字符**换算成 0 基**字节**）；断线 → `ConnectionError::Network`；缺能力 → `CommonError::NotSupported`；对端说话不算话 → `PluginError::ExecutionFailed` | UI 要能按域分流：SQL 错跳光标、缺能力置灰、断线提示重连，不能都堆成一句「查询失败」 |
| 连接失败 | ✅ **P2 已补一等码 `-32009 connect_failed`**，落 `ConnectionError::Refused`；细因（拒连 / 口令 / TLS）由驱动写在 `message` 里，**宿主不按子串猜**分类 | 连接失败是驱动最常见的失败，界面上的动作与 SQL 错不同（改配置 / 重连 vs 改 SQL）。参考实现就是靠 `containsAny` 猜的（§3.5），那套我们不要 |

#### 4.2.2.1 `meta.*` 的线格式（P2 落地，2026-09-20）

| 方法 | 请求 | 返回 |
| --- | --- | --- |
| `meta.catalogs` | `{session_id}` | `{catalogs: [string]}` |
| `meta.schemas` | `{session_id, catalog}` | `{schemas: [{name, comment?}]}` |
| `meta.objects` | `{session_id, catalog, schema}` | `{objects: [{name, kind, comment?, parent?}]}` |
| `meta.object_detail` | `{session_id, catalog, schema, object}` | `{object: {name, kind}, columns: [ColumnMeta], indexes?, row_count?}` |
| `meta.routine_source` | `{session_id, catalog, schema, name}` | 字符串 或 `null` |

`kind` 取值：`table` / `view` / `materialized_view` / `procedure` / `function` / `sequence` / `trigger`。
`ColumnMeta`：`{name, type_raw, canonical?, nullable?, is_pk?, is_fk?, default?, format?, comment?}`。

| 口径 | 依据 |
| --- | --- |
| `meta.objects` **一次给全、含 `kind`** | 五个文件夹（表 / 视图 / 例程 / 序列 / 触发器）共用这一次内省；按文件夹分家就要跑五遍，而它们在内省层面本来就是一条查询 |
| 认不出的 `kind` **不丢也不摆** | 解出来（`MetaObjectKind::Other`）但跳过，并记一条带对象名的日志：摆进树会被画成「表」（假信息），直接丢掉则没人知道驱动报了新东西 |
| 物化视图归到**视图**文件夹 | 导航只有五处（`NavFolder::ALL`），单开一层要多动导航与定位两处；原始类别在 `kind` 里没丢（属性面板说实话） |
| 形状严格 | 缺 `catalogs` / `objects` / `columns`、元素不是对象、`name` 为空 —— 一律协议错并点名哪个字段。「空」与「对端没说」是两回事 |
| 列少了 `type_raw` 退化成 `unknown` | 属性面板少一列类型，好过整张详情打不开（与 `parse_page` 同一条口径） |
| `routine_source` 只有两种形状 | 字符串或 `null`。包一层对象看着无害，但多一种形状就是两份契约（§3.5） |
| 元数据调用**不收附件** | 元数据是控制面；带 Arrow 附件说明对端把两条路搅在一起了，报协议错而不是猜它想说什么 |
| 能力为假 → 方法回 `-32006`、那类对象**不报** | 「驱动看不到」与「没有」在驱动侧是同一件事；宿主侧的门控口径见 §4.4 |
| 超时用 `DESCRIBE_RPC_TIMEOUT`（20s） | 内省该是快的：慢到超时说明对端那条内省查询有问题，该如实报出来，而不是让界面按查询的超时（600s）一直转圈 |
| **没有 schema 层时，宿主把 catalog 当 schema 传** | 导航在 `has_schema_level` 为 false 时走 `load_folders(conn_id, catalog, catalog)`：`meta.objects` 收到的 `schema` 就是 catalog 名（与 MySQL 原生驱动 `get_tables(catalog, catalog)` 同一个做法）。驱动对此**不必特判**：认它就是 schema 名即可 |
| schema 没给时的取值顺序 | 显式给的 → `driver.describe` 的 `default_schema` → **空串**。空串是有含义的：「没有 schema 层，用你自己的默认」，宿主不替驱动猜一个 schema 名 |
| 索引 / 约束**明细**暂无协议面 | `object_detail` 只给个数（`indexes`）；属性面板要列明细时再补 `meta.indexes` / `meta.constraints`（P4）。在那之前 `Database::list_indexes` 如实报不支持 |
#### 4.2.3 版本闸

`initialize` 交换 `protocol = "rds-driver/1"`；不匹配则**拒绝加载，并同时报出两个版本号**（对照 Zed 的 `zed:api-version` 做法：`extension_api/wit/since_v0.0.x` 目录冻结 + 加载期拒绝）。

#### 4.2.4 Arrow IPC 兼容子集（硬性，写进协议版本）

| 项 | 约束 |
| --- | --- |
| 字节序 | 仅 little-endian |
| offset 宽度 | 64-bit（`LargeUtf8`/`LargeBinary`），避开 2 GB 边界 |
| 压缩 | P1 不压缩；`lz4`/`zstd`（IPC body compression）留 P4 |
| dictionary encoding | 允许（字符串字典常见且省内存） |
| 嵌套类型 | `List`/`Struct`/`Map` P1 转 JSON 字符串，避免跨语言语义漂移 |
| 扩展类型 | 禁用（extension types 跨实现不一致） |
| 时间类型 | 归一化到 `Timestamp(us, UTC)` 或 `Date32`；厂商时区写进 `rds.type_raw` |
| 每页 | 一个**自洽** IPC stream（自带 schema），换取无状态 |

**为什么选 Arrow IPC 而不是自研结构体**：IPC 是独立稳定格式，**与 Rust crate 版本解绑**。`arrow = "58.4.0"` 被 duckdb 锁定（根 `Cargo.toml` 注释：进程内两端传 `RecordBatch` 必须同一 arrow）；跨进程只共享 IPC 格式，因此 Python `pyarrow`、Go `arrow-go` 各用各的版本均可互通。

### 4.3 清单契约（`PluginManifest` 演进）

沿用现有结构，新增 `[backend]` 段：

```toml
schema_version = 1

[plugin]
id = "com.example.sqlserver"
name = "SQL Server Driver"
version = "1.0.0"
publisher = "example"
engines.rdatastation = ">=0.1, <0.2"

[backend]
kind         = "sidecar"              # sidecar | wasm | script
executable   = "bin/agent"            # 相对插件目录
interpreter  = "python3"              # 可选（script 型）
transport    = "framed"              # framed | jsonl（P1 起仅 framed）
protocol     = "rds-driver/1"
platforms    = ["linux-x64","darwin-arm64","darwin-x64","win-x64"]
max_instances = 1

[backend.wasm]                        # 原 capabilities.wasm 并入
entry = "plugin.wasm"
max_memory_mb = 256
max_cpu_time_ms = 5000
allowed_host_functions = ["kv.get","kv.set","duckdb.read_only"]

[[contributes.drivers]]
id = "mssql"  display_name = "SQL Server"  default_port = 1433  connection_schema = "schemas/mssql.json"

[[contributes.commands]]  id = "mssql.explain"  title = "查看执行计划"  category = "SQL Server"
[[contributes.panels]]    id = "mssql.sessions"  title = "会话"  location = "right"  order = 100
[[contributes.settings]]  key = "mssql.poolMax"  type = "number"  default = 8  label = "连接池上限"
```

**版本闸必须真的拒绝**：`manifest.rs` 已有 `check_engine_compatibility`，但要在**加载与安装两处都强制**（对照 Tabularis 的 `min_runtime_version`）。

#### 4.3.1 `[backend]` 的实现口径（已落地，2026-09-20）

落在 `crates/plugin/src/manifest.rs`（`PluginBackend` / `BackendKind` / `BackendTransport`；
`PluginManifest::process_spec()` 把它接成 `lifecycle::ProcessSpec`）：

| 项 | 口径 | 理由 |
| --- | --- | --- |
| `concurrency` 的家 | **`[capabilities.driver].concurrency`**，`[backend]` 里**不放**第二份 | 只留一处：参考实现就是因为契约重复两份而两边跑偏（§3.5） |
| `[backend.wasm]` | **未实现**：wasm 入口暂仍读 `capabilities.wasm`；声明 `kind = "wasm"` 时校验它必须在 | 并入是 P3 收 wasm 时的迁移；现在两处都写才是真的乱 |
| `platforms` 为空 | 空 = **未声明**，不阻挡（视为跨平台） | 向后兼容已有清单；真正的闸在 P5 安装期（缺平台置灰） |
| `transport = "jsonl"` | **保留取值，但运行期明确拒绘**（提示「P1 只支持 framed」） | 「不得静默退化」：老形态清单要在起进程前得到可读原因 |
| 路径安全 | `executable` 必须是不含 `..` / 根 / 盘符的**相对路径**，**校验期**就挡 | 第三方清单给的字符串，拼路径前先过白名单（与 `validate_plugin_id` 同族） |
| Windows 后缀 | 清单写 `bin/agent` 且**只有** `bin/agent.exe` 存在时自动补全 | 清单不必为平台分叉；真找不到时报错里显示的是原样路径 |
| `kind = "script"` | 解析、进同一个进程池（`interpreter` + 脚本路径）；**端到端未接线** | 它本来就是子进程；但 Python / Jupyter 那条线在 P1 之后 |
| 「至少一种形态」 | `[backend]` **也算一种**（不再强制 `capabilities.frontend/wasm`） | 驱动插件本来就没有前端与 wasm |

### 4.4 能力矩阵（能力缺失必须有处表达）

清单 `[capabilities.driver]`（**P4 落，见 §4.4.1 末尾**），运行时的那一份是 `driver.describe` 回的 `capabilities`：

```toml
[capabilities.driver]
schemas = true          views = true             materialized_views = false
routines = true         sequences = false        triggers = true
indexes = true          constraints = true       comments = true
transactions = true     cancel = true            cursor = true
streaming = false       explain = true           affected_rows = true
readonly = false        concurrency = "serial"
identifier_quote = "\"" default_schema = "public"
```

**门控规则**：`false` 的能力 → 相关 UI 置灰 + 给出原因 + 对应 RPC 宿主不再调用。
**不得静默退化**：缺失能力造成的功能不可用，必须走 `capability_denied` 并可展示。

#### 4.4.1 运行时那一份怎么门控（P2 落地，2026-09-20）

| 能力 | 门控点 | 效果 |
| --- | --- | --- |
| `cancel` | `SidecarDatabase::query_with_cancel` | **令牌响了才判**（查询照跑，不能因为不能中断就不让人家查）：没声明就**不装中断**，继续等语句收场后如实报「没能停下来，结果按未中断丢弃」 |
| `transactions` | `begin_transaction` | 两句不同的话：「没声明 transactions 能力」（驱动的事）与「声明了但宿主桥还没接」（我们的事）——下一步不一样 |
| `schemas` | `MetadataBrowser::has_schema_level` | false → 导航不给 schema 层（catalog 直接挂五个文件夹），`get_schemas` 返回**空**而不是回退成 catalog 列表（否则出现同名重复层） |
| `affected_rows` | —— | 暂不门控：`QueryResult.affected_rows` 本来就是 `Option`，驱动没报就是 `None`，没有「假装」的余地 |
| `views` / `routines` / `sequences` / `triggers` | **驱动侧** | 没声明就不在 `meta.objects` 里报那类对象（「看不到」与「没有」在驱动侧是同一件事）；宿主侧不做二次过滤（那会变成两份真相） |
| `cursor` / `streaming` / `explain` / `readonly` / `comments` | —— | 协议面还没接，暂不门控；接了再补（`cursor` 会落在 `query.fetch` 那条路上） |
| `indexes` / `constraints` | —— | 连协议面都还没定：`object_detail` 只给个数，**明细**一律如实报不支持（`NotSupported`），不返回空 |

**清单 `[capabilities.driver]` 那一份放到 P4**（连同连接对话框一起落）。理由：现在落它就会是
**没有读者的第二份真相**，而「同一件事写两处」正是 §3.5 的教训。P2 的门控一律以 `driver.describe`
的**运行时**那份为准（清单可以撒谎，跑起来的进程没法撒谎）；P4 补清单字段时，要在
`describe` 与清单不一致时记一条告警（部署期已经有一套同样的「清单自相矛盾要报出来」的做法，
见 `Deployment`）。
### 4.5 Arrow 与类型

#### 4.5.1 schema metadata（类型映射的落点）

| key | 例 | 用途 |
| --- | --- | --- |
| `rds.type_raw` | `numeric(38,10)` | 原始类型名（属性面板、用户可见） |
| `rds.canonical` | `DECIMAL` | 归一化类型（喂 mock / insight / 质量分；`mock/src/schema_map.rs` 只认首词，归一化后不再落 Text） |
| `rds.nullable` / `rds.is_pk` / `rds.is_fk` | `true` | 现有导航/属性面板字段 |
| `rds.format` | `decimal(scale=10)` | 单元格格式化提示 |
| `rds.comment` | `订单金额` | 列注释 |

#### 4.5.2 二次分析桥改 Arrow 直灌（**最高 ROI，建议最先做**）

| | 现在 | 目标 |
| --- | --- | --- |
| 载荷 | `AnalysisRequest.rows: Vec<Vec<String>>` | `Arc<ResultSet>` |
| 建表 | `CREATE TABLE` + 逐行 `INSERT INTO … VALUES (?…)`（`duckdb_service.rs:38-101`） | `register_record_batch` / `Appender` 直灌 |
| 类型 | `infer_type` 从字符串猜 | Arrow schema 直接给 |
| 回程 | JSON → 字符串 | Arrow |

改动局部（`analysis.rs` 载荷 + `duckdb_service.rs` + `editor_exec.rs:641-669`），但直接兑现 README 卖点。

#### 4.5.3 `ResultSet` 薄封装（`editor` 不直接依赖 arrow）

```rust
// shared/src/result_set.rs（新增）
pub struct ColumnMeta { pub name: String, pub type_raw: String, pub canonical: String,
                        pub nullable: bool, pub format: Option<String> }

pub struct ResultSet {
    batches: Vec<ArrowBatch>,     // 权威数据，不对外暴露 arrow 类型
    columns: Vec<ColumnMeta>,
    pub total_rows: u32,
    pub truncated: bool,
    pub has_more: bool,
}

impl ResultSet {
    /// 两种来源：线格式走 IPC 或内联 JSON 对上层不可见（§4.2.1.2）
    pub fn from_batches(batches: Vec<ArrowBatch>, columns: Vec<ColumnMeta>) -> Self;
    pub fn from_json_rows(columns: Vec<ColumnMeta>, rows: Vec<Vec<Value>>) -> Self;

    pub fn cell_text(&self, row: usize, col: usize) -> Cow<'_, str>;   // 惰性格式化
    pub fn compare(&self, col: usize, a: usize, b: usize) -> Ordering; // 类型化比较
    pub fn matches(&self, row: usize, filter: &Filter) -> bool;
    pub fn column(&self, col: usize) -> &ColumnMeta;
}
```

`QueryData` 相应变为：

```rust
pub struct QueryData {
    pub result: Arc<ResultSet>,   // 替代 columns/rows(Vec<Vec<String>>)
    pub elapsed_ms: u64,
    pub affected_rows: Option<u32>,
    pub notice: Option<String>,
}
```

**四个收益**：可见区惰性格式化（现在 1000 行全量 `to_string()`）· 类型正确排序（替掉 `parse::<f64>()`）· 三处类型退化消失（含 blob 不再变 `[137, 80, …]`）· `column_types` 不再丢（「洞察此列」能拿到真类型）。

### 4.6 权限与信任（双轨语义必须不同）

#### 4.6.1 对照

| | wasm 轨 | sidecar 轨 |
| --- | --- | --- |
| 能否强制沙箱 | ✅（无 host function 即无能力） | ❌ 原生进程权限等同宿主 |
| `permissions` 字段作用 | **门控** | **仅展示与告警，不作为门控** |
| 信任来源 | 沙箱 + 清单声明 ∩ 宿主授予 | **安装时的显式信任动作 + 包签名 + 来源标注** |
| 路径约束 | 只开 `plugin-data/<id>` / `plugin-cache/<id>`，**强制** | 只能约定 |

`PermissionType` 增加 `Sidecar` / `Driver` 变体（现仅 `Frontend`/`Wasm`）。→ ✅ **P0 已落地**，并顺手把语义钉住：`PermissionType::is_gating()` 只有 `Frontend`/`Wasm` 返回 true；清单侧加了 `permissions.sidecar` / `permissions.driver` / `permissions.credentials`；`validate_permissions()` **只看门控轨**（否则会把“安装时授权”这条绕过去）。

#### 4.6.2 wasm 真限额（现为零）

落到 Extism/wasmtime：`max_cpu_time_ms` → epoch/fuel + 超时中断；`max_memory_mb` → 限额；显式 cache 目录 = `plugin_cache_dir(id)`；删除自报自检的 `ResourceLimits`。

#### 4.6.3 凭据口径（必须写明）

sidecar 是原生进程，**必然自己连库**。故：

| 选项 | 判断 |
| --- | --- |
| 宿主代连、sidecar 不碰网络 | ❌ 不现实（就是个 JDBC 进程） |
| 宿主注入短期凭据 | ❌ 数据库协议无通用方案 |
| **宿主把凭据交给 sidecar，且要求不落盘** | ✅ **采用**（Tabularis/DBX 同） |

配套三条：清单 `permissions.credentials = "database"` 明示 · 安装页单独一行「该插件会收到你的数据库密码」· 日志记录「凭据已交付插件 X / 时间」。

**口径**：sidecar 轨只保证「凭据不出本机」，**不保证「凭据不出宿主进程」**；wasm 轨才保证后者。

### 4.7 路径与沙箱

`paths` 新增（并扩展 `ensure_dirs()`）：

```rust
pub fn plugins_dir()        -> PathBuf  // <RDS_HOME>/plugins
pub fn plugin_dir(id)       -> PathBuf  // plugins/<id>
pub fn plugin_data_dir(id)  -> PathBuf  // plugin-data/<id>
pub fn plugin_cache_dir(id) -> PathBuf  // plugin-cache/<id>（wasmtime 缓存 / sidecar 日志）
pub fn sidecar_work_dir(id) -> PathBuf  // tmp/sidecar/<id>（子进程 current_dir）
pub fn plugin_registry_dir()-> PathBuf  // plugins/.registry（分发包缓存 + 清单索引）
```

**symlink 防御**：解析器返回**打开目录的句柄**而非路径；Linux 用 `openat2(RESOLVE_BENEATH)`（`cap-std`），其他平台逐段 `openat`。**禁止 check-then-use**——gpui-kit 文档记录过两次失败的尝试（字符串比较漏整条 link；字符串比较 + canonicalize 抓不住 check 与 syscall 之间植入的 link）。

### 4.8 生命周期与项目引用对账

```
应用级（安装）   install → plugins/<id>/ + plugins 表
                 ↓
项目级（引用）   打开项目 → 读 project_resources → 与 plugins 表对账
                 · 已装 + enabled   → 纳入激活集
                 · 已装 + disabled  → 不激活（文案「项目已禁用」）
                 · 未装             → 记「缺失引用」，★不阻断打开★
                 ↓
会话态（激活）   用到时按需激活（连接建立 / 面板打开 / 命令调用）
关闭项目         失活（wasm 实例 / sidecar 进程 / 端口 / 临时目录回收）
```

**表（待 Q1 拍板）**：

```sql
-- crates/engine/migrations/project_meta/026_project_resources.sql
CREATE TABLE IF NOT EXISTS project_resources (
    kind        TEXT NOT NULL,      -- 'plugin' | 'engine_extension'
    id          TEXT NOT NULL,
    version     TEXT,               -- 插件为语义化范围；扩展为空 = 跟内核
    enabled     INTEGER NOT NULL DEFAULT 1,
    enforce     INTEGER NOT NULL DEFAULT 1,   -- 由 project_used_plugins.required 迁移
    config_json TEXT,
    added_at    TEXT NOT NULL,
    PRIMARY KEY (kind, id)
);
```

迁移成本低：现有 6 个持久化方法 + 6 个 service 方法**均无生产调用方**。

### 4.9 面板 / 命令 / 设置（宿主注册表）

| 主题 | 现状 | 目标 |
| --- | --- | --- |
| 面板 | `LeftPanel`/`RightPanel` 硬编码枚举 + `const ALL`（`workbench_shell/src/model.rs:22-63`） | `PanelId(String)` + `PanelRegistry`；**未注册 id 渲染占位面板**（文案：「项目引用了 X，未安装」+ 安装入口）。`render` 仍是模式权威同步点 |
| 命令 | `CommandSpec` 内联在 `quick_open/commands.rs:79-85` | 提为 `CommandRegistry`（内置 + 插件贡献合并），Quick Open `>` 前缀消费 |
| 设置 | `settings` 已有「登记表 + 准入五条」 | 插件设置 = 带作用域的**外部登记项**，宿主渲染表单（对齐 Tabularis 的 `settings[]`） |
| 不变式 | `render` 纯读路径 | **保持**：三类贡献的 UI 全部由宿主渲染 |

---

## 5. 阶段任务

| Phase | 目标 | 主要落点 | 验收 |
| --- | --- | --- | --- |
| **P0 地基**<br>~1 周 | 契约与路径就位，不动传输 | `paths` 新增 6 函数 + 测试隔离；`PermissionType` 加 `Sidecar`/`Driver`；**决定** `sidecar/driver.rs` 与 `storage.rs` 的去留（补声明或删除）；4 个空壳 host function 删除或标注；修 `client.rs:127-155` 反向判据；文档修订（§10） | `cargo check-all` 绿；`paths` 单测；文档与代码一致 |
| **P1 sidecar 端到端**<br>~2 周 | 一个真实驱动跑通一次查询 | 三层对象模型；**二进制分帧 + Arrow IPC**；`initialize`/`driver.describe`/`session.open`/`query.execute`/`query.cancel`；`SidecarManager` 补 `current_dir`/日志/进程组回收；**用 PostgreSQL 包一层 sidecar 作靶子** | 能连 → 能查 3000 行（Arrow 到宿主）→ 能取消 → 宿主退出无孤儿进程（✅ 已固化为 `tests/sidecar_conformance.rs`：换 `RDS_SIDECAR_BIN` 可对任意 sidecar 复跑） |
| **P2 元数据与类型**<br>~2 周 | 驱动进入导航与搜索 | `meta.*` 五件必需；`DriverCapability` 门控；schema metadata 承载类型映射 | 契约层（自动化，`crates/plugin/tests/`）：五个文件夹各挑各的类别、表/视图按 `kind` 分两半、列带原始+归一化类型、触发器带所属表、缺 schema 落到 `default_schema`、索引/约束明细如实报不支持。**导航层（`NavigatorService` → `NavView`）要手动走一遍**：目前没有任何 crate 依赖 `rds-plugin`（第一条消费边留给 P4 的注册表接线），且靶子二进制只对本包测试可见（`CARGO_BIN_EXE_*`），跨 crate 拿不到 —— 详见 §11 P2 行 |
| **P2.5 二次分析直灌**<br>~1 周 | 兑现卖点 | `analysis.rs` 载荷 + `duckdb_service.rs:38-101` + `editor_exec.rs:641-669` 改 Arrow 直灌 | 结果集进 DuckDB 不再逐行 INSERT；类型保真往返 |
| **P3 wasm 轨收紧**<br>~1 周 | 「轻量」做成真的轻量且不可卡死 | Extism fuel/epoch 限额；显式 cache 目录；删自报自检的 `ResourceLimits`；host function 面收敛（Q4） | 死循环插件在 `max_cpu_time_ms` 内被中断且宿主不卡；`plugin-cache/<id>` 有产物；`~/.cache` 无新增 |
| **P4 注册表 + 引用 + 网格**<br>~2 周 | 四类扩展点全部有稳定宿主接口 | `PanelRegistry`/`CommandRegistry`/设置外部登记项；`project_resources` 迁移 + 对账 + 占位面板；`ResultSet` 接网格（渲染/筛选/排序/导出/复制） | 卸载插件后打开引用它的项目：**不阻断**，占位给出安装入口；内置面板行为不变；契约测试（零裸尺寸/零裸色值）全绿 |
| **P5 分发与签名**<br>~1 周 | 装上别人的插件不靠人品 | 包格式 zip（`manifest.toml` + `checksums.json` + `signature.json`）；Ed25519 + **逐资源 SHA-256 流式校验**；平台矩阵（缺平台置灰） | 篡改一字节 → 拒绝安装并说明；缺平台 → 如实「不支持」 |

**顺序理由**：P0→P1→P2 完成后即有第 7 个驱动，且契约经一次真实校验；P2.5 改动局部、收益最大，故提前；P4 是**唯一动布局状态机**的阶段，放最后。

### 5.1 P2.5 展开（Arrow 直灌）— 可执行改动清单

**目标**：结果集 → 本地 DuckDB 这条路，数据只过 Arrow 一次，**类型由 schema 决定而不是猜**。

**为什么是这条路**（三条事实，均已实测）：

1. **现状类型只有四档**：`infer_type` 只产 `BIGINT`/`DOUBLE`/`BOOLEAN`/`VARCHAR`（`engine/src/services/duckdb_service.rs:194`）——没有 DATE/TIMESTAMP/DECIMAL。日期落 VARCHAR、小数落 DOUBLE 或 VARCHAR。**这是可量化的失真，比「慢」更有说服力。**
2. **回程也是 JSON**：`query_duckdb` 返回 `(Vec<String>, Vec<Vec<serde_json::Value>>)`。
3. **逐行 INSERT 有两处同款**：结果集/导出（`duckdb_service.rs:38-101`）与洞察（`duckdb/analysis.rs:41-95` + `insert_rows:226-256`），共用 `infer_type` / `json_to_duckdb_value`。

**项目自己已经认识到这条原则**：`create_analysis_temp_table_as` 的注释原文——「数据**不过 Rust**……既省一次序列化往返，**类型也由 DuckDB 自己定（比 JSON 打型准）**」。本阶段不是引入新理念，而是把已有原则推广到「结果集桥接」这条路上。

**duckdb-rs 的真实入口（1.10505.0 实测）**：

| 需求 | API | 状态 |
| --- | --- | --- |
| **Arrow 入表** | `Appender::append_record_batch(RecordBatch) -> Result<()>`（`src/appender/arrow.rs:30`） | ✅ **已启用**（2026-09-20：根 `Cargo.toml` 的 `duckdb` 加 `features = ["appender-arrow"]`。落在 workspace 层而不是插件方案里写的 `crates/engine`：workbench 也依赖 duckdb，而 feature 本来就是全局相加的；实测**离线可构建**，只多编一个 `num`——`vtab-arrow` 没动 C 内核，预编译动态库照旧） |
| **Arrow 出表** | `Statement::query_arrow<P>(params) -> Result<Arrow<'_>>`（`src/statement.rs:125`） | 可用 |
| 批量行写入（过渡备选） | `Connection::appender` / `appender_with_columns`（`src/lib.rs:556/623`） | 可用 |

**没有** `register_record_batch` / `register_arrow` 这类一等公民入口（`grep "pub fn register" src/lib.rs` 无结果）——所以「直灌」= **建表 + `Appender::append_record_batch`**。

**关键简化：到面板这一段不需要序列化。** `to_data` 拿到的 `&QueryResult` 里 `batches` 就在内存里（`Serialize` 排除 `batches` 只影响跨进程与 JSON），它只是没被带走。所以「携带」几乎不要钱，**工作量大的是 C 档的网格改造**（§2.2 Q5）。

**改动清单（10 项，加法式，处处可回退）**：

| # | 文件 | 改动 | 风险 |
| --- | --- | --- | --- |
| 1 | 根 `Cargo.toml` | `duckdb = { version = "1.10505.0", features = ["appender-arrow"] }` | ✅ **已落地**（2026-09-20）：离线构建通过，未动 C 内核与动态库 |
| 2 | `crates/shared/src/result_set.rs`（新建） | `ColumnMeta` + `ResultSet{batches, columns, total_rows, truncated, has_more}` + `from_batches` / `from_json_rows` + `cell_text`（补 Decimal/Date/Time/Timestamp/Binary/UInt64 的正确格式化） | 新文件，零侵入 |
| 3 | `crates/engine/src/services/duckdb_service.rs` | `create_temp_table_from_batches(conn, &[RecordBatch])`（**用 `batch.schema()` 建表** + `Appender::append_record_batch`）｜`query_duckdb_arrow(conn, sql)` | ✅ **前半已落地**（2026-09-20）：入表那条 + 三条口径（形状一致 / 不支持的类型点名报错 / 列类型与 duckdb-rs 写入层对齐）+ 词典编码解码；6 条单测（含 `typeof` 保真与两条路的数据一致性）。`query_duckdb_arrow`（出表）**还没做** |
| 4 | `crates/engine/src/services/result_types.rs` | `ResultSet` 增 `batches: Vec<RecordBatch>` | — |
| 5 | `crates/engine/src/services/execution_service.rs` | `execute_duckdb_analysis` 增 Arrow 入参（`batches: Option<Vec<RecordBatch>>`）；有 batches 走 `create_temp_table_from_batches`，否则走旧路；回程优先 `query_duckdb_arrow` 并填 `batches` | 两条分支并存 |
| 6 | `crates/editor/src/execution.rs` | `QueryData` **增** `result: Option<Arc<ResultSet>>`（`rows` 保留不动） | 加法 |
| 7 | `crates/workbench/src/services/editor_exec.rs` | `to_data`：把 `executed.result.batches` 装进 `QueryData.result`；`analyze`：把 `Arc<ResultSet>` 传给引擎 | — |
| 8 | `crates/editor/src/analysis.rs` | `AnalysisRequest` 增 `result: Option<Arc<ResultSet>>`；`request_for` 填它（字符串 `rows` 保留作回退） | ⚠ `AnalysisRequest` 有 `PartialEq, Eq` derive，而 `Arc<ResultSet>` 内含 `RecordBatch`——**要么去 `Eq`，要么判等只按 `sql` + 行数** |
| 9 | `crates/workbench/src/services/result_service.rs` | 转发新参数 | — |
| 10 | 清理 | 删 `re_execute_with_filter`（engine + workbench 包装）与 `extract_rows_from_serialized`（均零调用），或按惯例记入零调用台账 | 删前确认无测试引用（已确认：全仓无） |

**建议分三刀提交**（每刀独立可验证）：

| 刀 | 内容 | 验证 |
| --- | --- | --- |
| **第 1 刀** ✅ | 第 1、3 项：**底座**（feature + `create_temp_table_from_batches`） | ✅ 已落地（2026-09-20）：6 条单测 —— `typeof` 三类列保真（`DECIMAL(38,10)` / `TIMESTAMP WITH TIME ZONE` / `BLOB`）/ 多批与空批 / 词典编码 / 形状与类型不支持时点名报错 / 两条路的数据一致 |
| **第 2 刀** ⬜ | 第 2、4、5、6、7、8、9 项：**接线**（`ResultSet` + 契约加 `batches` + 分析段走 Arrow） | D1~D3 断言 + `cargo test-all` 绿 |
| **第 3 刀** ⬜ | 第 10 项清理 + 更新 `infer_type` 的注释 + 出表 `query_duckdb_arrow` | 零调用台账更新 |

**第 1 刀已落地（2026-09-20）**：`create_temp_table_from_batches` 在 `crates/engine/src/services/duckdb_service.rs`，口径与逐行那条路**刻意保持一致**（同一个 `tmp_q_` 前缀与登记），差别只在类型与速度。三条口径都写进了函数文档：

| 口径 | 为什么 |
| --- | --- |
| 所有批**同一个形状**（列名与类型逐一对上） | 中间层不做隐式类型转换 —— 那会把「一边 int 一边 text」这种上游问题掩掉；报错点名第几个批 |
| 不支持的 Arrow 类型**报错**（点名列与类型） | 退化成 `VARCHAR` 是最坏的一种「成功」：用户会以为数据本身就是文本 |
| 列类型与 duckdb-rs 的写入层**对齐**（`duckdb_type_of` 对应它的 `to_duckdb_type_id`） | 声明成 `DECIMAL` 而写入按 `DOUBLE` 转换，会在 append 那一步炸得莫名其妙；`Decimal256` 因此**明确不支持**（它声明侧是 DOUBLE、写入侧没有那条路） |

外加一条协议要求的准备动作：**词典编码的列先解成值类型**（`decode_dictionary_columns`）——duckdb-rs 的写入层没有 `Dictionary` 那条路，而协议允许对端用词典编码（§4.2.4）。

**这一层现在没有生产调用方**（接线是第 2 刀）——「已就绪未接线」不算完成，别在别处当成已完成。

**为什么第 1 刀要单独提**：它是纯加法，把「契约改动」与「DuckDB 行为改动」两个风险分开。第 2 刀出问题时，第 1 刀不用回退。

**验证判据（可执行）**：

```sql
-- 桥接后立刻跑，看类型是否真进来了
SELECT typeof("amount"), typeof("created_at"), typeof("flag") FROM {table} LIMIT 1;
-- 期望：DECIMAL(…) / TIMESTAMP / BOOLEAN
-- 现状：DOUBLE 或 VARCHAR / VARCHAR / VARCHAR
```

| # | 判据 |
| --- | --- |
| D1 | 上述三类列 `typeof` 正确；列名与桥接列名逐个相等 |
| D2 | 桥接行数一致：`SELECT count(*) FROM {table}` = `request.bridged_rows()` |
| D3 | NULL 语义：桥接的 NULL 在库里是 NULL；字符串恰好为 `"NULL"` 的单元格**不应**被当作 NULL（**这是现有 `cell_json` 的隐患，Arrow 路径顺带消除**） |
| D4 | `cargo test-all` 全绿（除存量 `ui_contract` 欠债） |
| D5 | 1000×20 行集的桥接耗时与现状对比，记入 §11 |

**明确不在本阶段范围内**：

- 网格的类型化筛选/排序/惰性格式化（= C 档，§2.2 Q5）——本地筛选/排序**本就重写 SQL 下发**，与 Arrow 无关，不必改
- **洞察那条路**（`duckdb/analysis.rs` 的 `create_analysis_temp_table` + `insert_rows`）：它有惰性清理、半成品回收、`warn_if_near_capacity` 一整套，改它风险更高。**先只改「结果集桥接」，洞察留第二刀**；届时 `infer_type` 那段「两处必须同一套打型规则」的注释要同步改写（否则后人会误判）
- 跨进程 Arrow（属 P1 的协议）

**风险**：

| 风险 | 缓解 |
| --- | --- |
| `appender-arrow` 拉 vtab 导致编译变慢/体积变大 | 第 2 刀前单独量一次（本仓对编译与体积有既有记录：`-j 2`、DuckDB 动态链接） |
| `Appender` 的列类型/顺序与表不一致会报错 | 用**同一个 `batch.schema()`** 建表与 append，不手工拼 `col_defs` |
| 0 行结果集现在落 `VARCHAR`（`rows.is_empty()` 分支） | Arrow 路径由 schema 决定（0 行也有 schema）——**顺带修好的一个失真** |
| 大结果集内存 | 桥接本就有 `ANALYSIS_MAX_ROWS` 上限；Arrow 路径同样先截断再建表 |

---

## 6. 测试场景（验收清单）

### 6.1 跨语言 Arrow 互操作（硬门槛，需 CI）

| # | 场景 | 判据 |
| --- | --- | --- |
| A1 | Rust 写 golden IPC → Python `pyarrow` 读 | 逐列 dtype 与值相等 |
| A2 | Python 写 golden IPC → Rust 读 | 同上 |
| A3 | 不支持 dtype（big-endian / extension type） | **明确报错**，不静默降级 |

### 6.2 类型保真往返

| 类型 | 判据 |
| --- | --- |
| `Decimal(38,10)` | 驱动 → sidecar → 宿主 → DuckDB → 回程，值与精度不变（当前会退化成 Text） |
| `Timestamp(us, UTC)` 含时区 | 不变形 |
| `Binary` / NULL blob | 网格显示不是 `[137, 80, …]` |
| 超大 `UInt64`（> i64::MAX） | 不变 Text |
| 全 NULL 列 | 类型仍正确 |

### 6.3 行为场景

| # | 场景 | 判据 |
| --- | --- | --- |
| B1 | 查询 3000 行 | 首段 1000 + 续取 3 次；**恰好 1000 行时不再无限追加** |
| B2 | 长查询点「中断」 | `query.cancel` 生效，宿主 UI 不卡，错误码 `-32004` |
| B3 | 驱动崩溃 | 宿主如实报错 + 可重启；不静默重连 |
| B4 | 能力缺失 | `indexes=false` 时索引文件夹置灰并给出原因；`capability_denied` 可见 |
| B5 | 项目引用未装 | 打开项目**不阻断**；占位面板给出安装入口 |
| B6 | 协议版本不匹配 | 拒绝加载并报出两个版本号 |
| B7 | 篡改插件包 | 拒绝安装并说明哪一项校验失败 |
| B8 | wasm 死循环 | 在 `max_cpu_time_ms` 内被中断，宿主线程不卡 |

### 6.4 既有回归（不得引入）

| # | 项 | 判据 |
| --- | --- | --- |
| C1 | `ui_contract`（零裸尺寸/零裸色值） | 全绿 |
| C2 | 结果网格筛选/排序/复制/导出 | 行为与改造前一致（排序正确性提升不算回归） |
| C3 | 内置 6 驱动 | 行为不变（`QueryData` 变更后仍全绿） |

---

## 7. 风险

| 风险 | 影响 | 缓解 |
| --- | --- | --- |
| 协议定得太早、改起来贵 | 高 | P1 只定 6 个方法；`protocol` 版本闸；加载期拒绝不兼容 |
| 一个慢查询阻塞同连接的全部元数据访问 | 高 | 按 session 隔离 + `concurrency` 声明 + `session.ping` 独立通道 |
| Arrow 子集跨语言漂移 | 高 | §4.2.4 子集 + golden 互操作测试（§6.1） |
| Jupyter 与 `Transaction` 语义错配 | 中 | 不硬套：`transactions=false`；它真正的落点是 editor「分析模式（Phase 1c，已搁置）」，**单独排期** |
| 网格改造波及编辑器测试 | 中 | P4 先加 `PanelRegistry`（行为不变）再换数据源；分两步提交 |
| 平台矩阵与体积（JRE/Python） | 中 | 学 Tabularis：插件仓库自各平台 zip，宿主只存签名哈希；**内置运行时不在范围** |
| 文档继续漂移 | 中 | P0 修 §10 四处 → ✅ 已修（2026-09-20，含 P0 删除项的回填；见 §10）；此后「实现位置映射表」随改动更新 |

---

## 8. 验证命令

```sh
# 默认只覆盖 app 依赖图（根 Cargo.toml 的 default-members），plugin 不在其中
cargo check -p rds-plugin --all-targets
cargo test  -p rds-plugin

# 全量
cargo check-all        # check --workspace --all-targets
cargo test-all         # test --workspace -j 2（自带 RUST_MIN_STACK / RDS_HOME / TEMP）
```

---

## 9. 实现位置映射表

| 设计决策 | 实现位置 |
| --- | --- |
| 清单与 `[backend]` 段 | `crates/plugin/src/manifest.rs`（✅ **已落地**：`backend` / `BackendKind` / `BackendTransport` / `CapabilitiesDriver{concurrency}`；`command()` 四道闸：形态 → 传输 → 平台 → 路径；`process_spec()` 接进程池） |
| 权限与信任（双轨） | `crates/plugin/src/permission.rs` |
| 生命周期与进程池 | `crates/plugin/src/sidecar/lifecycle.rs`（✅ **P1 已落地**：三层对象模型的决策内核，sans-io）、`crates/plugin/src/sidecar/process.rs`（✅ **P1 已落地**：起/收真实进程 —— `current_dir` / stderr 日志 / 先关 stdin 再等，不信 EOF 就强杀）、`crates/plugin/src/manager.rs`、`sidecar/health_checker.rs`（0 字节，待填） |
| 传输与帧 | `crates/plugin/src/sidecar/proto.rs`（✅ 帧/增量解码/async 流读写/版本闸/阈值/错误码）+ `crates/plugin/src/sidecar/conn.rs`（✅ 异步客户端：drive 任务 + 在飞表 + 事件通道 + 超时放弃）+ `crates/plugin/src/sidecar/process.rs`（✅ 三管道接入连接）；`client.rs` 与旧 `sidecar/manager.rs` 已**删除**（P1）：HTTP/端口那条路整条下线，`rds-plugin` 也不再依赖 `reqwest` |
| sidecar 运行时接线 | `crates/plugin/src/sidecar/supervisor.rs`（✅ **P1 已落地**：`Deployment`（清单 → 进程规格 + 命令）+ `SidecarSupervisor` —— 执行内核动作、兑现放行、事件如实上报、generation 防旧事件误伤、起不来不留僵尸 `Starting`） |
| 真实进程验收 | `crates/plugin/tests/fixture/sidecar.rs`（`[[bin]] rds-sidecar-fixture`：**独立实现**一遍帧编解码的测试对端）+ `crates/plugin/tests/spawn_real_process.rs`（8 条：起收摊 / 超时不断连 / 崩溃交还 + 退出码 / 未调用也发现它死 / 强杀兜底 / stderr 落盘）+ `crates/plugin/tests/supervisor_real_process.rs`（8 条：开会话 / 串行排队放行 / 放行失败 / 崩溃要手动重启 / 心跳判死 / 空闲回收不误伤新实例 / 起不来不留僵尸 / 收摊） |
| 跨实现一致性验收 | `crates/plugin/tests/sidecar_conformance.rs`（✅ **P1 已落地**：同一套检查跑**任何** sidecar —— `RDS_SIDECAR_BIN` 换个二进制就行。五条 = 握手与描述 / 大结果必须走 Arrow 且解得成 `RecordBatch` / 取消要真的停 / stdin EOF 必须自退 / 强杀兜底。**这五条就是 P1 的退出标准**，可反复执行；写法见用户手册 §2.8） |
| `meta.*` 元数据面 | `crates/plugin/src/sidecar/meta.rs`（✅ **P2 已落地**：线格式 ↔ 引擎类型；认不出的类别进 `MetaObjectKind::Other` 并跳过；`into_node_info` 把物化视图并到视图、`into_column_detail` 把 `canonical`/`format` 放进 `extra`）+ `crates/plugin/src/sidecar/driver.rs`（`SessionDriver::{meta_catalogs, meta_schemas, meta_objects, meta_object_detail, meta_routine_source}`，统一 `call_meta`：同一超时、不收附件）+ `crates/plugin/tests/meta_bridge_real_process.rs`（9 条真进程） |
| RPC 方法表 | `crates/plugin/src/sidecar/*`（+ `jsonrpsee-core` 的 `RpcModule`/`Methods`） |
| 注册路径（引擎侧选得到） | `crates/plugin/src/sidecar/factory.rs`（✅ **P1 已落地**：`SidecarDriverFactory` + `register_sidecar_drivers()`；`DriverKind::Sidecar` 是它在引擎里的身份，见 `crates/engine/src/driver/registry/descriptors.rs`） |
| 驱动桥（RPC 方法表） | `crates/plugin/src/sidecar/driver.rs`（✅ **P1 已落地**：`SessionDriver` = 会话之上的 `driver.describe` / `query.execute` / `query.fetch` / `query.cancel` / `session.ping`，`QueryPage`/`PageData` 把内联 JSON 与 Arrow 附件统一成同一个类型，`DriverError` 按错误码分流。旧 HTTP 版已于 P0 删除，见 §1.2） |
| 接引擎 `Database` trait | `crates/plugin/src/sidecar/driver.rs`（✅ **P1 已落地**：`SidecarDatabase` —— `QueryPage` → `QueryResult`（只填 `batches`，内联 JSON 也补成 `RecordBatch`）、错误按域映射、弱引用连接、真取消）；工厂与注册见 `SidecarDriverFactory`（P1 收尾） |
| 元数据面（导航 / 属性面板 / 内容档） | `crates/plugin/src/sidecar/driver.rs`（✅ **P2 已落地**：`SidecarDatabase` 实现 `Database` 的 `list_catalogs` / `list_schemas` / `list_tables` / `list_columns` / `list_procedures` / `list_functions` / `list_sequences` / `list_triggers` / `get_routine_source`；五个文件夹都从**同一次** `meta.objects` 里挑（`objects_of` + `take_kind`）；`wire_schema` 的取值顺序是「显式 → `default_schema` → 空串」；索引/约束**明细**如实报不支持）+ `crates/plugin/tests/database_meta_real_process.rs`（8 条真进程） |
| 能力门控 | `crates/plugin/src/sidecar/driver.rs`（✅ **P2 已落地**：`cancel` 在 `query_with_cancel` 里门控、`transactions` 分两句说、`schemas` 落成 `MetadataBrowser::has_schema_level`；口径见 §4.4.1）+ `crates/plugin/tests/capability_gate_real_process.rs`（5 条真进程） |
| 能力矩阵 | `crates/engine/src/driver/capability.rs`（`CAPABILITY_DICTIONARY`）——**P4 再谈**：清单那一份要等连接对话框这个读者 |
| 类型归一化与 `ResultSet` | `crates/shared/src/result_set.rs`（新增）+ `crates/shared/src/arrow.rs` |
| 二次分析直灌 | `crates/engine/src/duckdb/duckdb_service.rs:38-101`、`crates/editor/src/analysis.rs` |
| 编辑器承载 | `crates/editor/src/execution.rs`（`QueryData`）、`src/store.rs`（`ResultEntry`）、`src/view/results/grid.rs` |
| 宿主汇聚 | `crates/workbench/src/services/editor_exec.rs:1089-1106`（`to_data`） |
| 面板/命令/设置注册表 | `crates/workbench_shell/src/model.rs`、`crates/workbench/src/panels/mod.rs`、`quick_open/commands.rs` |
| 路径 | `crates/paths/src/lib.rs` |
| 项目引用表 | `crates/engine/migrations/project_meta/026_project_resources.sql`（待 Q1）+ `crates/engine/src/persistence/plugin_store.rs` |
| wasm 限额与缓存 | `crates/plugin/src/wasm/{extism,plugin_manager}.rs` |
| 安装与签名 | `crates/plugin/src/installer.rs` |

---

## 10. 文档修订清单（与代码不符）

P0 一次性清理完毕（2026-09-20）——下表是**已处置**清单，留作口径存档：

| 文档 | 曾声称 | 实际 | 处置 |
| --- | --- | --- | --- |
| `plugin-architecture.md` §2 | plugin 7488 行 | P0 前 4630 行 → **现 4395 行**（删了 311+107+98） | ✅ 已改 |
| `plugin-architecture.md` §8 | 「项目引用：V2 无，只有全局 `plugin_store`」 | **表 + 持久化 + 6 个 service 方法都在**（无生产调用方） | ✅ 已改 |
| `plugin-architecture.md` §2 / `core-design-current.md` §3 | 注册表在 `global.sqlite` 的 `plugin_store` | 表名是 **`plugins`**（`plugin_store` 是 Rust 模块名） | ✅ 均已改 |
| 未记录 | — | `sidecar/driver.rs`（311 行）与 `storage.rs`（107 行）**未参与编译** | ✅ 已记 → **现已删除**（§1.2），相关文档（`plugin-architecture.md` §2、`driver-capability-matrix.md` §5、本仓 `plugin/README.md`、`plugin-user-guide.md`）已同步为“已删” |
| `plugin-architecture.md` §9 | 三期计划骨架 P3-a…P3-g | 由本文件 §5 取代，§9 改为指向本文 | ✅ 已改 |
| `runtime/data-paths.md` §8 | 7488 行、`docs/` 下无插件文档、目标目录未落地 | 目录已落地（`paths` 六个函数）、文档已建 | ✅ 已改（标「已落地」并指向本目录） |
| `plugin-prototype-design.md` §1 / §5 | 「本原型不提供 Webview」、「`~/.rds/extensions/`」 | Q8 早已定案可提供；`RDS_HOME` 默认是安装目录，插件根是 `<RDS_HOME>/plugins/` | ✅ 已改（2026-09-20） |

---

## 11. 进度记录

| Phase | 状态 | 数字 / 证据 |
| --- | --- | --- |
| P0 | ✅ **完成**（2026-09-20） | `paths` 新增 6 函数 + `validate_plugin_id` 白名单 + `NEW_LAYOUT_DIRS` 补登（`cargo test -p rds-paths` 13/13）；`PermissionType::{Sidecar,Driver}` + `is_gating()` + 清单三字段；删除 `sidecar/driver.rs`/`storage.rs`/`wasm/host_functions.rs`（共 516 行）；修 `client.rs` 反向判据（抽 `parse_rpc_response` + 4 条单测）；`cargo check-all` 绿；`cargo test -p rds-plugin` 19/19 |
| P1 | 🟡 进行中，**代码面已齐**（协议 / 附件 / 生命周期 / 异步客户端 / 进程层 / 清单 `[backend]` / 运行时接线 / 驱动桥 / 接引擎 / 注册路径十块已落地） | `sidecar/proto.rs`：帧 + 增量解码 + async 流读写 + 版本闸 + 阈值 + 错误码。`sidecar/router.rs`：附件语义两个方向 + 错位上报 + 断线交还 + 放弃。`sidecar/lifecycle.rs`：三层对象模型决策内核（去重 / max_instances / serial 排队 / ping 判死 / 空闲回收 / 崩溃不静默重连）。`sidecar/conn.rs`：异步客户端（三任务、在飞状态只一份、超时显式放弃、`initialize` 含版本闸）。`sidecar/process.rs`：起/收真实进程（`SpawnSpec` → `paths::*` 目录 + stderr 日志；`retire` 先关 stdin 再等，不信 EOF 就强杀）。`manifest.rs`：`[backend]` 段与 `process_spec()`（口径见 §4.3.1）。`sidecar/supervisor.rs`：运行时接线（`Deployment` + `SidecarSupervisor`：执行内核动作、把 `Decision::Open` 兑现成 `session.open`、事件显式 `drain_events`、generation 防旧事件误伤、起不来告诉内核不留僵尸）。`lifecycle.rs` 的 `acquire` 按规则 1 修正分流（并行共享 / 串行有额度就起新实例）。`sidecar/driver.rs`：驱动桥（`SessionDriver`：describe / execute / fetch / cancel / session.ping；`PageData` 把内联 JSON 与 Arrow 统一成一个类型；Arrow 在宿主侧解成 `RecordBatch`；`DriverError` 按错误码分流）。`cargo test -p rds-plugin` 103/103 + 集成 22/22（P1 新增 86 条单测 + 22 条真进程集成测试）。`sidecar/factory.rs`：注册路径（`SidecarDriverFactory` + `register_sidecar_drivers`；`create` = 起进程 → 握手 → 会话 → describe；排队超时明确失败并撤排队；`Drop` 交还会话）。`lifecycle.rs` 修一处内核缺口：撤掉**还在排队**的会话（原先从队列里摘不掉，别人关闭时会被放行成幽灵会话）。`sidecar/driver.rs` 再接上引擎：`SidecarDatabase` 实现 `engine::driver::Database`（`QueryPage` → `QueryResult` 只填 `batches`；错误按域映射；弱引用连接；令牌取消走**真取消**）。HTTP 旧路径两个文件（`client.rs` + 旧 `manager.rs`）已删，`reqwest` 依赖随之去掉。`cargo test -p rds-plugin` 106/106 + 集成 44/44（新增 `tests/sidecar_conformance.rs`：5 条把 P1 退出标准做成可反复执行的检查，换 `RDS_SIDECAR_BIN` 就能拿同一套检查去验任何语言的 sidecar）。**代码面到此齐了**；剩下的要在真机上做：**PostgreSQL 靶子**（真库 + 真 Arrow）、**跨语言 Arrow 互通**（Go `arrow-go` / Python `pyarrow`，本机拉不到依赖）、把这两条固化成验收脚本。之后进 P2（`meta.*` + `DriverCapability` 门控 + 导航树） |
| P2 | 🟡 **代码面已齐**（四刀：协议 / 引擎元数据面 / 能力门控 / 一致性验收） | 见 §11.1（`cargo test -p rds-plugin` = 114/114 + 集成 61/61） |
| P2.5 | 🟡 **底座已落地**（第 1 刀），接线未做 | 见 §11.2 |
| P3 | ⬜ 未开始 | 面已收窄：`host_functions.rs` 已删，P3 是**从零建**而不是“已有面收敛” |
| P4 | ⬜ 未开始 | — |
| P5 | ⬜ 未开始 | — |
### 11.2 P2.5 的落地清单

**第 1 刀（底座）✅ 2026-09-20**：根 `Cargo.toml` 给 `duckdb` 打开 `appender-arrow`（纯 crate 侧特性，离线可构建）；`DuckDbService::create_temp_table_from_batches` 落地 —— 用 `batch.schema()` 建表 + `Appender::append_record_batch` 直灌，配 `duckdb_type_of`（Arrow → DuckDB 列类型，与 duckdb-rs 写入层对齐）、`same_shape`（批形状一致）、`decode_dictionary_columns`（词典编码先解码）、`quote_ident`（列名加引号，含引号翻倍）。6 条单测：类型保真（`DECIMAL(38,10)` / `TIMESTAMP WITH TIME ZONE` / `BLOB` / `BOOLEAN` / `VARCHAR` / `BIGINT`）、高精度值往返（`1234567890.1234567890` 不被压成 f64）、多批累加与空批、词典编码、形状/类型不支持时点名报错、两条路（Arrow / 逐行）数据一致。`cargo test -p rds-engine --lib` = 483/483 + `check-all` 绿。

**第 2 刀（接线）⬜**：按 §5.1 的 10 项清单做第 2、4、5、6、7、8、9 项 —— 它的形状已经清楚：`ResultEntry` 不带 Arrow 是关键卡点（编辑器那层只有 `Vec<Vec<String>>`），所以要给 `QueryData` / `AnalysisRequest` 加「带批的那一份」并一路传到引擎；`AnalysisRequest` 有 `PartialEq, Eq` derive，`Arc<ResultSet>` 进去会让判等变味（去 `Eq` 或只按 SQL 与行数判等）。

**第 3 刀 ⬜**：`query_duckdb_arrow`（出表那条路）+ 清理零调用项 + 更新 `infer_type` 的注释（它只产四档类型，接线后「两处必须同一套打型规则」的说法要改）。

### 11.1 P2 的落地清单（四刀）

**第一刀（协议与驱动桥）**：`sidecar/meta.rs`：五个方法的线格式与解析、`MetaObjectKind`（含 `Other` 留痕）、`nav_kind`（物化视图并到视图）、`into_node_info` / `into_column_detail`（`canonical`/`format` 进 `extra`，属性面板仍看原始类型）、6 条单测。`driver.rs`：`SessionDriver::{meta_catalogs, meta_schemas, meta_objects, meta_object_detail, meta_routine_source}` + `call_meta`（同一超时、**不收附件**）。`proto.rs`：补 `-32009 connect_failed`（P1 记下的缺口）→ `map_error` 落 `ConnectionError::Refused`。靶子：`meta.*` 的假 schema（覆盖七个类别 + 一个摆不下的 `domain`）+ `--cap-na=<键>` 能力开关（连带的 meta / cancel 调用也如实回 `-32006`）。`cargo test -p rds-plugin` = 112/112 + 集成 47/47（其中 `meta_bridge_real_process.rs` 9 条）。

**第二刀（引擎元数据面）**：`SidecarDatabase` 实现 `Database` 的九个元数据方法（catalog / schema / 表 / 列 / 过程 / 函数 / 序列 / 触发器 / 例程源码）。五个文件夹共用**同一次** `meta.objects`（`objects_of` + `take_kind`）—— 导航逐文件夹收集，所以展开一个 schema 会问 4 次（表+视图 1 次、例程 1 次、序列 1 次、触发器 1 次），重复展开由导航的 L1 / L2 挡住；`wire_schema` 的取值顺序是「显式给的 → `describe` 的 `default_schema` → **空串**」（空串是有含义的：没有 schema 层的驱动自己决定默认，宿主不替它猜）。索引 / 约束**明细**如实报 `NotSupported`（协议面还没定，`object_detail` 只给个数）—— 这不是「没有索引」。`cargo test -p rds-plugin` = 114/114 + 集成 55/55（新增 `database_meta_real_process.rs` 8 条）。

**第三刀（能力门控）**：`impl MetadataBrowser for SidecarDatabase`（与 `Database` 那几个方法**同一份实现**；多出来的关键一处是 `has_schema_level` = 驱动的 `schemas` 能力 —— 单层库的 sidecar 不再长出一层空 schema）+ `as_metadata_browser` 返回 `Some(self)`；`cancel` 门控（没声明就**不装中断**：令牌响了先等语句收场，再如实报「没能停下来」，不把跑完的结果伪装成被中断的结果）；`transactions` 门控把「驱动没这个能力」与「宿主桥还没接」分开说；`get_table_detail` 遇到导航摆不下的类别**报错而不是拿「表」顶上**。`cargo test -p rds-plugin` = 114/114 + 集成 60/60（新增 `capability_gate_real_process.rs` 5 条）。清单 `[capabilities.driver]` 那一份**明确推到 P4**（见 §4.4.1 末尾：没有读者的第二份真相）。

**第四刀（一致性验收）**：`sidecar_conformance.rs` 从五条补到六条 —— 第 6 条是**导航面**（`meta.*` 五件的形状 + 空库的合法降级，`--nocapture` 时把实际答出来的 catalog / schema / 对象数 / 列打出来）。`RDS_SIDECAR_BIN` 换个二进制，第三方 sidecar 也能拿同一套检查自检。`cargo test -p rds-plugin` = 114/114 + 集成 61/61。

**导航层的验收边界（说清楚，别当成已验）**：「导航树完整」这一条在**契约层**是自动化的（那些测试断言的就是 `NavigatorService` 消费的那份 `Vec<NodeInfo>` / `Vec<ColumnDetail>`，含「按 `kind == View` 分两半」这条导航依赖的规则）；但**导航层本身**（`NavigatorService` → 树节点 → 属性面板）没法在本仓自动跑：① 目前**没有任何 crate 依赖 `rds-plugin`**（第一条消费边留给 P4 的注册表接线，现在加一个只测试用的反向 dev-dep 会把分层搞乱）；② 靶子二进制只对 `rds-plugin` 自己的测试可见（`CARGO_BIN_EXE_*` 是包内变量），跨 crate 拿不到路径。所以这一条进**实机验收清单**（真库 + 手动展开五个文件夹 + `#` 内容档搜到该库对象）。

**「不支持的文件夹置灰并说明」还没做到**（P2 这一条**不算完成**）：现在只能做到「文件夹计数 0」—— 要置灰还得让 `NavNode` 支持「禁用 + 原因」，那是导航模型的事，与 P4 的 `PanelRegistry` 一起动；能力信息已经在 `SidecarDatabase::descriptor()` 上等着读者了。

**P2 剩下的（都不属 P2 的必要路径）**：`rds.*` 的 schema metadata 读到消费方（P2.5 的 Arrow 直灌会用到）；`cursor` / `explain` / `readonly` 等能力等到有消费者再谈；`meta.indexes` / `meta.constraints` 的明细面（属性面板要列明细时定）。
