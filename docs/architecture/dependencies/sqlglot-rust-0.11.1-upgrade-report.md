# sqlglot-rust 0.10.33 → 0.11.1 升级报告

> 日期：2026-11-08 · 分支：`upgrade/sqlglot-rust-0.11.1`（叠在 `upgrade/duckdb-1.5.6` 之上，可独立 rebase）
> 关联：`dependency-strategy.md` §4 / §5 / R4 例外表 · 接入点唯一：`crates/sql`（`rds-sql`）

## 1. 结论

**升级完成，代价极小但上游改动面不小。** `sqlglot-rust` `=0.10.33` → **`=0.11.1`**（crates.io 2026-10-28），
项目侧只需在 `crates/sql/src/builder.rs` 的**两处** `CreateTableStatement` 字面量补上 0.11 新增的两个字段，
**生成 SQL 与格式化结果没有任何变化**——`cargo test -p rds-sql` 的 195 + 10 项与 0.10.33 基线**逐项一致**，
包括把生成 SQL 字面结果钉死的 `builder::tests::test_generated_sql_is_pinned`。

真正需要留意的是上游把**运算符优先级**做成了独立模块（新增 `src/precedence.rs`，解析器 `+405/−146`），
这类改动不一定体现在我们现有的用例集里，见 §6。

## 2. 版本与依赖事实

| 项 | 值 |
| --- | --- |
| 版本 | `0.10.33` → `0.11.1`（发布 2026-10-28） |
| 运行时依赖 | `log ^0.4`、`serde ^1`（derive）、`serde_json ^1`、`thiserror ^2`；`clap ^4`（optional，`cli` feature，本仓未启用） |
| 上游提交 | 见 crate 内 `.cargo_vcs_info.json` |
| 锁变化 | `Cargo.lock` 只有 `sqlglot-rust` 一行变动，无其他包进出 |

## 3. 上游变更面（0.10.33 → 0.11.1，`git diff --no-index` 实测）

整包 **40 文件 +2382/−411**（含测试）；**仅 `src/` 为 19 文件 +848/−379**：

| 文件 | 增/删 | 关注点 |
| --- | --- | --- |
| `src/parser/sql_parser.rs` | +405/−146 | 解析器主改动 |
| `src/precedence.rs`（**新增**） | +41 | 运算符优先级独立成表 |
| `src/generator/sql_generator.rs` | +98/−5 | 生成器：CTAS 的 `WITH` 属性 / `WITH [NO] DATA` |
| `src/dialects/mod.rs` | +71/−66 | 方言层重排 |
| `src/optimizer/lineage.rs` | +46/−54 | 血缘分析 |
| `src/optimizer/mod.rs` · `pushdown_predicates.rs` · `annotate_types.rs` | 40/40 · 15/17 · 22/22 | 优化器内部重排 |
| `src/ast/types.rs` | +33/−2 | `CreateTableStatement` 增 `with_properties: Vec<TableProperty>` 与 `with_data: Option<bool>`；新增 `TableProperty` |
| `src/tokens/tokenizer.rs` | +8/−2 | 词法层微调 |
| `src/ffi.rs` | +22/−1 | 导出面新增 |
| 新测试 | `tests/test_ctas_properties.rs` +826 · `tests/test_operator_precedence.rs` +279 | 上游为这两块补了重测试 |

## 4. 项目侧改动（2 处，行为不变）

`crates/sql/src/builder.rs`：

| 位置 | 函数 | 改动 |
| --- | --- | --- |
| `create_table_sql`（约 L141） | `build_create_table` 的内核 | 结构体字面量补 `with_properties: vec![]` / `with_data: None` |
| `build_create_table_as_select`（约 L240） | 生成 `CREATE TABLE ... AS SELECT` | 同上 |

取空值的依据（读上游 0.11.1 `src/ast/types.rs:1770-1785` 的字段注释）：
`with_properties` 只在 CTAS 的 `WITH (...)` 子句里有值，`with_data` 只在 `WITH [NO] DATA` 里出现；
两者在 `as_select` 为 `None` 时被生成器**直接忽略**。我们的两处调用一处是纯建表（`as_select: None`），
一处是 CTAS 但不带 `WITH` 属性、也不写 `WITH DATA`，因此填空值即可，不必也不应该猜到别的取值。

`crates/sql` 其余 12 个模块（`parser` / `formatter` / `highlight` / `refs` / `semantic` / `hover` / `jump` /
`filter` / `transpiler` / `bind` / `split` / `script`）**零改动**：用到的 `parse` / `parse_statements_with_comments` /
`generate` / `generate_pretty` / `transpile` / `transpile_with_comments` / `Dialect` / `tokens::*` /
`optimizer::qualify_columns` / `schema::MappingSchema` / `ast::*` 签名在 0.11.1 上均未变。

## 5. 验证

| 项 | 命令 | 结果 |
| --- | --- | --- |
| SQL 回归基线（0.10.33） | `cargo test -p rds-sql -j 2` | 195 passed / 10 passed / 0 failed |
| SQL 回归（0.11.1） | 同上 | **195 passed / 10 passed / 0 failed**，与基线逐项一致（含 `test_generated_sql_is_pinned`） |
| 应用编译 | `cargo check -p rds-app -j 2` | exit 0，**0 error 0 warning**，57.6s |
| 编辑器回归 | `cargo test -p rds-editor -j 2` | 746 passed / **2 failed** / 1 ignored |
| 能力出口（2026-11-08 新接线） | `cargo test -p rds-sql -j 2` | **206 passed**（195 基线 + `analysis` 11 条）/ 探针 10 passed / 0 failed |
| 设置域 | `cargo test -p rds-settings --lib -j 2` | 34 passed / 0 failed（三条新 key 通过注册契约校验） |
| 三处消费 + 装配 | `cargo check -p rds-settings -p rds-editor -p rds-app --all-targets` | 全部 exit 0，0 error |

**那 2 条失败与本升级无关**（已在 0.10.33 上复现，升级前后失败断言原文相同）：

- `view::tests::legacy::opening_an_analysis_document_builds_a_cell_editor_per_cell` —— 断言「单元源在渲染树里是内核编辑器（不是纯文本回退）」
- `view::tests::notebook_win::analysis_cell_renders_multiple_output_blocks`

复现方式：`git stash` 掉本升级改动 → 分别跑这两条 → 仍然 `FAILED. 0 passed; 1 failed`。属仓库既有失败（视图渲染树断言），
建议单独开一条修复（不在本次依赖升级的范围里，也不该被这次升级的 commit 顺手改掉）。

## 6. 未验证与风险

1. **运算符优先级重写**：0.11.1 把优先级抽成独立表并改造解析器（+405/−146），上游为此新增 279 行优先级测试。
   我们的 195 项覆盖的是自身用到的语句形状，**没有与上游做全量一致性对拍**。若线上出现「同一 SQL 在 0.10 与 0.11 解析树不同」，
   优先怀疑这一块；建议后续用真实项目 SQL 语料（`docs/` 与测试里的样例）做一次 parse-tree 对拍。
2. **`WITH` 属性语法**：0.11 才支持 CTAS 的 `WITH (...)` 属性解析/生成。我们目前不生成它，
   但如果将来要把建表语句里的 `WITH` 属性做进 UI，这次新增的 `with_properties` / `with_data` 就是入口。
3. **`cli` feature 未启用**：`clap ^4` 仍为 optional，本仓不用 CLI 子命令。
4. **转译输出对拍**：`transpile` 到 MySQL / PostgreSQL 的**具体输出文本**没有样本级对拍（只有行为用例）。
5. 既有的 2 条视图用例仍是红的（见 §5），不要把它们记成这次升级的回归。

## 7. 复现与回滚

```powershell
$wt = "D:\RdataStation\RDS\RdataStation-v2-upgrade"
# 升级
#  根 Cargo.toml：sqlglot-rust = "=0.11.1"
cargo update -p sqlglot-rust --precise 0.11.1
#  builder.rs 两处 CreateTableStatement 补 with_properties / with_data
cargo test -p rds-sql -j 2      # 应得 195 + 10
cargo check -p rds-app -j 2     # 应得 0 error 0 warning
```

回滚：`git checkout -- Cargo.toml Cargo.lock crates/sql/src/builder.rs`（或直接 revert 本分支的两个 commit）。

## 8. 能力释放度（0.11.1 上重新确认，2026-11-08 已接线 6 项）

**结论：出口层 8 项「完整释放」，编辑器侧已接 6 项 + 3 个设置开关；剩下的两项（血缘、diff）与计划面板不是能力缺口，而是**没有 UI 承载**——按需接，不做「为接线而接线」。**

`crates/sql/src/analysis.rs` 把 0.11.1 里 8 项此前完全没出口的能力统一封成函数，一律 `Result<_, String>`
（错误只带一行中文原因，不外泄上游类型），并 `pub use` 到 `engine::sql`——业务侧写法与原有 `diagnose` / `hover_at` 一致：

| 能力（上游） | 出口函数 | 前置 | 编辑器侧接线 | 状态 |
| --- | --- | --- | --- | --- |
| 作用域分析 `build_scope` | `scope_report` | 无（不查元数据） | 悬停补「来源表」一行 + 悬停表/列列表（目录快照） | ✅ 已接线（`sql_analysis.hover_scope`） |
| 类型标注 `annotate_types` | `column_types` / `data_type_label` | `TypedTable`（表名 + 列名/类型名） | 悬停列补「类型：DECIMAL(12,2)」一行 | ✅ 已接线（2026-11-08，目录快照喂 `TypedTable`） |
| 列级血缘 `lineage_sql` | `lineage_of_column` | `TypedTable` | 暂无 UI 承载（血缘面板未立项） | 🟡 压后（触发条件见 `sqlglot-unwired-backlog.md`） |
| AST 差异 `diff_sql` | `diff_summaries` | 无 | 暂无 UI 承载（版本/结果对比未立项） | 🟡 压后（同上） |
| 谓词下推 `pushdown_predicates` | `pushdown_sql` | 无 | 代码动作「把 WHERE 下推进子查询」 | ✅ 已接线（`sql_analysis.predicate_pushdown`） |
| 本地计划 `plan` | `plan_summary`（含 mermaid） | 无（DDL 直报不支持） | 明确不给普通用户（是 sqlglot 自建计划，不是源库 EXPLAIN） | 🟡 压后（只该进调试面板） |
| 时间格式串转换 `format_time_dialect` | `translate_time_format` | 无 | 转译提示「时间格式 %Y-%m-%d → YYYY-MM-DD」 | ✅ 已接线（2026-11-08，`translate::notes`） |
| 目标方言兼容校验 `validate_dialect_support` | `unsupported_for_target` | 无 | 转译提示「目标方言不支持：…」 | ✅ 已接线（2026-11-08，`translate::notes`） |
| 语义诊断（`diagnose` + 目录） | 原有 `diagnose` | 表目录 | 打字期未知表/未知列 | ✅ 已接线（`sql_analysis.semantic_diagnostics` 闸） |

### 8.1 设置暴露（`crates/settings` 的 `sql_analysis` 域）

设置项是**能力开关**（不是调参），改动即时生效、不重启：

| key | 界面文案 | 默认 | 生效点 |
| --- | --- | --- | --- |
| `sql_analysis.semantic_diagnostics` | 语义诊断 | 开 | `face_diag.rs`：关掉只跑词法层诊断（大文件打字卡顿的退路） |
| `sql_analysis.predicate_pushdown` | 谓词下推改写 | 开 | `face_actions.rs`：关掉则不摆「把 WHERE 下推进子查询」这条动作 |
| `sql_analysis.hover_scope` | 悬停显示来源表 | 开 | `sql_hover.rs`：关掉则悬停不再补「来源：a、b」那行 |

跨层传递沿用仓库既有口径（`settings` 不依赖 `rds-sql`）：偏好存三个 `AtomicBool`，
设置侧改开关 → 落盘 → `install_sql_analysis_sink` 回调 → `rds_sql::analysis::set_prefs`；
启动时 `SettingsService::init` 之后立刻用落盘值推一次，保证首帧就按设置生效（读侧无锁，打字路径零成本）。

### 8.2 表列快照从哪来（**各源库，不只 DuckDB 分析模式**）

`column_types` 需要的 `TypedTable` 不是新造的一条元数据通路，而是复用仓库**已有的目录快照通道**：

1. `WorkbenchCompletion` 的 L1 目录缓存按**连接**给出 `Catalog`（`objects` + `columns: (限定表名, 列名, Option<类型名>)`，见 `crates/editor/src/language/completion.rs:88`）；
2. `editor_catalog::pump`（`crates/workbench/src/view.rs:902`，1s 一拍）把「活动文档绑定连接」的目录按文档推进 `EditorShared::set_catalog_snapshot`；
3. hover / 补全 / 语义诊断读同一份快照（`catalog_snapshot_for` / `completion_catalog`）。

因此这条能力对**每个源库**一视同仁：MySQL、PostgreSQL、SQLite、Oracle、SQL Server、DuckDB……只要文档绑了连接、导航树把表列载进 L1 缓存，hover 就有「类型：X」与列列表；未绑定连接或目录没载好则如实退化（不给类型，不猜）。DuckDB 分析模式只是**其中一种**使用场景，不是它的前提。

### 8.3 仍未接线（压后项，逐条记在侧）

血缘、diff、计划面板、`builder` 构造器、方言动态注册这五块的**现状 / 为什么不接 / 什么条件再接 / 接线成本**单独立了一份决策侧记：

`docs/architecture/dependencies/sqlglot-unwired-backlog.md`

其中**列级血缘已经给出可执行设计**（先做「列上的一条来源链」，不做面板）：
`docs/architecture/editor/column-lineage-design.md`。

> `executor`（内存执行器）依旧**不采用**：无事务/并发/权限，本地分析一律走 DuckDB。

逐项源码证据与行为边界见 `docs/architecture/editor/editor-prototype-design.md` §7.4。
