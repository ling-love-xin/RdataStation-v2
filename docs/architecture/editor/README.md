# SQL 编辑器模块 · 模块入口

> **一句话**：一个内核、三档能力——文本模式是"不与数据库通信的记事本"，SQL 模式是 DBeaver 一档的脚本窗口（连接 + 执行 + 结果 + 历史），分析模式把文档变成**单元 + 会话 + 输出**的可执行笔记；核心始终是 SQL。
>
> 本文只提炼**特点 / 边界 / 代码地图 / 硬约束**；细节一律指向本目录内文档，**不复制设计**。
> 状态：**Phase 0 已完成 + Phase 1a 基本完成（2026-09-15）**——地基部分落地：语句切分、SQL 高亮（13 项回归）、格式化（sqlglot generator）、历史字段真实化；**编辑器已进工作台中央 Dock**（多文档标签、脏点、状态栏、SQL 着色、`Ctrl+S` / `Ctrl+/` / `Ctrl+W` / `Ctrl+Enter` 实测生效），**执行链已闭环**（`Ctrl+Enter` → 结果网格；真机四库 mysql/postgres/sqlite/duckdb 实测通过），**查找 / 替换用内核与组件库能力**（`Ctrl+F` / `Ctrl+H`，本模块零自建），**会话跨重启恢复**（光标 / 选区 / 模式落 `editor_contexts`），**大文件按档位降级**（>50MB 关重能力、≥200MB 不读进内存）。已知问题与排期的**唯一权威**是架构文档 §12。
>
> **边界**：本模块拥有**编辑与执行**（编辑器内核 / 三模式 / 执行族 / 结果集 / 历史 / 单元与会话）。连接的新建与编辑属 M3；对象树与元数据内省属 M4；Mock 属 M7；洞察属 M8；DuckDB 分析资产属 M6。本模块只经服务/命令与它们协作（契约见架构 §3.4）。

## 1. 模块特点

### 产品行为

| 特点 | 含义 | 出处 |
| --- | --- | --- |
| **一个内核 + 三档能力** | 文本 ⊂ SQL ⊂ 分析，能力严格递进；模式只是"挂哪些服务、显示哪些 chrome、结果去哪"，**不做三套编辑器** | 架构 D1、原型 §1.1 |
| **模式可判定、切换有代价** | 显式 > 扩展名 > 入口语义；上/降级是显式动作（SQL→分析 需确认转换粒度），**禁止静默改字段** | 原型 §1.2 / §1.3、架构 D2/D3 |
| **只读是两个维度** | 编辑器只读（不可输入）与连接只读（可输入、写语句被拦）分别表达 | 原型 §1.4、架构 D4 |
| **执行目标是显式的** | 选区优先 → 当前语句（词法级切分定位）→ 全部 → 批量；`Ctrl+Enter` 是主动作 | 原型 §5.1、架构 D7 |
| **执行族与执行通道正交** | “执行什么”（当前语句 / 选区 / 全部 / 批量 / 新标签）与“在哪执行”（源库 / DuckDB 本地加速 / 联邦）是两个轴；通道用「执行位置」指示器表达、随文档绑定持久化 | 原型 §2.2 / §5.7、架构 D21 |
| **结果单权威、多视图** | 一份 `ResultSet`，两个视图（停靠结果面板 / 单元内联输出）；同一结果不复制数据 | 架构 D6 |
| **结果带通道徽标与血缘** | 筛选下发与 DuckDB 分析**产生新结果集**（不就地替换），旧结果可回看；标签显示来源与通道 | 原型 §2.4、架构 D22 |
| **分析模式：单元 + 会话 + 输出** | `cell_id` 稳定、输出只存引用 + 预览（数据驻 DuckDB 临时表）、`stale` 显式表达"结果已过期" | 原型 §4、架构 D14/D15/D16 |
| **键盘优先** | 保存 / 执行 / 注释 / 格式化 / 运行全部 / 插入单元全部有键位，**注册了才宣传**（不留"假快捷键"） | 原型 §5.2 |

### 执行与结果

| 特点 | 含义 | 出处 |
| --- | --- | --- |
| **执行入口唯一** | `EditorService::execute(target)`；UI 不得自行拼装执行（M4 的"查看数据自动执行"复用同一入口） | 架构 D5、§3.4 |
| **后台执行，render 零 I/O** | 执行 / 内省 / 加载一律入后台任务 + 结果队列，主线程轮询回填（沿用 M4 `nav_jobs` 模式） | 架构 D19、§5.3 |
| **取消 / 超时 / 行数上限** | 取消走连接级 token；超时按连接配置；结果超过上限截断并明示 | 架构 §8 |
| **历史字段必须真实** | 耗时 / 成功 / 失败原因 / 返回与影响行数全部落库（现状硬编码 `unknown`、耗时 0、失败不写——必须先修） | 架构 §7.3 #4 |
| **错误可定位** | 解析驱动错误文本中的行列 → 诊断 + 定位 + 聚焦；无位置信息时只提示，不误报第一行 | 原型 §5.4 |
| **结果集只读** | 行内编辑与写回源库属 M4 的表格能力（避免 V1 的语义纠缠与脏状态双轨）；两者只共享网格渲染层 | 架构 D23 / §3.6 |

### 架构与约束

| 特点 | 含义 | 出处 |
| --- | --- | --- |
| **独立 crate** | 编辑器有独立状态与生命周期、稳定边界、使用方 ≥2 → `crates/editor`；`workbench` 退化为壳层装配 | 架构 §3.2、D17 |
| **依赖只向下** | `workbench → editor → engine / database / shared / gpui-kit`；**editor 不依赖 workbench** | 架构 §3.1 |
| **组件选型不手搓** | 代码编辑器用 `gpui_kit::component::input::Editor`；SQL 高亮走 **sqlglot tokenizer**（不引 tree-sitter）；网格用 `table::DataTable`；标签条优先用 Dock 自带能力 | 原型 §10、架构 D12/D13 |
| **不引入 Zed 编辑器代码** | Zed 的 `editor/text/rope/language` 为 GPL-3.0-or-later（且包身份与依赖闭包不兼容）——**只借鉴设计** | 架构 D17 |
| **不引入 LSP 作为内部协议** | 单进程内 Rust，LSP 只作将来接外部语言服务的适配层 | 架构 D11 |
| **零裸值** | 颜色取主题 token（含待新增的编辑器/笔记本产品语义角色）；尺寸取 `ui.rs` 常量 | 原型 §7 / §8 |

### 工程与文档

| 特点 | 含义 | 出处 |
| --- | --- | --- |
| **纯函数优先** | 语句切分 / 模式判定 / 能力表 / 执行目标解析 / 错误位置解析 / stale 判定 全部拍成纯函数 + 表驱动测试 | 架构 §10 |
| **文档先于实现** | 本轮先出模块入口 + 原型设计 + 架构 + 开发方案 + 交互稿；实现按 Phase 0/1a/1b/1c 推进 | `editor-dev-plan.md` |
| **假底座先修** | 格式化 / 切句 / 事务 / 历史四项在 Phase 0 转真，否则功能"看起来有、实际不可用" | 架构 §7.3 |

## 2. 边界

- **做**：文本/SQL/分析三模式的编辑体验、SQL 补全与诊断、执行族与**执行通道（源库/加速/联邦）**、结果集（含血缘与筛选/分析）、历史、单元与会话、`.rdsnote` 持久化、编辑器相关快捷键与 Action。
- **不做**：连接新建/编辑对话框（M3）、对象树与内省实现（M4）、**表数据浏览/编辑（M4 的 `table_editor` 能力位；本模块只提供只读结果集）**、Mock 生成（M7）、洞察规则（M8）、通用 IDE 能力（Git / 终端 / 调试器）、Jupyter 全协议前端（第二期只做 sidecar 内核适配）。

## 3. 代码地图（规划；括号内为现状）

| 想改 | 去哪 |
| --- | --- |
| 文档 / 模式 / 只读 / 能力表 | `crates/editor/src/{model.rs, mode.rs}` |
| 打开 / 关闭 / 激活 / 保存 / 脏状态 | `crates/editor/src/service.rs` |
| 执行编排（目标解析 / 后台任务 / 回填） | `crates/editor/src/execution.rs`（现状：`crates/workbench/src/panels/` L6777-6998 内联闭包） |
| 执行通道与门控（源库 / 加速 / 联邦） | `crates/editor/src/channel.rs` + `crates/connection/src/secret.rs` + `crates/engine/src/duckdb/federation.rs` |
| 筛选下发 / DuckDB 分析 | `crates/editor/src/execution.rs` + `crates/engine/src/services/execution_service.rs` |
| 网格渲染层（将来可提炼） | `crates/editor/src/view/widgets/grid/`（`GridDataSource` / `GridEditSink`，架构 §3.6） |
| 语句切分（词法级） | `crates/engine/src/sql/split.rs`（✅ 已落地；`SqlEngine::split_statements` + `sql_parser_service::split_sql` 委托） |
| SQL 高亮区间 | `crates/engine/src/sql/highlight.rs`（✅ 已落地：tokenizer → 字节区间 + 类别，不上色） |
| 结果集入库与淘汰 | `crates/editor/src/store.rs` |
| 补全 | `crates/editor/src/completion.rs` + `crates/database/src/metadata_service.rs`（现状零消费） |
| 会话 / 单元 / 输出 | `crates/editor/src/{session.rs, notebook.rs}` |
| 三模式视图 | `crates/editor/src/view/{host.rs}`（✅ 一面板 = 一标签 = 一份文档） |
| SQL 语义着色 | `crates/editor/src/view/highlight.rs`（✅ `TokenClass → 主题词汇`，颜色由主题解析） |
| Action / 快捷键 | `crates/editor/src/commands.rs` + `crates/app/src/main.rs` |
| 中央区装配 | `crates/workbench/src/view.rs::init_workspace` |
| SQL 执行 / 事务 / 取消 | `crates/engine/src/services/sql_service.rs` |
| 历史存储 | `crates/engine/src/persistence/history_store.rs` |
| 格式化 / 转译 | `crates/engine/src/sql/{formatter.rs, transpiler.rs}`（格式化 ✅ 走 sqlglot generator；转译未接线，注意 `transpile` 只吃单条） |
| 编辑器上下文持久化 | `crates/engine/src/persistence/workbench_context_store.rs` |
| 尺寸常量 | `crates/editor/src/ui.rs` |
| 契约测试范围 | `crates/workbench/tests/ui_contract.rs` |

数据链路：`editor（编辑/执行编排）→ engine::SqlService（连接路由 / 取消 / 超时 / 缓存 / 历史）→ 驱动 → 目标库`；补全：`editor::completion → database::MetadataService → 驱动 MetadataBrowser`；结果：`engine::ResultSet → editor::ResultStore → 结果面板 / 内联输出`。**无 HTTP / IPC 层**。

## 4. 改这个模块前必须遵守

1. **执行只经 `EditorService::execute`**：不得在视图/回调用例里自己拼执行（现状 render 内联闭包是反面案例）。
2. **render 零 I/O**：加载 / 执行 / 内省一律后台任务 + 轮询回填。
3. **单一权威**：结果只有 `ResultStore`、脏状态只有 `Document.dirty`、会话只有 `SessionRegistry`；不出现"同一状态两处可写"。
4. **稳定标识**：`document_id` / `cell_id` / `result_id` 永不变，**不参与排序语义、不做下标键**。
5. **位置用锚点/偏移**，不用行号。
6. **只读两维度分开判断**：编辑器只读 ≠ 连接只读。
7. **零裸值**：颜色走主题 token（缺角色先补产品语义 token），尺寸进 `ui.rs`（裸 `px(` 会被契约测试拦下）。
8. **快捷键必须注册**：不在欢迎页/气泡里宣传未注册的键。
9. 注释与文档用简体中文，说明意图与取舍（不复述代码）。
10. `cargo` 命令固定 `-j 2`（DuckDB 静态库并发链接会 OOM）。
11. **高亮区间取原文**：`Token::position` 是**字符**下标（tokenizer 内部 `chars()`），必须换算成字节偏移再切 `&str`；区间要覆盖**原文**（含引号 / 转义 / 注释标记），**不得**按解码后的 `value` 反查（理由与回归见架构 §12 #20）。
12. **用 sqlglot 前先查台账**：原型 §7.4 已逐条记「签名 + 行为级事实 + 接线前置条件」（如 `transpile` 只吃单条、`plan` 只是本地计划、类型标注 / 血缘需 `MappingSchema`）；标 ⚪ 的候选先写验证用例再接线。

## 5. 测试与验证

```sh
# 模块回归（编辑器服务层 + 引擎 SQL 层；含 Phase 0 原语：切分 26 / 高亮 13 / 格式化 7 / 历史 4）
cargo test -p rds-editor -p rds-engine --lib -j 2

# P0.2 / P0.2b / P0.2c 真机探针（顺序亲和 + 驱动级事务 + **并发亲和**；凭据只从环境变量读；共 12 例）
cargo test -p rds-engine --test transaction_affinity -j 2 -- --nocapture --test-threads=1

# 能力探针（原型 §7.4 台账 ⚪ 候选的真实行为；离线、报告式输出）
cargo test -p rds-engine --test sqlglot_capabilities -j 2 -- --nocapture --test-threads=1

# 契约（零裸色 / 零裸 px，扫描 editor + workbench）
cargo test -p rds-workbench --test ui_contract -j 2

# 全工作区编译守卫
cargo check --workspace --all-targets -j 2
```

- 真机回归矩阵：MySQL / PostgreSQL / SQLite / DuckDB × 执行族（当前语句 / 选区 / 全部 / 批量）× 只读 / 可写 × 明暗主题。
- 逐阶段验收场景见 `editor-dev-plan.md` §3（29 条）。

## 6. 文档地图

| 文档 | 什么时候读它 |
| --- | --- |
| `editor-prototype-design.md` | **长什么样 / 怎么交互**：三模式对照与判定规则 / SQL 模式解剖 / 文本模式 / 分析模式（单元解剖与输出类型）/ 交互与快捷键 / 状态与空态矩阵 / 主题映射与尺寸常量 / **§9 与 V1 的逐项对照** |
| `editor-architecture.md` | **为什么这样设计 / 怎么运转**：概念模型与不变式 / 分层与 crate 归属 / 状态所有权 / 八条数据流 / **D1–D20 决策表** / **§7 现状与四个假底座（权威）** / 降级矩阵 / 测试策略 / 实现映射 / **§12 已知问题（权威）** / §13 待确认 |
| `editor-dev-plan.md` | **做什么、做到哪**：Phase 0/1a/1b/1c 任务表与验收 / 测试场景 29 条 / 风险与对策 / 实现位置映射 / 验证命令 |
| `editor-prototype.html` | 可交互示意稿（明暗双主题 × 三模式；含执行、结果集上限、单元运行与 stale 演示） |
| `../database/README.md` | 上游：对象树与内省（M4 提供「在 SQL 编辑器中打开 / 查看数据」入口） |
| `../connection/README.md` | 上游：连接的运行态与只读策略（M3） |

> 缺口：`editor-user-guide.md`（使用手册）按约定应在实现完成后补写——"怎么用"以真实实现为准。

## 7. 下一步

| 类别 | 项 |
| --- | --- |
| 待你拍板（阻塞开工） | 架构 §13 十项；其中必须回答：多文档标签方案 · SQL→分析 转换粒度 · 批量执行语义 · 分析模式首期语言 · 是否独立 crate |
| Phase 0（先做，无 UI） | ✅ 已落地：语句切分 · 历史字段贯通 · crate 骨架 · SQL 高亮 · 格式化选型 · Dock 关闭语义（静态）· **P0.2 三组探针跑完（顺序亲和四库成立；并发下 MySQL/PG 会换连接 → 1b 需 per-session 独占连接；MySQL `BEGIN` 已改文本协议）** · P0.10 台账候选探针已实跑／待你跑：编译基线 P0.9 ／余：驱动层真实 `affected_rows`、驱动填 `column_types`（转 1b） |
| Phase 1a | ✅ **A1–A15 全部完成（含 A9 对话框收尾）**：服务层 · Dock 标签面板 · 内核视图 · SQL 高亮 · 模式切换矩阵（含确认对话框）· 只读两维度 · 状态栏 · 脏状态 · 持久化 + workbench 接线 + 聚焦已存在标签 · Actions 与快捷键 · 查找 / 替换（内核 + 组件库，零自建）· 会话持久化（真 SQLite 实测）· 大文件档位（真 200MB 稀疏文件实测）· 最小执行 + 结果网格（真机四库实测）· ui_contract 契约 · **关闭三态 / 另存为 / 模式切换确认对话框 + 系统文件对话框（`rfd`）**。⬜ 余：A13 的“关折叠”差内核开关 · 工具栏其余控件（执行族 / 格式化 / 历史 / 执行位置 / 连接）随 1b |
| Phase 1b | 执行闭环（"合格的 SQL 客户端"，并关闭 M4 遗留的"查看数据不自动执行"）。**进度**：✅ B16（关闭口径 + 新建入口 + 工具栏执行级）· ✅ B11 + B12（**编辑器成为唯一的 SQL 编辑器**：导航四处改走 `QueryRequest`，旧 `EditorPanel` 的 SQL 区块与内联执行闭包已删，M1 草稿拦截改读 `EditorShared`；“查看数据”现在打开即执行）· 🟡 B1 切片一（连接绑定：文档属性 + 工具栏选择器 + 状态栏 + 执行真的用它）——余：绑定随会话持久化 · ⬜ B2 执行族余项（批量 / 新结果标签）· B3 中断超时 · B4 事务 · B5/B5b 结果区与分段抓取 · B6 错误定位 · B7 导出 · B8 历史面板 · B9 补全 · B10 格式化/转译/执行计划 · B13 执行通道 · B14 筛选下发 · B15 DuckDB 分析入口 · 项目只读模式对执行的拦截 |
| Phase 1c | 分析模式骨架（Cell/Output/Session，仅 SQL + Markdown 单元） |
| Phase 2 | Python / Rust 内核 · Arrow 变量桥 · 富输出 · `.ipynb` 互操作 |
