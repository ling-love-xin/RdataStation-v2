# SQL 编辑器模块 · 开发方案（Phase 0 / 1a / 1b / 1c）

> 状态：**方案待确认（2026-09-15）**——**尚未开始实现**，本轮只产出文档（设计 / 架构 / 原型 / 交互稿）。
> 关联：`editor-prototype-design.md`（长什么样）、`editor-architecture.md`（为什么这样设计，§7 现状与 §12 已知问题为权威）、`editor-prototype.html`（交互稿）。
> 开工前必须先关闭 `editor-architecture.md` §13 的待确认项；其中 #1（Dock 标签能力）与 #3（格式化选型）是 Phase 0 的第一批动作。

---

## 0. 进度记录（最近在前）

| 日期 | 内容 | 状态 |
| --- | --- | --- |
| 2026-09-15（提交 + 台账二次核验） | **模块已提交**（`ac0d75f`，24 文件 / 5625 行：文档 5 件 + `crates/editor` 骨架 + engine SQL 原语 + 历史字段 + 事务探针；暂存时避开同工作区其它会话的在途改动，混文件 `Cargo.lock` / `sql/engine.rs` / `docs/architecture/README.md` 按 hunk 级分离）· **sqlglot 台账二次核验（读实现）**：修正 4 处（关键字是**逐词枚举变体** / `Token::position` 是**字符**下标 / `transpile` **只吃单条** / `builder` 证据行号），补 `Schema`+`MappingSchema`、`qualify_columns`、`optimize`、`plan`（仅本地计划）、`executor`（不采用）等条目（原型 §7.4） · **修正一个真缺陷**：高亮区间改为「字符偏移 → 字节偏移 + 取原文区间」（原实现按 `value` 回查，转义字符串落空、引号与注释标记取不到）→ 架构 §12 #20 | ✅ 已完成（编译验证待本机执行，见 §6） |
| 2026-09-15 | 文档阶段：模块入口 + 原型设计 + 架构（含现状盘点与四个假底座）+ 本方案 + 交互稿（`editor-prototype.html`，1554 行，24 条交互断言） | ✅ 已完成 |
| 2026-09-15（修订） | 按评审意见补充：**执行通道与执行族正交**（源库/加速/联邦）· 结果区从三模式过滤收敛为「筛选（本地/下发）+ DuckDB 分析」· **结果血缘与通道徽标** · 结果集只读边界与网格归属（架构 §3.6）· 任务表新增 B13/B14/B15/C9 与测试场景 30–36 | ✅ 已完成 |
| 2026-09-15（Phase 0 第一批） | **P0.4 语句切分**：`engine::sql::split`（词法级状态机 + 26 项表驱动测试）+ `SqlEngine::split_statements` + `sql_parser_service::split_sql` 委托重写 · **P0.5 历史字段**：`SqlHistoryEntry` + `save_sql_history(_into)`（耗时/成功/失败原因/行数真实，失败也留痕）+ 4 项单测 · **P0.7 骨架**：新建 `crates/editor`（`model` 能力表 + `mode` 判定规则表 + README）+ workspace 登记 · `QueryResult::from_batches` 不再把 `total_rows` 当作 `affected_rows` | ✅ 已完成（编译验证待本机执行，见 §6） |
| 2026-09-15（Phase 0 待办） | P0.1 Dock 标签能力验证 · P0.2 事务会话亲和验证 · P0.3 格式化选型 · P0.6 驱动层真实 `affected_rows` · P0.8 SQL 高亮注册 · P0.9 平台基线 | ⬜ 未开始 |
| 2026-09-15（P0.3 完成） | **格式化不再依赖新库**：sqlglot-rust 已自带 `generate_pretty`；`SqlEngine::format` 改为“多语句逐条生成 + `;\n\n` 拼接”，解析失败原样返回；旧测试从“非空”升级为 7 项回归 | ✅ 已完成 |
| 2026-09-15（P0.8 完成 · P0.2 探针就绪） | **P0.8 换路线并完成**：不再引 tree-sitter，改用 sqlglot tokenizer → `engine/src/sql/highlight.rs`（字节区间 + 类别，**13 项单测**）· **P0.2 探针就绪**：`crates/engine/tests/transaction_affinity.rs`（四类库、环境变量注入、临时表作亲和判据、不改用户数据） | ✅ P0.8 完成；P0.2 待真机运行 |
| 2026-09-15（评审决策 + P0.1 静态核实） | **分段抓取已确认纳入 1b**（新增 B5b + 测试场景 37–39）· 自动刷新/钉住 1b 顺带 · **P0.1 结论（静态）**：Dock **没有“关闭前否决”钩子**（`DockArea` 收到 `TabGroupEvent::ClosePanel` 后直接 `remove_panel_id`；`Panel::closable(cx)` 是唯一闸门；`with_renderer` / `remove_panel` 均为 `pub`）→ 关闭语义默认走“草稿兜底”（架构 §13 #15） | ✅ 已完成 |
| — | Phase 0 地基与技术验证 | ⬜ 未开始 |
| — | Phase 1a 内核 + 文本/SQL 编辑体验 + 最小执行 | ⬜ 未开始 |
| — | Phase 1b SQL 执行闭环（DBeaver 一档） | ⬜ 未开始 |
| — | Phase 1c 分析模式骨架（Cell/Output/Session，仅 SQL 单元） | ⬜ 未开始 |
| — | Phase 2 Python/Rust 内核 · 变量桥 · 富输出 · `.ipynb` | ⬜ 未开始 |

---

## 1. 现状结论（盘点摘要）

| 层 | 状态 | 证据 |
| --- | --- | --- |
| 编辑器视图 | ⛔ 只有一个 `Textarea`（无高亮/行号/补全），且仅在 `use_duckdb_fed` 连接下出现 | `crates/workbench/src/panels.rs` L6094 / L6809 / L6779 |
| 执行链路 | ⛔ 同步直连全局分析库文件，与连接无关、无取消/超时/事务/批量；且内联在 render 闭包内（违反 render 零 I/O） | 同上 L6821-6825；`services/query_runner.rs` |
| 结果区 | ⛔ 定宽 `div` 表格（无虚拟化/排序/过滤/分页） | 同上 L6921-6950 |
| 历史 / 导出 | 🟡 20 条 JSON 回填 / 仅 CSV | `services/query_history.rs`、`services/query_export.rs` |
| 多文档 / 快捷键 / Action | ⛔ 无 | `view.rs` L302、`commands.rs`、`app/src/main.rs` L80-110 |
| engine SQL 能力（解析/执行/历史/模板/上下文） | ✅ 已迁移但**多数零接线** | 架构 §7.2 |
| 元数据内省（补全数据源） | ✅ 已迁移（异步）但零消费 | `crates/database/src/metadata_service.rs` |
| 四个假底座（格式化/切句/事务/历史字段） | 🔴 必须先修 | 架构 §7.3 |

**关键缺口**：① 编辑器本体（内核 + 三模式）；② 执行管线（抽取 + 后台化）；③ 语句切分与格式化重做；④ 事务与历史真实化；⑤ 分析模式的模型（Cell/Output/Session）。

---

## 2. 阶段划分

```
Phase 0（地基，无 UI）
   ├─ 技术验证：Dock 标签能力 / 事务会话亲和 / 格式化选型
   ├─ 后端修复：语句切分 · 历史字段 · affected_rows
   ├─ 结构：crates/editor 骨架 + 依赖接线
   └─ 验证：SQL 语法高亮注册（tree-sitter-sql）
        │
        ├──► Phase 1a（内核 + 文本模式 + SQL 编辑体验 + 最小执行）→ 交付「能用的编辑器」
        │        │
        │        └──► Phase 1b（执行闭环：选区/当前语句/批量/取消/事务/结果区/历史/导出）→ 交付「合格的 SQL 客户端」
        │                 │
        │                 └──► Phase 1c（Cell/Output/Session + 内联输出 + .rdsnote）→ 交付「分析笔记骨架」
        │                          │
        │                          └──► Phase 2（Python/Rust 内核 · 变量桥 · 富输出 · 互操作）
```

**为什么要分 1a/1b/1c 而不是一次做完**：1a 结束即可自证价值（编辑器可用）；1b 与 1a 的验证手段完全不同（前者靠窗口/单测，后者靠真实端点回归）；1c 的复杂度在"会话与输出"，与"编辑体验/执行语义"耦合会互相拖慢。

---

### Phase 0 — 地基与技术验证（不含 UI）

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| P0.1 | **Dock 标签能力验证**：`Panel::{title, title_suffix, closable}` 能否承载脏点与“关闭前确认” | ✅ **静态结论已出（2026-09-15）**：脏点走 `title_suffix` 可行；**关闭无否决钩子**（`DockArea` :1257 收到 `TabGroupEvent::ClosePanel` 即 `remove_panel_id`；`closable(cx)` 只是静态许可）。可选路径：① 草稿兜底（默认）② 自绘标签条 + `with_renderer` + 确认后 `remove_panel` | 结论已写回架构 §12 #18 / §13 #15；运行期探针（脏点渲染 + 两个关闭路径）随 1a 的 A2 一并验证 |
| P0.2 | **事务会话亲和验证**：连接池下 `BEGIN` 与后续语句是否同一物理连接（临时表作判据，四类库均可跑） | ✅ **探针已就绪**：`crates/engine/tests/transaction_affinity.rs`（环境变量注入，未设置则跳过） | 运行：`cargo test -p rds-engine --test transaction_affinity -j 2 -- --nocapture --test-threads=1`（环境变量见 §6）；输出 “会话亲和成立 / 不成立 / 驱动未填充 rows” 三种结论，回写架构 §12 #2 |
| P0.3 | **格式化实现选型** | ✅ **选定（2026-09-15）**：sqlglot-rust 自带 `generate_pretty` + `parse_statements_with_comments`（**无需新依赖**） | `engine/src/sql/formatter.rs` 重写；回归从“非空”升级为 7 项（非 Debug 打印 / 可再解析 / 多语句不丢 / 失败原样返回 / 空输入 / 前导注释 / 各方言参数）；残留：行内注释丢失（**已源码核实** `gen_statement` 只 emit 前导注释） |
| P0.4 | **语句切分实现**：词法级扫描器（`'…'` `"…"` `` `…` `` `--` `/* */` `$$…$$`），返回 `Vec<SqlStatement{start,end,line}>` | **`crates/engine/src/sql/split.rs`**（✅ 已完成）+ `SqlEngine::split_statements` + `sql_parser_service::split_sql` 委托 | 表驱动单测 26 项（含各方言字面量/注释/嵌套/占位符/多字节/未闭合/行号）；`psql` 过程体 `$$` 块不被切开 |
| P0.5 | **历史字段贯通**：`save_sql_history` 增参（`elapsed_ms / success / error / rows_affected / rows_returned / db_type`）+ 失败路径也写 | ✅ `engine/src/persistence/history_store.rs`（`SqlHistoryEntry` + `save_sql_history` / `save_sql_history_into`）、`engine/src/services/sql_service.rs`（成功/失败双路径写入 + `db_type_of` 助手） | 单测：成功/失败各写一条且字段真实（耗时 > 0、成功标志正确、失败含原因）；存储可注入（`*_into`）不碰用户真实历史 |
| P0.6 | **影响行数**：`QueryResult::from_batches` 不再把 `total_rows` 当作 `affected_rows`（✅）；历史在写语句上记 `rows_affected`（已接线） | `crates/shared/src/models.rs`、`engine/src/services/sql_service.rs` | 遗留（转 1b）：**驱动层返回真实 affected_rows**（见架构 §12 #17） |
| P0.7 | **crate 骨架与依赖接线**：`crates/editor`（lib + `model` 能力表 + `mode` 判定表 + README）、workspace 依赖唯一入口登记 | ✅ `Cargo.toml`（members + 依赖别名）、`crates/editor/{Cargo.toml,README.md,src/*}` | `cargo check --workspace --all-targets -j 2` 零告警；`cargo test -p rds-editor --lib` 全绿（workbench → editor 的依赖线等到 1a 首次使用再连，避免死依赖） |
| P0.8 | **SQL 高亮注册验证** | ✅ **方案已变且已落地（2026-09-15）**：改用 **sqlglot tokenizer**（`sqlglot_rust::tokens`，带注释与行列位）→ `engine/src/sql/highlight.rs` 产出「字节区间 + 类别」，**不需要 tree-sitter、不需要联网取包** | 单测 **13 项**（关键字/类型/函数/字符串含引号/**转义字符串**/数字/占位符/注释/标点运算符/升序不重叠/**中文 SQL 字节区间**/未闭合降级/区间助手）；视图层只负责“类别 → 主题色”。行为级事实已记原型 §7.4（空白被丢弃、关键字逐词变体、`position` 是字符下标、`quote_char` 只给带引号标识符） |
| P0.9 | **平台与性能基线**：记录编译时间增量（新增依赖后 `cargo build -p rds-app`）与二进制体积变化 | — | 数据写回本文件 §6 |

> Phase 0 结束时：后端"假底座"全部转真 + 结构就位 + 三个技术未知消除。

---

### Phase 1a — 内核 + 文本模式 + SQL 编辑体验（+ 最小执行）

**目标**：打开 `.sql` / `.txt` 就能得到"键盘优先、状态可信"的编辑体验；`Ctrl+Enter` 能把整篇 SQL 跑出结果（结果区先做基础版）。**此阶段结束即"能用的编辑器"**。

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| A1 | `EditorService`：打开/关闭/激活/重命名文档；`DocumentId` 稳定生成；文档集合状态 | `crates/editor/src/{service.rs, model.rs}` | 服务层单测：打开/关闭/激活/去重（同一路径重复打开=激活） |
| A2 | **多文档标签条**（依 P0.1 结论）：标签、脏点、关闭、`+` 新建、溢出处理 | `crates/editor/src/view/host.rs` | 窗口测试：3 个文档并存、脏点显示、关闭拦截触发 |
| A3 | 内核视图接入：`input::editor::Editor` + `EditorState` 每文档一份 + 行号 + 只读态 | `crates/editor/src/view/{text_mode.rs, sql_mode.rs}` | 窗口测试：文本可输入、只读不可输入 |
| A4 | **SQL 高亮**：注册 grammar + 方言选择 + 主题角色映射（缺角色复用最接近标准 token） | `crates/editor/src/view/` + 主题资产 | 真机核对明暗两套下 keyword/string/number/comment/function 可辨 |
| A5 | **模式判定与切换**：规则表（§1.2）+ 切换矩阵（§1.3，含确认对话框与内容变换）；能力表驱动 chrome | `crates/editor/src/mode.rs` + `view/*` | 单测：判定表全覆盖（含 `.rdsnote`/`.sql`/`.txt`/未知/入口语义）；窗口测试：切换确认分支 |
| A6 | **只读两维度**：编辑器只读（文档属性）与连接只读（策略）分别表达 | `model.rs` + 视图 + 状态栏 | 窗口测试：三种组合的渲染与禁用态正确 |
| A7 | **编辑器状态栏**（真实值）：Ln/Col、选区字数、语句数（来自 `split.rs`）、方言、编码/换行/缩进、模式、脏 | `crates/editor/src/view/widgets/status_bar.rs` | 断言：语句数随内容变化（不再恒 1）；无硬编码文案 |
| A8 | **脏状态**：输入事件 → 与 baseline 比较 → 置脏/清脏；标签脏点与状态栏同步 | `service.rs` + `view/host.rs` | 单测：编辑→脏、撤销回原值→干净、保存→干净 |
| A9 | 保存 / 另存为 / 外部修改检测 / 关闭三态确认（保存失败二次确认） | `service.rs` + `persist.rs` + 对话框 | 单测 + 窗口测试：三态分支、外部修改分支 |
| A10 | Actions 与快捷键：`Ctrl+S` / `Ctrl+Enter`（最小执行=执行全部）/ `Ctrl+/` / `Ctrl+F` / `Ctrl+Shift+F` | `commands.rs` + `app/src/main.rs` | 真机：全部按键生效（**禁止"只宣传未注册"**） |
| A11 | 查找 / 替换（优先用组件能力，缺则自建） | `view/widgets/` | 真机：查找高亮、替换、跳转 |
| A12 | **工作区上下文持久化**：光标/选区/模式/连接绑定落 `workbench_context_store`（扩展表字段） | `crates/editor/src/persist.rs` + `engine` 迁移 | 集成测试：关闭重开恢复光标与模式 |
| A13 | 大文件档位：>50MB 关补全/折叠、>200MB 只读提示卡（分块加载推到 1b 后） | `view/*` | 单测：档位判定纯函数；真机：200MB 文件不崩 |
| A14 | 最小执行 + 基础结果网格（`DataTable`）：执行全部 → 结果集 → 表格 + 行数/耗时 | `crates/editor/src/execution.rs` + `view/widgets/result_grid.rs` | 集成测试：执行 → 结果入库 → 渲染；真机：DuckDB/SQLite/MySQL/PG 各一次 SELECT |
| A15 | 契约与回归：`ui_contract` 扫描范围扩到 `crates/editor/src` | `crates/workbench/tests/ui_contract.rs` | `cargo test -p rds-workbench --test ui_contract -j 2` 全绿 |

**1a 验收口径**：能新建/打开/编辑/保存 `.sql` 与 `.txt`；SQL 有高亮与真实状态栏；模式可判定可显式切换；`Ctrl+Enter` 能跑通一次查询并看到表格结果。

---

### Phase 1b — SQL 执行闭环（DBeaver 一档）

**目标**：把"执行"这件事做合格——目标可选、能中断、有事务、有历史、结果可操作、错误能定位；并对外开放自动执行接口（关闭 M4 遗留项）。

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| B1 | 连接绑定与切换：选择器（P/G/GP 短码 + 运行态）、自动建连（同 M4 口径）、失败可读原因 | `view/sql_mode.rs` + `services`（复用 M3/M4 加载器） | 真机：4 类数据库切换；未运行连接自动建连成功 |
| B2 | **执行族**：执行当前语句（用 `split.rs` 定位）/ 选区（有选区优先）/ 全部 / 批量（逐条独立） | `execution.rs` | 单测：目标解析（光标位置 × 选区 × 空文档）；集成：批量 3 语句 → 3 结果集，含 1 失败不中断 |
| B3 | 取消与超时：中断按钮 + `Ctrl+Break` 等价入口；超时按连接的 `timeout_ms` | `execution.rs` + `engine::SqlService::cancel_query` | 真机：长查询中断返回；超时返回可读错误 |
| B4 | **事务**（依 P0.2 结论）：真实状态机 + 状态栏 TX 区（开启时长 / 提交 / 回滚 / 自动提交开关） | `engine::SqlService` + `view/widgets/status_bar.rs` | 集成：begin → insert → rollback → 数据未变；跨语句事务状态一致 |
| B5 | 结果区完整化：结果集标签条（上限 5 + 淘汰 + **通道徽标 + 血缘摘要**）、工具栏（行数/耗时/连接/**筛选 + 下发开关**/**分析**/导出/刷新/复制）、截断提示、`affected_rows` 展示 | `view/widgets/result_grid.rs` + `store.rs` | 窗口/真机：滚动流畅；DML 显示影响行数；标签显示通道与血缘 |
| B5b | **分段抓取**（已确认）：数据源按“已抓取窗口 + 取下一段”实现（`known_row_count() -> Option<u64>` / `window()` / `fetch_next(limit)` / `rows(offset,limit)`），固定 **1000 行/段**起步，未知总数显示 `N+`；导出区分“仅已抓取”与“抓全量” | `store.rs`（窗口状态）+ `view/widgets/grid/`（“取下一段”入口与 `N+` 展示）+ `execution.rs`（后台抓取） | 集成：>10,000 行表 → 首段 1000 行 + 状态行 `1000+`；取下一段追加不重复；导出两种语义行数一致（测试场景 37–39） |
| B6 | 错误回填：错误位置解析（多家驱动格式）→ 诊断 + 定位 + 聚焦；无位置则只提示 | `execution.rs` + `view/` | 单测：≥8 种真实错误文本；真机：故意写错列名可定位 |
| B7 | **导出**：CSV / JSON / INSERT / Parquet / XLSX（后者经 DuckDB 临时表） | `execution.rs` / `persist.rs` | 集成：5 种格式落盘可回读（CSV/JSON 断言内容） |
| B8 | **历史面板**：右 Dock `History` 实装（列表 / 搜索 / 重放 / 删除 / 清空），字段真实 | `view/history.rs` + `engine::history_store` | 集成：执行 3 次 → 历史 3 条且耗时>0；失败也留痕 |
| B9 | **补全**：上下文判定 + 排序 + 元数据缓存（TTL 30s）+ 降级（关键字/函数 + 说明） | `completion.rs` + `database::MetadataService` | 单测：上下文判定与排序；真机：`FROM` 后出表、`t.` 后出列 |
| B10 | 格式化 / 转译 / 执行计划接线（用 P0.3 的格式化实现；EXPLAIN 按方言生成） | `commands.rs` + `engine::sql::SqlEngine` | 真机：格式化结果可往返解析；EXPLAIN 在 4 类库均返回 |
| B11 | 对外接口：`open_sql(conn_id, sql)` / `execute_all(...)`（供 M4「在 SQL 编辑器中打开 / 查看数据」自动执行） | `service.rs` + `crates/workbench/src/panels.rs` 调用点 | 集成：M4 右键 → 编辑器打开并（可选）自动执行 |
| B12 | 移除遗留：删除 `EditorPanel` 的 SQL 区块与内联执行闭包；连接详情卡/导航树按架构 §3.3 迁出 | `crates/workbench/src/panels.rs` | `cargo check --workspace` 零告警；无 render 期 I/O（人工复核 + 契约测试） |
| B13 | **执行通道**：工具栏「执行位置」指示器（源库 / 本地加速 / 联邦）+ `channel_status(conn_id) -> Result<(), Reason>` 可用性判定 + 不可用原因 + 通道随文档持久化 + 切通道失效提示 | `crates/editor/src/channel.rs`（新）+ `view/sql_mode.rs` + `connection::secret` / `engine::duckdb::federation` | 集成：三通道各跑一次；未开加速的连接下拉置灰并给原因；写语句在加速通道被拒；切通道后旧结果标灰 |
| B14 | **筛选下发源库**：筛选框 + `▢ 下发源库` 开关 → `re_execute_with_filter` → **新结果集**（lineage = 原结果 + 条件 + 通道） | `execution.rs` + `engine::services::execution_service` | 集成：本地筛选不重查；下发后新结果集且原结果保留；含 `LIMIT` 的原查询给出提示（架构 §12 #15） |
| B15 | **DuckDB 分析入口**：结果集「分析 ▾」→ `execute_duckdb_analysis` → 新结果集（lineage = 临时表 + 分析 SQL）；支持“桥接当前可见行” | `execution.rs` + `engine::services::execution_service` | 集成：对结果集做 COUNT / GROUP BY 各一次，产出新结果集；桥接过滤可用 |

**1b 验收口径**：达到"合格 SQL 编辑器"——执行族齐备、可中断、有事务与历史、结果区可过滤可导出、错误可定位、补全可用。

---

### Phase 1c — 分析模式骨架（Cell / Output / Session）

**目标**：`.rdsnote` 笔记可选连接、可编辑单元、可执行 SQL 单元并看到内联输出；Python/Rust 只留扩展位。

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| C1 | 模型：`NoteBook` / `Cell` / `Output` / `run_seq` / stale 判定（纯函数） | `crates/editor/src/notebook.rs` | 单测：插入/删除/移动后 id 稳定；stale 规则（改源/会话重启/依赖变化） |
| C2 | `Session` trait + `SqlSession`：连接 + DuckDB 分析命名空间 + `CREATE TEMP TABLE` 作为变量 | `session.rs` | 集成:临时表创建后，后续单元可 `SELECT * FROM t` |
| C3 | 笔记视图：笔记头（会话选择器 / 全部运行 / 重启会话 / 清空输出）+ 单元卡片（类型/执行/⋮/折叠/stale）+ 插入点 | `view/notebook_view.rs` | 窗口测试：3 单元渲染、执行态切换、插入与排序 |
| C4 | 内联输出：`Table`（预览 + 展开）/ `Text` / `Error`（含定位） | `view/widgets/output_block.rs` | 窗口/真机：三种输出可见；"展开"切到结果面板（同一结果集） |
| C5 | 键盘：`Shift+Enter` / `Ctrl+Enter` / `Alt+Enter` / `Ctrl+Shift+Enter` / `Esc` 单元焦点模型 | `commands.rs` + 视图 | 真机：全键位可用（与 `editor-notebook` context 匹配） |
| C6 | `.rdsnote` 读写：单元 + 元数据 +（可选）输出引用；与项目版本链对接留接口 | `persist.rs` | 集成：写→读往返一致；输出引用可失效并提示 |
| C7 | 模式转换：SQL → 分析（整篇单单元，含确认）/ 分析 → SQL（拼接脚本） | `mode.rs` | 窗口测试：两条转换路径 + 确认分支 |
| C8 | Markdown 单元：渲染态 / 编辑态切换 | `view/notebook_view.rs` | 真机：双击进编辑、失焦渲染 |
| C9 | **分析沉淀**：结果区的 DuckDB 分析可一键沉淀为分析单元（单元源 = 分析 SQL，输出 = 结果集，临时表 = 会话变量） | `notebook.rs` + `view/notebook_view.rs` + `execution.rs` | 窗口/真机：从结果区发起分析 → 生成单元 → 下游单元可引用该临时表 |

**1c 验收口径**：能创建 `.rdsnote`、选连接、写多个 SQL/Markdown 单元、`Shift+Enter` 依次执行并有内联输出、重启会话后全部标 stale、保存后重开内容与结构一致。

---

### Phase 2 — 后续（本次不排期）

| 主题 | 内容 | 前置 |
| --- | --- | --- |
| Python 内核 | sidecar（复用 `plugin/src/sidecar`）+ `ipykernel` + Jupyter 消息协议；单元 `PY` | 1c 的 `Session` 接口固化 |
| Rust 内核 | WASM 沙箱（wasmtime/extism，无外部工具链）或单元级 `cargo script`；需进度/取消/缓存 | 同上 |
| 变量桥 | 会话间以 **Arrow** 交换（SQL 临时表 ↔ DataFrame ↔ Rust） | Python/Rust 内核就位 |
| 富输出 | `Chart`（`component::chart`）/ `Insight`（M8）/ 输出导出 | 1c 输出注册表 |
| 互操作 | 导入/导出 `.ipynb`（nbformat 4.5） | 1c 模型稳定 |
| 其他 | 参数绑定（prepared statement）、拖拽插入限定名、`minicatalogs`、变量浏览器、使用手册 | — |

---

## 3. 测试场景清单

**Phase 0（后端）**
1. 切分：单语句 / 尾分号 / 多语句 / 空语句 / 字符串内分号 / 行注释 / 块注释 / `$$` 块 / 反引号标识符 / 未闭合引号（不 panic，报错）
2. 历史：成功记录耗时>0、失败也写并含原因、行数/影响行数正确、`db_type` 非 `unknown`
3. 执行结果结构：SELECT 无 `affected_rows`、DML 有

**Phase 1a（编辑体验）**
4. 模式判定：`.sql`→SQL、`.rdsnote`→分析、`.md`/`.txt`/未知→文本、入口语义覆盖、显式记忆优先
5. 切换：文本↔SQL 内容不变；SQL→文本 显示"结果标灰"提示；切换确认可取消
6. 脏状态：输入即脏、撤销回原值干净、保存干净、标签脏点与状态栏同步
7. 关闭：三态确认（保存/丢弃/取消）+ 保存失败二次确认
8. 外部修改：磁盘被改后保存 → 三态（重载/覆盖/取消）
9. 只读：编辑器只读不可输入；连接只读可输可执行但写语句被拦
10. 持久化：关闭重开恢复光标/选区/模式/连接绑定
11. 大文件：>50MB 关补全；>200MB 只读卡
12. 快捷键：`Ctrl+S` `Ctrl+Enter` `Ctrl+/` `Ctrl+F` `Ctrl+Shift+F` 全部生效

**Phase 1b（执行闭环）**
13. 目标解析：无选区执行当前语句（光标在中间语句）/ 有选区执行选区 / 全部 / 批量
14. 批量：3 条语句、第 2 条失败 → 3 个结果集且失败标红，其余成功
15. 取消 / 超时：长查询中断；超时给可读错误
16. 事务：begin→insert→rollback 数据不变；commit 生效；状态栏状态与时长正确
17. 结果区：10 万行滚动流畅；快速过滤生效；上限 5 淘汰最旧；截断提示
18. 导出：CSV/JSON/INSERT/Parquet/XLSX 落盘可回读
19. 历史：执行 3 次（含 1 次失败）→ 面板 3 条且字段真实
20. 补全：`FROM` 后表、`t.` 后列、无连接降级为关键字+函数
21. 错误定位：故意写错列名 → 定位到行；无位置信息时不误报第 1 行

**Phase 1c（分析模式）**
22. 单元生命周期：插入/删除/移动后 id 稳定、输出不错位
23. 会话变量：单元 1 建临时表 → 单元 2 可查；重启会话后全 stale
24. stale：改单元 1 源 → 自身与下游标 stale；重跑后清除
25. 持久化：`.rdsnote` 写读往返一致；输出引用失效时给出提示
26. 转换：SQL→分析（整篇单单元）内容不丢；分析→SQL 拼接脚本

**跨阶段**
27. 主题：明暗切换下编辑区/结果区/单元卡/输出区对比度（`theme-preview.html` 与交互稿为基准）
28. 契约：`ui_contract` 扫描 `crates/editor/src` 零裸色值、零裸 `px(`
29. 无项目：工作台覆盖选择器时编辑器不额外报错

**通道与结果血缘（2026-09-15 修订新增）**
30. 通道门控：未开本地加速的连接 → 执行位置中加速项置灰且行尾有原因；联邦无源时给“注册新源…”入口
31. 通道切换：源库 → 加速后旧结果集标灰 + 顶部提示；分析模式下全部单元标 `stale`
32. 通道写限制：加速通道下 `INSERT` 被拒并提示切回源库（不得落本地副本）
33. 筛选本地 vs 下发：本地筛选不重查（无新结果集）；下发产生新结果集且原结果保留
34. 重查包裹语义：原查询含 `ORDER BY` 时结果仍有序；含 `LIMIT` 时给出“将去掉 LIMIT”提示
35. DuckDB 分析：对结果集 COUNT / GROUP BY → 新结果集（血缘 = 临时表 + 分析 SQL），原结果保留
36. 分析沉淀（1c）：结果区分析 → 生成单元 → 下游单元引用该临时表；重启会话后 `stale`

**分段抓取（2026-09-15 新增）**
37. 首段与未知总数：>10,000 行的表 → 初始抓 1000 行、状态行显示 `1000+`；“取下一段”追加且不重复、不跳行
38. 导出语义：区分“仅导出已抓取”与“抓全量后导出”——提示文案与实际落盘行数一致
39. 抓取中取消：取下一段过程中取消 → 已抓取部分仍可用于筛选/排序/导出（不丢已取数据）

---

## 4. 风险与对策

| 风险 | 影响 | 对策 |
| --- | --- | --- |
| Dock 标签无法满足关闭拦截（P0.1） | 1a 需自绘标签条（+约 300 行，且与 Dock 视觉重复） | Phase 0 先验证；不成立则采用"自绘标签条 + Dock 仅作容器"并记录到架构 §12 |
| 事务会话亲和不可得（P0.2） | 事务功能只能"看起来能用" | 验证先行；不可得则引入 per-session 独占连接并评估对池化的影响 |
| 格式化选型失败（P0.3） | 1b 的"格式化"缺口 | 退路：只做轻量缩进与关键字大写（不做完整重排），并在 UI 明示能力边界 |
| SQL grammar 与主题角色不匹配（P0.8） | 高亮角色缺失或颜色异常 | 缺角色复用最接近标准 token（零裸色）；必要时补产品语义 token |
| editor 独立 crate 引入循环依赖 | 编译结构被破坏 | 依赖方向固定为 `workbench → editor → engine/database/shared`；editor 不得依赖 workbench（架构 §3.1） |
| 与 M4/M5 的接口漂移（`editor_set` / 草稿箱打开文件） | 收编时改动面扩大 | 契约先行（架构 §3.4），1b 的 B11/B12 一次性切换并在文档中更新映射 |
| 1c 的 `Session` 抽象过早固化 | 第二期 Python 内核接入时返工 | 抽象只保留"执行/中断/状态/命名空间"四件事；Python 相关字段（ZMQ/内核握手）**不进第一版接口** |
| 输出引用（DuckDB 临时表）生命周期 | `.rdsnote` 重开后输出失效 | 输出块显式区分"可展开的真实数据"与"已失效（重跑即可）"；默认不持久化全量输出 |
| 现有 `EditorPanel` 迁出牽连 M3/M4 宿主职责 | 迁移期功能回归 | 按架构 §3.3 逐项迁出，先迁编辑器部分；每步跑 `-p rds-workbench` 全测 |
| **通道可用性判定无来源**（架构 §12 #13） | UI 给不出准确的“加速不可用原因” | 1b 前定义 `channel_status`；暂无来源时按“未注册 Secret”降级提示，并在文案里说明是推断而非事实 |
| **重查包裹语义**（含 `ORDER BY` / `LIMIT`） | 重查结果与原查询不一致，用户困惑 | 定规则 + 显式提示（测试场景 34）；必要时提供“重查后保留 LIMIT”开关 |
| **ATTACH 快照新鲜度** | 用户拿旧快照做决策 | 结果集徽标 + 状态栏“快照” + “重新 ATTACH”入口（原型 §5.7） |
| 通道/血缘增加结果集数量 | 上限 5 与淘汰策略被更快触发 | 淘汰时提示来源；后续评估“按通道分组淘汰” |
| 编译时间与体积增长（新增 grammar 等依赖） | 开发体验 | P0.9 记录基线；新依赖一律进 workspace 唯一入口表并评估必要性 |

---

## 5. 实现位置映射（设计决策 → 代码）

| 设计决策 | 代码文件 |
| --- | --- |
| 文档/模式/只读/能力表 | `crates/editor/src/{model.rs, mode.rs}` |
| 唯一执行入口 + 后台任务 | `crates/editor/src/execution.rs`（模式参考 `crates/workbench/src/services/nav_jobs.rs`） |
| 语句切分（词法级） | `crates/editor/src/split.rs`（替换 `engine/src/services/sql_parser_service.rs::split_sql`） |
| 结果单权威 + 上限淘汰 | `crates/editor/src/store.rs` |
| 补全 | `crates/editor/src/completion.rs` + `crates/database/src/metadata_service.rs` |
| 会话与单元 | `crates/editor/src/{session.rs, notebook.rs}` |
| 持久化（工作区上下文 / `.rdsnote`） | `crates/editor/src/persist.rs` + `engine/src/persistence/workbench_context_store.rs` |
| 三模式视图 | `crates/editor/src/view/{host,text_mode,sql_mode,notebook_view}.rs` |
| Action 与快捷键 | `crates/editor/src/commands.rs` + `crates/app/src/main.rs` |
| 宿装配（中央区） | `crates/workbench/src/view.rs::init_workspace` |
| 历史字段 / 事务 / affected_rows | `engine/src/{persistence/history_store.rs, services/sql_service.rs}` |
| 格式化实现 | `engine/src/sql/formatter.rs` |
| SQL 高亮注册与主题角色 | `crates/editor/src/view/` + `assets/themes/{rds-theme.json, product-tokens.json}` |
| 尺寸常量 | `crates/editor/src/ui.rs` |
| 执行通道与门控 | `crates/editor/src/channel.rs` + `connection/src/secret.rs` + `engine/src/duckdb/federation.rs` |
| 筛选下发 / DuckDB 分析 | `crates/editor/src/execution.rs` + `engine/src/services/execution_service.rs` |
| 结果血缘与通道徽标 | `crates/editor/src/store.rs`（结果集元数据） |
| 网格渲染层（将来可提炼到 shared） | `crates/editor/src/view/widgets/grid/`（`GridDataSource` / `GridEditSink`，架构 §3.6） |
| 契约测试范围 | `crates/workbench/tests/ui_contract.rs` |
| 交互稿 | `docs/architecture/editor/editor-prototype.html` |

---

## 6. 验证方式

```sh
# 模块回归（editor 服务层 + 引擎 SQL 层）
cargo test -p rds-editor -p rds-engine --lib -j 2

# 契约（零裸色 / 零裸 px，扫描 editor + workbench）
cargo test -p rds-workbench --test ui_contract -j 2

# 全工作区编译守卫
cargo check --workspace --all-targets -j 2

# P0.2 真机探针（事务会话亲和；凭据只从环境变量读）
# PowerShell：
#   $env:RDS_TEST_MYSQL_URL="mysql://root:root@192.168.3.138:3306/mysql"
#   $env:RDS_TEST_PG_URL="postgres://postgres:postgresql@192.168.3.138:5432/postgres"
#   $env:RDS_TEST_SQLITE_PATH="D:\FossilT\T.fossil"
#   $env:RDS_TEST_DUCKDB_PATH="D:\data\123"
cargo test -p rds-engine --test transaction_affinity -j 2 -- --nocapture --test-threads=1
```

- 每阶段结束：上列命令全绿 + §3 对应场景真机走通（`cargo run -p rds-app -j 2`）。
- 真机回归矩阵（1b 起每轮至少一遍）：MySQL / PostgreSQL / SQLite / DuckDB × 执行族（当前语句 / 选区 / 全部 / 批量）× 只读 / 可写 × 明暗主题。
- P0.9 基线记录：新增依赖后的 `cargo build -p rds-app -j 2` 耗时与 `target/debug/rds-app` 体积（写入本文件，用于后续判断依赖收益）。
- 阶段完成后回填 §0 进度表，并同步 `editor-architecture.md` §12 与 `editor-prototype-design.md`（若交互有调整）。

---

## 7. 待确认（开工前的阻塞清单）

见 `editor-architecture.md` §13（10 项）。其中**必须先回答**的 5 项：

| # | 问题 | 建议 |
| --- | --- | --- |
| 1 | 多文档标签：Dock 自带 / 自绘 | Dock 自带（待 P0.1 验证） |
| 2 | SQL → 分析 转换粒度 | 整篇单单元 + 二次"按语句拆分" |
| 3 | 批量执行语义 | 逐条独立、失败不中断 |
| 4 | 分析模式首期语言 | SQL + Markdown（Python/Rust 第二期） |
| 5 | 是否独立 `crates/editor` | 是（§3.2 判定四条成立） |
| 6 | 加速 / 联邦通道遇写语句 | 直接拒绝 / 允许但仅落本地副本 | 直接拒绝 + 提示切回源库 |
| 7 | 执行通道默认值 | 总是源库 / 记忆上次 | 记忆上次（随文档绑定） |
| 8 | DuckDB 分析结果落点 | 新结果集 / 覆盖 / 直接作为分析单元 | 1b 新结果集（血缘）；1c 可沉淀为分析单元 |
| 9 | 标签关闭语义 | ① 草稿兜底（关闭即落草稿 + 最近关闭恢复）/ ② 自定义标签条后弹三态确认 | **①**（关闭入口单一、无绕过）；② 作为体验升级后置（API 已确认可行） |
