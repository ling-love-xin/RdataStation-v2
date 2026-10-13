# SQL 编辑器模块 · 模块入口

> **宣传页（一页看懂）**：[`editor-showcase.html`](editor-showcase.html)（视觉版，明暗双主题、离线可开）· [`editor-showcase.md`](editor-showcase.md)（可贴版，适合贴进 PR / wiki）
>
> **一句话**：一个内核、三档能力——文本模式是"不与数据库通信的记事本"，SQL 模式是 DBeaver 一档的脚本窗口（连接 + 执行 + 结果 + 历史），分析模式把文档变成**单元 + 会话 + 输出**的可执行笔记；核心始终是 SQL。
>
> 本文只提炼**特点 / 边界 / 代码地图 / 硬约束**；细节一律指向本目录内文档，**不复制设计**。
> 状态（2026-10-13）：**Phase 0 + 1a + 1b 已完成**——执行闭环（执行族五项 / 分段抓取 / 中断与超时 / 事务 / 结果区 / 历史）、**三档执行通道**（源库 / 本地加速 / 联邦全部真接通，含 T1.6 联邦档「源清单 ▾」入口）、导出五档（含 Parquet / XLSX 经 DuckDB `COPY`）、B9 补全（打字即补 + `Ctrl+Space` + 模板片段）、B10 格式化 / 转译 / 执行计划、B15 本地 DuckDB 分析（预置菜单 + 自定义 SQL + 血缘摘要）均已落地。**Phase 2a 结果集就地编辑已落地**（双击脏格 → 提交变更，六道防线 + 只做 UPDATE，详见架构 §6 D23）；**Phase 2c 全部完成**（T2c-1~6：Schema 面包屑 / 方言段 / 英文微标 / 重设菜单 / 工具栏分组自定义 / 面板布局持久化，514 tests passed）；**T2c-1 验收收尾「切 schema 真影响执行」已落地**（2026-10-13，rds-editor 516 / rds-engine 532 测绿，见任务书 T2c-1 补记）。**分析模式的笔记本视图属 1c/2d，尚未开工**。已知问题与排期的**唯一权威**是架构文档 §12 与 `editor-impl-taskbook.md`。
>
> **边界**：本模块拥有**编辑与执行**（编辑器内核 / 三模式 / 执行族 / 结果集 / 历史 / 单元与会话）。连接的新建与编辑属 M3；对象树与元数据内省属 M4；Mock 属 M7；洞察属 M8；DuckDB 分析资产属 M6。本模块只经服务/命令与它们协作（契约见架构 §3.4）。

## 1. 模块特点

### 产品行为

| 特点 | 含义 | 出处 |
| --- | --- | --- |
| **一个内核 + 三档能力** | 文本 ⊂ SQL ⊂ 分析，能力严格递进；模式只是"挂哪些服务、显示哪些 chrome、结果去哪"，**不做三套编辑器**（注：Phase 2b 拟把"文本模式"更名为"通用模式"并与 SQL 模式平行而非严格子集，现代码仍为 `EditorMode::Text`，口径以任务书为准） | 架构 D1、原型 §1.1 |
| **模式可判定、切换有代价** | 显式 > 扩展名 > 入口语义；上/降级是显式动作（SQL→分析 需确认转换粒度），**禁止静默改字段** | 原型 §1.2 / §1.3、架构 D2/D3 |
| **只读是两个维度** | 编辑器只读（不可输入）与连接只读（可输入、写语句被拦）分别表达 | 原型 §1.4、架构 D4 |
| **折叠候选自算、机制用现成的** | 折什么由我们的纯函数算（括号组 / `CASE`·`BEGIN`·`IF` 块 / 多行注释与字面量），折起来的样子、chevron、点击折叠、开关都用内核现成的；**候选随文本重喂**（内核不会替我们平移） | 架构 D26、`fold.rs` / `view/fold.rs` |
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
| **结果集有条件可写**（Phase 2a 起） | 结果网格支持双击就地编辑并提交写回源库，仅做 UPDATE、受六道防线门控（通道/项目/编辑器/连接只读、无行键、列缺失）；新增/删除行属二期；独立表浏览面板（M4）仍不做。详见架构 §6 D23（2026-10-12 修订） | 架构 D23 / §3.6 |

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
- **不做**：连接新建/编辑对话框（M3）、对象树与内省实现（M4）、**独立表数据浏览面板（M4；结果集就地编辑已在 Phase 2a 落地，但不等于独立表浏览能力，驱动级 `table_editor` 能力位仍为 NotBuilt——此语义待定，见架构 §13）**、Mock 生成（M7）、洞察规则（M8）、通用 IDE 能力（Git / 终端 / 调试器）、Jupyter 全协议前端（第二期只做 sidecar 内核适配）。

## 3. 代码地图（规划；括号内为现状）

| 想改 | 去哪 |
| --- | --- |
| 文档 / 模式 / 只读 / 能力表 | `crates/editor/src/{model.rs, mode.rs}` |
| 打开 / 关闭 / 激活 / 保存 / 脏状态 | `crates/editor/src/service.rs` |
| 执行编排（目标解析 / 后台任务 / 回填 / 提交变更） | `crates/editor/src/execution.rs`（✅ 已落地：`ExecTarget` 含 `Commit`，提交在宿主 `workbench/src/services/editor_exec.rs::commit_changes` 落库） |
| 执行通道与门控（源库 / 加速 / 联邦） | `crates/editor/src/channel.rs` + `crates/connection/src/secret.rs` + `crates/engine/src/duckdb/federation.rs` |
| 筛选下发 / DuckDB 分析 | `crates/editor/src/execution.rs` + `crates/engine/src/services/execution_service.rs` |
| 结果网格（渲染 + 就地编辑） | `crates/editor/src/view/results/grid.rs`（✅ 已落地；`GridEditSink` 形状的编辑逻辑在 `changes.rs` / `editability.rs` / `execution.rs`，**未抽公开 trait**，架构 §3.6） |
| 语句切分（词法级） | `crates/engine/src/sql/split.rs`（✅ 已落地；`SqlEngine::split_statements` + `sql_parser_service::split_sql` 委托） |
| SQL 高亮区间 | `crates/engine/src/sql/highlight.rs`（✅ 已落地：tokenizer → 字节区间 + 类别，不上色） |
| 结果集入库与淘汰 | `crates/editor/src/store.rs` |
| 补全 | `crates/editor/src/completion.rs` + `crates/database/src/metadata_service.rs`（✅ B9 已消费；`primary_key` 方法供结果集编辑门控） |
| 会话 / 单元 / 输出 | `crates/editor/src/session.rs`（✅ 已落地）；`notebook.rs` **未建**（1c/2d 范围） |
| 三模式视图 | `crates/editor/src/view/{host.rs}`（✅ 一面板 = 一标签 = 一份文档） |
| SQL 语义着色 | `crates/editor/src/view/highlight.rs`（✅ `TokenClass → 主题词汇`，颜色由主题解析） |
| 折叠（候选纯函数 / 喂内核） | `crates/editor/src/fold.rs`（✅ 纯函数：括号组 / `CASE`·`BEGIN`·`IF` → `END` / 多行注释与字面量；档位与词法边界都在模块头）+ `crates/editor/src/view/fold.rs`（✅ 喂候选：构造一次 + 每次文本变更后重喂） |
| Action / 快捷键 | `crates/editor/src/commands.rs` + `crates/app/src/main.rs` |
| 中央区装配 | `crates/workbench/src/view.rs::init_workspace` |
| SQL 执行 / 事务 / 取消 | `crates/engine/src/services/sql_service.rs` |
| 历史存储 | `crates/engine/src/persistence/history_store.rs` |
| 格式化 / 转译 / 执行计划 | `crates/engine/src/sql/{formatter.rs, script.rs, transpiler.rs, explain.rs}`（✅ B10 三个都接线了）：格式化与转译共用 **`script.rs`** 的“切分 → 逐条改写 → 原位回填”（区间外一个字节不动）；转译**先切分再逐条**（整篇 `transpile` 对脚本会静默丢语句）；执行计划的前缀按方言生成。编辑器侧的“改哪一段 / 光标去哪”分别在 `crates/editor/src/{format.rs, translate.rs}` |
| 编辑器上下文持久化 | `crates/engine/src/persistence/workbench_context_store.rs` |
| 尺寸常量 | `crates/editor/src/ui.rs` |
| 结果集就地编辑（Phase 2a） | `crates/editor/src/{changes.rs, editability.rs}`（变更集三态 / 六道防线纯函数）+ `store.rs`（`DocumentResults.changes`、行键）+ `view/host.rs`（双击入口 / 提交·回滚按钮） |
| Schema 面包屑 / 方言 / 状态栏（2c） | `crates/editor/src/connection.rs`（`SchemasPort`）+ `view/widgets/status_bar.rs` + `language_service.rs` |
| 联邦源清单（T1.6） | `crates/editor/src/sources.rs` + `view/host.rs::render_sources_picker`（仅联邦档） |
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
10. `cargo` 命令固定 `-j 2`（并发链接重型 crate 会 OOM（DuckDB 已改动态链接））。
11. **高亮区间取原文**：`Token::position` 是**字符**下标（tokenizer 内部 `chars()`），必须换算成字节偏移再切 `&str`；区间要覆盖**原文**（含引号 / 转义 / 注释标记），**不得**按解码后的 `value` 反查（理由与回归见架构 §12 #20）。
12. **用 sqlglot 前先查台账**：原型 §7.4 已逐条记「签名 + 行为级事实 + 接线前置条件」（如 `transpile` 只吃单条、`plan` 只是本地计划、类型标注 / 血缘需 `MappingSchema`）；标 ⚪ 的候选先写验证用例再接线。

## 5. 测试与验证

```sh
# 模块回归（编辑器服务层 + 引擎 SQL 层；含 Phase 0 原语：切分 26 / 高亮 13 / 格式化 9 / 历史 4；B17 折叠纯函数 14）
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
| `editor-architecture.md` | **为什么这样设计 / 怎么运转**：概念模型与不变式 / 分层与 crate 归属 / 状态所有权 / 八条数据流 / **D1–D26 决策表**（D23 已于 2026-10-12 修订）/ **§7 现状与四个假底座（权威）** / 降级矩阵 / 测试策略 / 实现映射 / **§12 已知问题（权威）** / §13 待确认 |
| `editor-dev-plan.md` | **做什么、做到哪**：Phase 0/1a/1b/1c 任务表与验收 / 测试场景 29 条 / 风险与对策 / 实现位置映射 / 验证命令 |
| `editor-user-guide.md` | **怎么用**：能力与期次标注 / 入口 / 界面导览（工具栏三层 · 编辑区 · 结果区 · 状态栏）/ 典型流程（执行族 · 结果集七件事含就地改数据 · 格式化转译执行计划 · 补全模板 · 事务中断 · 三通道与联邦源选择 · 历史 · 草稿箱闭环）/ 一次操作落到哪 / 快捷键（已注册的）/ 只读与安全（含结果集编辑六道防线）/ FAQ 排查 / **USIT 验收清单** |
| `editor-prototype.html` | 可交互示意稿（明暗双主题 × 三模式；含执行、结果集上限、单元运行与 stale 演示） |
| `editor-showcase.html` / `editor-showcase.md` | **宣传页（一页看懂）**：特点 / 流程 / 架构 / 质量证据一页扫完；改了实现请顺手核对页里的关键数字 |
| `../database/README.md` | 上游：对象树与内省（M4 提供「在 SQL 编辑器中打开 / 查看数据」入口） |
| `../connection/README.md` | 上游：连接的运行态与只读策略（M3） |

## 7. 下一步

| 类别 | 项 |
| --- | --- |
| 待你拍板（阻塞开工） | 架构 §13 十项；其中必须回答：多文档标签方案 · SQL→分析 转换粒度 · 批量执行语义 · 分析模式首期语言 · 是否独立 crate |
| Phase 0（先做，无 UI） | ✅ 已落地：语句切分 · 历史字段贯通 · crate 骨架 · SQL 高亮 · 格式化选型 · Dock 关闭语义（静态）· **P0.2 三组探针跑完（顺序亲和四库成立；并发下 MySQL/PG 会换连接 → 1b 需 per-session 独占连接；MySQL `BEGIN` 已改文本协议）** · P0.10 台账候选探针已实跑／待你跑：编译基线 P0.9 ／余：驱动层真实 `affected_rows`、驱动填 `column_types`（转 1b） |
| Phase 1a | ✅ **A1–A15 全部完成（含 A9 对话框收尾）**：服务层 · Dock 标签面板 · 内核视图 · SQL 高亮 · 模式切换矩阵（含确认对话框）· 只读两维度 · 状态栏 · 脏状态 · 持久化 + workbench 接线 + 聚焦已存在标签 · Actions 与快捷键 · 查找 / 替换（内核 + 组件库，零自建）· 会话持久化（真 SQLite 实测）· 大文件档位（真 200MB 稀疏文件实测）· 最小执行 + 结果网格（真机四库实测）· ui_contract 契约 · **关闭三态 / 另存为 / 模式切换确认对话框 + 系统文件对话框（`rfd`）**。⬜ 余：A13 的“关折叠”差内核开关 · 工具栏其余控件（执行族 / 格式化 / 历史 / 执行位置 / 连接）随 1b |
| Phase 1b | 执行闭环（"合格的 SQL 客户端"，并关闭 M4 遗留的"查看数据不自动执行"）。**进度**：✅ B16（关闭口径 + 新建入口 + 工具栏执行级）· ✅ B11 + B12（**编辑器成为唯一的 SQL 编辑器**：导航四处改走 `QueryRequest`，旧 `EditorPanel` 的 SQL 区块与内联执行闭包已删，M1 草稿拦截改读 `EditorShared`；“查看数据”现在打开即执行）· ✅ B14（筛选 / 排序本地那档 + 下发源库那档都收圆）· ✅ B8（历史面板在右 Dock）· ✅ B7 / B5 / B5b / B6 / B2 / B3 / B4（导出 / 结果区 / 分段抓取 / 错误定位 / 执行族 / 中断 / 事务）· ✅ **B10 格式化 / 转译 / 执行计划（三个切片全完成）**：格式化与转译共用脚本骨架（**脚本一条不丢**）· 工具栏「格式化」+ `Ctrl+Shift+F` · 「⋯ 更多 ▾ ▸ 转译为」十个目标 · 执行计划**按通道取方言**且落新结果集贴标题 · 🟡 B13 执行通道（**切片一 / 二 / 三前半已完成**：三档互斥 + 门控真值 + 随会话持久化 + 切通道旧结果标灰与提示 + **加速档真接通**（`ATTACH … READ_ONLY` / `USE` 解析表名 / 四类执行全分流）+ **「重新挂载源库」入口** + **历史带通道字段** + **T1.6 联邦档「源清单 ▾」入口（2026-09-18 已完成，`sources.rs` / `render_sources_picker`）**）· ✅ **B1（连接绑定，切片一 + 收尾 2026-09-18）**：文档属性 + 工具栏选择器 + 状态栏 + 执行真的用它 + **绑定跨重启**（`editor_contexts.connection_id` 写真值；恢复时校验连接还在不在，不在就回退并说原因）；⬜ 余：只读锁 ⓘ（要连接侧写策略来源） · ✅ **项目只读对执行的拦截已补回（2026-09-18：`editor/src/project.rs` 端口 + 提交前拒写，判据与通道闸同一份）** · ✅ **B9 补全（两切片全完成 2026-09-18）**：打字触发 + **`Ctrl+Space` 手动触发**（与打字那条同一份候选）· 内建 provider（零自建下拉）· 候选来自导航缓存（后台预载 + 按通道限定名：联邦是 `别名.schema.表`）· **模板片段**（「⋯ 更多 ▾ ▸ 插入模板」，插到光标处并选中 `{table}`）；⬜ 余：缓存未命中时回落实时内省 · 🟡 **B15 DuckDB 分析（切片一 / 二 + 血缘已完成 2026-09-18）**：结果集「⚗ 分析 ▾」→ 本地内存 DuckDB 上对**已抓到的行**跑聚合（**不重跑源库**，上限 2 万行并如实说明）→ **新结果集**（原结果一行不动、不抢选中）；预置菜单 = 计数行数 / 逐列分组计数，底部「自定义分析 SQL…」弹输入框（预填模板，`{table}` 指桥接出来的临时表，空白在本地就拒）；结果工具栏左段带**来源摘要**（`原查询` / `下发筛选` / `排序下发` / `取下一段` / 标题中的执行计划 / 分析）· ✅ **B7 导出两切片全完成（2026-09-18）**：CSV / JSON / INSERT 编辑侧编码 + **Parquet / XLSX 经 DuckDB `COPY`**（一次性线程 + 回执，不堵 UI：XLSX 首次要装 excel 扩展；`HEADER true` 必须显式传）；⬜ 余：列类型化输出（CSV / JSON 仍是展示文本）· 沉淀为分析单元（1c 已搁置） |
| Phase 2a | ✅ **结果集就地编辑已落地（2026-10-12，T2a-1~7）**：双击脏格 → 「提交变更」写回源库（独立事务逐条 UPDATE / 失败回滚 / 0 行判冲突），六道防线门控；详见架构 §6 D23 与任务书。**未做**：新增/删除行、类型化编辑器、独立表浏览面板 |
| Phase 2c | ✅ **全部完成（514 tests passed）+ T2c-1 验收收尾已落地（2026-10-13，rds-editor 516 / rds-engine 532 测绿）**：✅ T2c-1 Schema 面包屑（绑定随会话持久化；**切 schema 已真影响执行**——执行链逐执行携带文档 schema + 引擎会话上下文注入（SET search_path / USE），未限定表名落到所选 schema，见任务书 T2c-1 补记）· ✅ T2c-2 方言段 · ✅ T2c-3 英文微标 + 行｜列入口（列编辑置灰，gpui-base 0.7.1 接口为 `pub(super)`）· ✅ T2c-4 重设菜单（缩进真改内核 + 随会话持久化；编码/换行置灰）· ✅ T2c-5 工具栏分组自定义（最右 `···` 三栏面板 / 组折叠 / 分隔符拆分 / 重置默认）· ✅ T2c-6 面板布局持久化（布局/schema/缩进跨会话，见任务书）。权威排期见 `editor-impl-taskbook.md` |
| Phase 1c / 2d | 分析模式骨架（Cell/Output/Session，仅 SQL + Markdown 单元，即任务书 2d；`notebook.rs` / `notebook_view.rs` 未建），按计划后置 |
| Phase 2（远期） | Python / Rust 内核 · Arrow 变量桥 · 富输出 · `.ipynb` 互操作 |
