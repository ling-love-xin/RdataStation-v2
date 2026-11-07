# 列级血缘（来源链）界面设计建议

日期：2026-11-08　状态：**设计定稿，未实现**（`lineage_of_column` 出口已在，等界面）
关联：`sqlglot-rust-0.11.1-upgrade-report.md` §8、`sqlglot-unwired-backlog.md` §3.1、
`editor-prototype-design.md` §7.4（列级血缘行）

## 1. 结论

**先做「列上的一条来源链」，不要一上来做面板。**

数据和 UI 之间已经有一条现成通道（结果集的列注释），补一行调用就能让「表达式列从哪来」可用；
而新增一个右 Dock 面板要动的契约远多于它当期带来的价值。面板留给**多列 / 跨结果集**的真实诉求
（对账、影响分析）出现时再做。

## 2. 为什么先不上面板（代码依据）

右栏面板不是加一个 enum 变体：

| 触点 | 位置 |
| --- | --- |
| 面板分发 | `crates/workbench/src/panels/right.rs:246-249`（`RightPanel::{Insight, Mock, History, Archive}`） |
| 激活态 / 打开列表 / 宽度持久化 | `crates/workbench/src/panels/shared.rs:262-281`、`:411-421` |
| 图标存在性 | `crates/workbench_shell/src/model.rs` 的 `activity_icon_tests`（**图标缺失会渲染空白，曾被误判成没接线**） |

一个新面板 ≈ 5 个触点 + 空态 + 拖拽宽度，约 2–3 天；而它当期要回答的问题只是「这一列从哪来」。

## 3. 现成落点（三件套已经齐了）

| 现有件 | 位置 | 已经有的东西 |
| --- | --- | --- |
| 列注释模型 | `crates/workbench_shell/src/model.rs:322` `ColumnAnnotation` | `source_table`、`source_column`、`comment`、`data_type`、`nullable`、`default_value` |
| 列注释产出 | `crates/editor/src/ports/editor_completion.rs:426` `column_annotations(..)` | 方言从连接驱动推、`SqlEngine::projection_sources` 解析、`meta_map`（NavCache 列名 + 驱动类型名） |
| 列注释消费 | `crates/result_panel/src/grid/header.rs:43-47` `header_tooltip_lines` | 表头 tooltip 已在画「来源：orders.amount」 |

关键点：**SQL、方言、表目录（含类型）三样输入，在 `column_annotations` 里已经同时存在**，
`TypedTable` 可以直接从 `meta_map` 拼出来——不需要新的跨 crate 通路，也不需要新的端口注入。
现状的缺口只有一处：表达式 / 聚合列两处 `None`，tooltip 连「来源」行都不出现。

## 4. 三段式

### 第一段（0.5–1 天，优先做）：把「表达式列无来源」补上

**做法**

1. `ColumnAnnotation` 新增 `upstream: Vec<(name, source, depth)>`（`LineageStep` 的直接投影）；
2. `column_annotations` 里对 `ProjectionSource::Column` 取不到列元数据的项（表达式 / 聚合 / 别名），
   用**输出列名**跑一次 `engine::sql::lineage_of_column(sql, dialect, column, &typed_tables)`；
3. 表头 tooltip 在多跳时多画一行：`来源链：SUM(amount) → orders.amount`。

**入口**：沿用「鼠标停在列头上」，不新增任何入口。

**验收**

- `SELECT SUM(amount) AS total FROM orders` → 该列 tooltip 出现 ≥2 跳来源链；
- 单表裸列（`SELECT id FROM orders`）行为不变，不重复画；
- 未绑定连接 / 目录未载好 → 整行不显示（不猜，与 hover 无目录同口径）；
- `cargo test -p rds-editor --lib` 不新增失败。

**已知性能口径**：lineage 是纯解析，单列毫秒级；但结果集可能上百列。**不在渲染路径同步算**，
放在注释生成阶段（本来就在执行后一次性算）或 tooltip 打开时按列算；需要一条预算测试兜住。

### 第二段（0.5 天）：右键列头「查看血缘」→ 小弹层

tooltip 撑不住多跳（CTE + 子查询 + 表达式）。右键列头弹 `Popover` / `Dialog`，表格两列：

```
深度缩进 | 节点             | 来源
0        | total            | （输出列）
1        | SUM(amount)      | （表达式）
2        | amount           | orders
```

并给**「复制为文本」**——对账 / 写工单的人要的是能贴走的东西，不是一张图。

**不要先做 mermaid 图**：右栏约 320px，图在小宽度下不可读；mermaid 只适合导出。

### 第三段（2–3 天，等真实诉求）：右 Dock 面板

只在出现「**多列 / 跨结果集**」需求时才做：

- 输入不是「当前文档」，而是「当前结果集 + 选中的列集合」；
- 多列并排看共同上游（对账：两份报表是否同源）；
- 反向影响分析（改基表某列 → 影响哪些输出列）需要对每个输出列各跑一次再求并集，成本线性、可后台跑。

## 5. 要定死的口径

1. **只认单条语句**：`lineage_of_column` 吃一条 SQL。结果集存的是原文，多语句脚本要先
   `split_statements` 定位「产生这份结果的那条」，别把整段脚本丢进去（解析失败 → 一条链都画不出）。
2. **中间节点与实体表必须可区分**：CTE 名、派生表别名不是真实表，加前缀（`CTE:` / `子查询`），
   否则用户会去导航树找一张不存在的表。
3. **拆不开就直说**：`COUNT(DISTINCT …)`、窗口函数、部分方言特有时上游可能给不出。
   此时显示「由表达式计算，无法拆到基列」，不画断链——与「宁缺毋滥」一致。
4. **不做跨结果 diff**：血缘只回答「这列从哪来」，不回答「两份结果是否同源」；
   后者是 `diff_summaries` + 版本链的职责，不要混进同一个面板。
5. **面板名**：主标「血缘（来源链）」，副标说明；单独叫「血缘」对非数据从业者不自解释。

## 6. 未验证 / 待定（实现前先补探针）

- **UNION / INTERSECT 的输出列**：上游 `lineage_sql` 对多分支的选择行为**未实测**，可能只取一支或报错；
  做第一段前先用探针确认（`crates/sql/src/analysis/tests.rs` 加一条）。
- **`SELECT *` 展开**下的列血缘：靠目录展开（`meta_map` 有），但派生表的 `*` 无法展开，需如实降级。
- **性能预算数字**：没有真实 100 列结果集的耗时基线，第一段落地时补一次测量再定阈值。
- 多语句脚本「哪条语句产生这份结果」目前靠 `projection_sources` 的解析结果反推，**没有语句级定位 API**，
  若后续出现需要，再考虑在 `engine::sql` 加 `statement_at(sql, byte)` 出口。

## 7. 与相邻能力的边界

| 能力 | 回答的问题 | 不做什么 |
| --- | --- | --- |
| 本设计（列级血缘） | 这一列**从哪来**（逐跳） | 不做两份结果是否同源、不做版本链 |
| `diff_summaries` | 两段 SQL **结构改了什么** | 不解释数据来源 |
| `plan_summary` | sqlglot 自建的算子图 | **不是源库 EXPLAIN**，不给普通用户 |
| hover「来源」行 | 当前语句引用了哪些表 | 不追到基列 |
