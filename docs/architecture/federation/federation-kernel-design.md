# 联邦 / 分析 / 洞察 公共内核 · 设计（Phase 5.5）

> 分支 `feat/federation-kernel`（worktree `D:\RdataStation\RDS\wt-kernel`，隔离目标目录与数据根）。
> 本文只定义**内核边界与切片顺序**，不重复 `federation-entry-design.md` 的方案论证。
> 前提裁决（已定，不再问）：共享会话不共享连接；编辑器按通道显式启动；洞察 / 分析默认本地 + 默认关的「用联邦查询」开关。

## 1. 为什么需要内核（现状三条通道各写一套）

| 通道 | 现在怎么走 | 落点 |
| --- | --- | --- |
| 编辑器联邦通道 | 自己拼 `FederatedPlan{owner,sources,primary}` → `ensure_session` → `run` | `editor/src/ports/editor_exec/federation.rs:171-174`、`:268`、`:327` |
| 编辑器加速通道 | `accel::ensure_session(&source)` 另一条装配 | `editor/src/ports/editor_exec/accel_exec.rs:20/62` |
| 洞察取样 | 入口把来源翻译成 `SampleSource{conn_id: Option, sql, label}`，交付 `jobs` 执行 | `insight/src/model/target.rs:13-48`；`workbench/src/components/insight_new_dialog.rs:49+` |
| 结果集二次分析 | **未实装**（选择器只是裁掉了 `ResultSet`） | `source_picker/mod.rs:78` |

四者的差别只在**「这份源在哪、怎么挂、在哪跑」**三件事，却各自维护一份翻译逻辑。内核要收的就是这份翻译。

## 2. 已存在、直接复用的东西（不要重造）

| 能力 | 现成实现 |
| --- | --- |
| 会话建立 / 缓存 / 指纹 | `engine/duckdb/federation/session.rs:498 ensure_session(owner, sources, requested_primary)`、`:562 snapshot_for(owner)` |
| 单源挂载（唯一实现） | `session.rs:381 attach_source`；`registry.rs:274-290 attach_sql()`（L1 源一律 `READ_ONLY`） |
| 会话内表枚举 | `temp_table.rs:539 list_by_source` |
| 本地临时表（Arrow 直灌） | `temp_table/analysis.rs create_analysis_temp_table_arrow`（洞察取样已在用） |
| 三分法分类 | `source_picker/group.rs:49 group_of`（根 3 / 叶 5） |
| 入口选源 UI | `source_picker`（13 刀已进 main） |

## 3. 缺口（内核要补的三件事）

1. **没有「源 → 挂载计划」的统一翻译**。`SampleSource` 只能表达「一条连接 / DuckDB 内存库上的 SQL」，表达不了「多源同时挂进一个会话」「哪条是 primary」「挂不上时怎么降级」。
2. **没有「通道」概念的统一表达**。编辑器的 `ExecChannel` 是 UI 侧门控枚举（`editor/src/channel`），引擎侧没有对应类型；洞察与结果集没有通道概念。
3. **物化（形态 I / 形态 II）没有统一入口**。洞察自己包 `LIMIT` + 建 `tmp_i_`；结果集二次分析要「建表与 SQL 都在会话里」；两条路径现在各写一次。

## 4. 内核形态

归属 **`rds-engine`**（workbench / insight / editor 都依赖它，且会话与临时表都在这里；不再抽新 crate——理由同选择器：痛点是把装配散在多处，不是缺少 crate 边界）。

新模块 `engine/src/analysis/`：

```rust
// 4.1 通道：谁在跑、跑在哪儿
// 【2026-11-15 口径修订】执行通道是**三档**，不是「本地 / 联邦」两档：
//   Source  = 源库直连，SQL 交给驱动，不进 DuckDB、不挂源
//   Accelerated = 本地加速，把**一个**源只读挂进进程内 DuckDB（accel::ensure_session，
//                 按源 id 缓存；L2 驱动在这里注册会话级 Secret）
//   Federated = 联邦会话，按 owner 多源挂载（federation::ensure_session）
// 早先把 Source 与 Accelerated 合成一个 Local 是错的（这条由用户 2026-11-15 指正）。
pub enum AnalysisChannel {
    Source,                       // 源库直连（默认档）
    Accelerated,                  // 本地加速（单源，accel.rs:508）
    Federated { owner: String },  // 显式联邦（编辑器联邦档 / 洞察开开关）
}

// 「用联邦查询」开关是对 base 通道的**升级**，不改变用户选的档：
// from_toggle(base, on, mountable_sources, owner) → on && ≥2 源 && owner 非空 ? Federated : base

// 4.2 源：内核只认「怎么挂」，不认 UI 语义
pub enum MountSpec {
    SqlConnection { conn_id: String, read_only: bool }, // L1：驱动连接，READ_ONLY
    DuckDbFile { path: PathBuf, read_only: bool },      // 全局库 / 项目分析库 / 归档 duckdb
    DuckDbTable { catalog: String, schema: String },    // 会话内已有表（结果集 / 先前物化）
}

// 4.3 计划：一次分析的完整装配结果
pub struct SessionPlan {
    pub channel: AnalysisChannel,
    pub mounts: Vec<MountSpec>,       // 空 = 纯本地
    pub primary: Option<String>,      // 会话默认 catalog；学界见 session.rs 的 requested_primary
    pub label: String,                // 面板标题 / 快照来源
}

// 4.4 内核入口（三通道共用）
pub struct AnalysisKernel;
impl AnalysisKernel {
    /// 源的 UI 语义 → 挂载计划（唯一翻译点）
    pub fn plan(source: &SourceSpec, mode: SessionMode) -> Result<SessionPlan, CoreError>;
    /// 按计划拿会话：Local 走内存单例，Federated 走 ensure_session
    pub fn open(plan: &SessionPlan) -> Result<SessionHandle, CoreError>;
}

pub enum SessionHandle {
    Local(Arc<DuckDBManager>),          // 或现有内存单例句柄类型
    Federated(Arc<FederatedSession>),
}

// 4.5 物化（形态 I / II 的唯一实现）
impl SessionHandle {
    /// 形态 I：把源样本搬进本地 tmp_i_（洞察）
    pub fn materialize_sample(&self, spec: &SourceSpec, limit: usize) -> Result<String, CoreError>;
    /// 形态 II：在会话内建表并登记（结果集二次分析）
    pub fn materialize_in_session(&self, spec: &SourceSpec, name: &str) -> Result<String, CoreError>;
}
```

**`SourceSpec` 与选择器的关系**：`SourceSpec` 是工程化投影（`Picked` + 上下文），不是新枚举——`Picked{Draft,Analysis,Db,External}` 保持不变（`Picked` 是 UI 契约，已在 13 刀里钉死）。

### 4.6 三通道各自的调用形态（改完后）

| 通道 | plan 的 channel | 物化 |
| --- | --- | --- |
| 编辑器源库档 | `Source`（不挂源） | 不物化（驱动直接执行） |
| 编辑器联邦档 | `Federated{owner}`（显式） | 不物化（SQL 直接 `session.run`） |
| 编辑器加速档 | `Accelerated`（`accel::ensure_session`，**单源**） | 不物化 |
| 洞察 | `Accelerated`，开关开时 `Federated{owner}` | `materialize_sample`（形态 I） |
| 结果集二次分析 | `Accelerated` 或 `Federated{owner}` | `materialize_in_session`（形态 II） |

> 注意：加速档**不是**「进程级内存单例」那条通道。`accel::ensure_session` 按源连接 id 各建一条会话，凭据随连接串进 `ATTACH`（DuckDB 1.5.5 的 mysql/pg 扫描器不认 Secret，`accel.rs` 注释有实测记录）；进程级单例（`duckdb_service.rs:14`）只服务「不挂任何外部源」的纯本地查询（导出、临时表、二次分析的建表）。

## 5. 切片（每刀一 commit，每刀 `cargo check` + 该模块测试）

| 刀 | 内容 | 验收 |
| --- | --- | --- |
| 0 | 本文 | —— |
| 1 | `analysis/channel.rs`：`AnalysisChannel{Source, Accelerated, Federated}` + `ConnRef{Live, Historical}` 纯类型 + 语义测试（三档互斥、owner 非空、开关只做升级） | 纯单测（已落地 `d98ef2c4` + `9395a679`） |
| 2 | `analysis/mount.rs`：`MountSpec` + `plan_for` 规则（Source 禁挂 / Accelerated 恰 1 源 / Federated 去重 + primary 校验），覆盖 5 叶 × 3 档 | 纯单测（已落地 `80f37c86` + `9395a679`） |
| 3 | `analysis/mod.rs`：`AnalysisKernel::open`，把三档收口（Source 无会话 / Accelerated→accel / Federated→federation）；**不改任何消费方** | 单测 + `engine --lib` 全绿 |
| 4 | 编辑器通道切到 `AnalysisKernel::open`（`federation.rs` / `accel_exec.rs` 两处），删掉各自的 plan 拼装 | 编辑器现有 242/277 两条端口测试继续绿 |
| 5 | 洞察切到 `plan + materialize_sample`；「用联邦查询」开关做 `from_toggle` 升级（仍默认关、仅 ≥2 可挂源时出现） | `insight --lib` 280 + e2e 14 |
| 6 | 结果集二次分析入口接上（`source_picker` 的 `ResultSet` 解除裁剪），走形态 II | 新用例：建表→SQL→清理 |
| 7 | 全局表通道 A（内存单例 attach）+ B（联邦会话加源）——**需先拍板**，见 §7 | —— |

## 6. 验收口径

- `cargo check -p rds-engine -p rds-insight -p rds-editor -p rds-workbench --lib` 零告警；
- `cargo test -p rds-engine --lib`（现基线 428）、`-p rds-insight --lib`（280）、`-p rds-workbench --lib`（138）全绿；
- 三个通道各留一条端到端用例（真机四库用例沿用 `workbench/tests/{federation_sources,insight_source_real}.rs` 的骨架）。

## 7. 待拍板（挡在刀 6 / 刀 7 之前）

1. **owner 口径**：`conn_id`（推荐，编辑器的 `FederatedPlan.owner` 已按它工作）vs 面板级 owner。
2. **全局表**：(a) 先做 A+B 两条 attach；(b) 拆 `SourceKind::AnalysisGlobal` 按入口裁剪；(c) 冻结全局段。**洞察侧还有一个结构事实**：取样按 `conn_id` 下推（`insight/jobs/table.rs:312-320`、`insight_engine_internal/profile.rs:12`），全局表无 conn_id → 形态 I 不可达，必须选形态 II 或造伪 conn_id。
3. **结果集二次分析的默认档**：`Local`（推荐）还是跟随编辑器通道。

## 8. 风险

- **三套单例 / 会话库**（2026-11-15 修订后口径）：
  1. 进程级内存单例：`execution_service.rs:60/113 → duckdb_service.rs:14`（纯本地查询、导出、临时表）；
  2. 加速会话：`duckdb/accel.rs` 的 `accel::ensure_session`（按源 id 缓存，单源只读挂载）；
  3. 联邦会话：`federation/manager.rs:18` 的 `GLOBAL_DUCKDB` + `session.rs:498 ensure_session`（按 owner 多源）。
  三者**都不能合并**：合并会踩「取消洞察误伤编辑器」那条已实测的坑（同一连接被两处同时中断）。刀 3 收口时按通道分流，而不是收成一条连接。
- **物化路径的清理责任**：形态 I 建在本地 `tmp_i_`（TTL/上限现成），形态 II 建在会话里（会话销毁即消失，但用户可见）；两者「表失效」的提示条不同，刀 6 要各自给文案。
- **不要在刀 4 改编辑器行为**：`ExecChannel` 门控是 UI 事实，内核只接装配，不接管门控。
