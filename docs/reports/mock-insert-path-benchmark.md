# Mock 生成链路：插入路径基准与优化方向

测量对象：`MockEngine::generate_table` 里「生成值 → 落入内存 DuckDB 临时表」这一跳。
提问背景：落库那一跳已经 `ATTACH` + `INSERT SELECT`（数据不经 Rust 字符串），
但**生成到内存表**这一跳仍是「String → AST → SQL 文本 → DuckDB 再解析」。本文回答它值不值得换。

## 结论先行

**值得换，而且这是当前生成链路唯一的大杠杆。**

10 万行 × 5 列（id / name / email / amount / note，混合类型）：

| 阶段 | 耗时 | 占文本路径 |
| --- | --- | --- |
| `generate_cell` 取值 | 835 ms | 13.7% |
| `build_insert` 生成 SQL 文本 | 408 ms | 6.7% |
| `execute_batch`（DuckDB 解析巨型 INSERT） | **4 866 ms** | **79.6%** |
| **文本路径合计**（现状） | **6 110 ms** | |
| **Appender 合计**（取值 + 类型解析 + append） | **1 377 ms** | |

- **换 `duckdb::Appender` 后总耗时降到约 1/4（4.4×）**；
- 若只看 DuckDB 侧（`Appender` 合计 − `generate_cell` ≈ 542 ms vs `execute_batch` 4 866 ms），
  **数据库写入部分快约 9×**；
- 瓶颈**不在 Rust 侧**：搭 SQL 文本那 408 ms 只占 6.7%，优化它收益有限。

数据来源：debug 构建，10 万行 × 5 列，固定 seed。`crates/mock/src/engine.rs` 的
`perf_insert_path_probe`（临时探针，测完已删）。

### 为什么 debug 数据足以支撑结论

release 优化的只有 Rust 代码，而决定性数据项不受影响：

- `execute_batch` 4 866 ms 是 **DuckDB native**（C++），debug / release 基本同值；
- `generate_cell` / `build_insert` 是 Rust，release 下会显著变快（量级 10×）；
- 于是 release 下 `execute_batch` 占比**只会更高**（预计 > 95%），文本路径 ≈ 5 s、Appender 路径 ≈ 0.3 s，
  **加速比不小于 debug 的 4.4×**。

结论的方向（该换）与收益量级（数倍）都是稳的；绝对值以 release 复算为准。
复算命令见文末。

## 这个开销为什么躲过了先前的优化

README 写着「落库跨库直写：数据不经 Rust 字符串」——这没错，但它描述的是**出口**那一跳。
完整链路其实有两跳：

```
generate_cell ──► String ──► build_insert 文本 ──► execute_batch  ┐
                                                                  ├─► 内存临时表
（候选）generate_cell ──► 类型化值 ──► Appender.append_row ───────┘      │
                                                                        │
内存临时表 ──► ATTACH + INSERT SELECT ─────────────────────────────────► 项目分析库
```

出口侧（右下）已经优化过，左侧（生成→内存表）没有。
而左侧恰恰是每轮生成必经、且行数最大的一跳。

## 实施方案与代价

`duckdb` crate 已启用 `appender-arrow` feature（`Cargo.toml:112`），无需加依赖。可用 API：

- `Connection::appender(table)` → `Appender`
- `Appender::append_row(&[&dyn ToSql])` / `append_rows(iter)` / `flush()`
- `Appender::append_record_batch(RecordBatch)`（arrow 通道，见下）

两点代价需要接受：

1. **值必须类型化**。现在所有值都是 `String`，Appender 要求按表列类型给值
   （INTEGER 列要 `i64`，DOUBLE 列要 `f64`）。基准里这一步（字符串 → 类型解析）
   已计入 Appender 合计（1 377 ms 含它），所以上表的收益已经是**扣掉代价之后**的净收益。
   实施时按 `ColumnDataType` 分派即可，且这一份「列类型 → Rust 类型」的分派可以钉单元测试。
2. **NULL 仍要表示**。`AppenderParams` 支持 `Option<T>` → SQL NULL；本仓刚刚把表示从
   字符串哨兵 `"NULL"` 换成 `Option<&str>`（`crates/sql/src/builder.rs`，`build_insert`
   现接受 `&[Vec<Option<&str>>]`），这一步正好是 Appender 改造的前置准备。

### 更进一步：arrow 通道

`appender-arrow` 已开，`Appender::append_record_batch` 可以整批 `RecordBatch` 直灌。
mock 的列类型与 `to_duckdb_logical_type_for_field` 支持的类型要逐一对过（尤其 `DECIMAL` / `TIMESTAMP` /
`BLOB`），可行性需要单独立项验证；但方向上是「完全不再逐值过 Rust」。
若 arrow 通道打通，理论上还能吃掉 Appender 路径里剩余的取值 + 解析成本。

## 建议的推进顺序

1. 先按 `append_row` 落地（改动集中在 `generate_table` 一个批次循环内，收益已有 4.4×）；
2. 把 DuckDB ∨ Canvas 类型分派抽成纯函数并补单测（标准化 `AutoIncrement` / `Decimal` / `Date` 边界）；
3. 确有必要再评估 arrow 通道（`append_record_batch`）。

复算命令（release）：

```bash
cargo test -p rds-mock --lib --release -j 2 perf_insert_path_probe -- --nocapture
```

## 备注

- 本次所有临时脚本 / 日志落在 `.rds/tmp/`（不入库），报告入 `docs/reports/`；
- 探针代码（`crates/mock/src/engine.rs::perf_insert_path_probe`）**测完已删除**，不入库；
- `generate_table` 的「同一 monolist 每批 1 万行」分批策略保持不变——分批是为了取消令牌在批次边界
  响应（README §2），与插入方式无关。
