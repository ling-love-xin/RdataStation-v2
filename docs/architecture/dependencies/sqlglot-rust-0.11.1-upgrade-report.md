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

## 8. 能力释放度（0.11.1 上重新确认）

**结论：没有完全释放。** 已接进产品的只有 6 项基础能力，另有 6 项「上游现成、探针实测可用、但产品还没接线」，
真正的差距不在库版本，而在我们没把 `Schema` 组装起来（血缘 / 类型标注 / 列限定都以它为前置）。

| 能力 | 状态 | 事实 |
| --- | --- | --- |
| 解析 / 语法校验 / 代码生成 / 方言转译 | ✅ 已用 | 执行路由、格式化、方言转移器；`transpile` 对脚本会**静默截断**（多条语句要 `transpile_statements`） |
| 词法 tokenizer（高亮） | ✅ 已用 | `position` 是**字符下标**，不能当字节偏移 |
| 语句切分 | ✅ 已用（自研） | `crates/sql/src/split.rs` 词法状态机，**故意不用 sqlglot**（未写完的文本也要能切） |
| 方言覆盖 | 🟡 部分 | 上游 30 种方言，我们在 `crates/sql` 里用到 11 种（Ansi 90 次、Postgres 25、Duckdb 24、Mysql 22、Sqlite 16、Oracle 12、BigQuery/Snowflake/Redshift 各 10、MsSQL 7、Tsql 5） |
| DDL / DML AST 构建（`builder` 全量） | 🟡 未接线 | 我们只用了 `select` / `select_all`，其余实体靠手写结构体字面量——这正是 0.11 加两个字段就要改代码的原因；改用建造器可降低这类升级成本 |
| 类型标注 `annotate_types` | ⚪ 实测可用、未接线 | 运算后仍保持类型（`total * 2 → Decimal{12,2}`），值查看器 / 图表轴 / 数字格式化直接受益 |
| 列级血缘 `lineage_sql` | ⚪ 实测可用、未接线 | 按输出别名能追到基列、表达式也能追；`editor-dev-plan` 里「分析结果血缘摘要」的遗留项就等它 |
| 作用域分析 `build_scope` | ⚪ 实测可用、未接线 | **无需 schema**，接线最轻；可用于补全排序（「当前语句引用了哪些表」） |
| AST 差异 `diff_sql` | ⚪ 实测可用、未接线 | 粒度到 `SelectItem` / `Expr` / `OrderByItem`；结果集对比（V1 有 `ResultDiffViewer`）、笔记版本链 diff |
| 谓词下推 `pushdown_predicates` | ⚪ 实测可用、未接线 | 过滤能推入派生表；**保留**：`unnest_subqueries` 改写与 NULL 语义不等价 |
| 本地查询计划 `plan` | ⚪ 实测可用、限定用途 | 只是 sqlglot 自建计划，**不是源库 EXPLAIN**；DDL 直接报不支持 |
| Schema / 方言插件 / 时间格式 | ⚪ 未用 | `format_time_dialect` 是「方言转移器」的高价值子集（时间格式串转换） |
| 内存执行器 `executor` | ⛔ 不采用 | 只在内存表上跑，无事务/并发/权限；本地分析一律走 DuckDB |

接线优先级（按「产品价值 / 成本」排）：**① `build_scope`（免 schema，补全排序）→ ② `annotate_types` + `MappingSchema`
（值查看器类型）→ ③ `lineage_sql`（结果血缘）→ ④ `diff_sql`（结果对比）→ ⑤ `pushdown_predicates`（联邦改写，须先定 NULL 语义）**。
②③ 的共同前置「驱动类型 → `ast::DataType` 映射表 + 从 `MetadataService` 组装表列」才是真正的工程量。
逐项源码证据与行为边界见 `docs/architecture/editor/editor-prototype-design.md` §7.4（已同步到 0.11.1）。
