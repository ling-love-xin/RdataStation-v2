# #54 SQL 语义层：验证点与步骤清单

> 面向用户统一实测的记录（2026-10-24）。分工边界：结果集模块另一会话（不碰）、python LSP 放插件模块。
> 标注「✅ 已就绪可验」= 当前可测；「⏳ 待接线」= 能力已落地但运行时入口未通，当前测不到，需宿主注入后解锁。

## 一、状态总览

| 能力 | engine 层 | editor 接线 | UI | 实测状态 |
| --- | --- | --- | --- | --- |
| ① 语义诊断（未知表/未知列） | ✅ `c7de3e06` + 本轮 | ✅ 纯函数层（`Request.dialect` + 目录转换） | ⏳ 波浪线渲染待宿主注入目录+方言 | ⏳ 运行时暂不生效 |
| ② hover（函数/表/列） | ✅ `129eb656` | ❌ 未接 | ❌ 未做 | ⏳ |
| ③ 函数签名目录 | ✅ `d69a971f` | ❌ 未接 | ❌ 未做 | ⏳ |
| ③+ 函数手册（中文） | ✅ `2f90e066` | ❌ 未接（Quick Open 式交互待做） | ❌ 未做 | ⏳ |
| ④ 跳转定义 | ✅ `1136862f` | ❌ 未接 | ❌ 未做（导航树定位待做） | ⏳ |

> ⚠️ **运行时语义诊断为什么暂不生效（重要）**：编辑器诊断的唯一生产路径是 `editor/src/scan.rs` 扫描线程，
> 它按既有约定给**空目录** + `connection: None`（注释「目录与连接是补全面的，给空的不参与」）。
> 语义诊断（`engine::sql::diagnose`）无目录即退化空 → **当前运行时只能看到词法层波浪线**
> （未闭合字符串/括号不平衡），未知表/列的红波浪线要在「文档绑定连接 → 宿主把目录快照 + 方言（`driver_to_dialect`）带进扫描」之后才出现。
> editor 侧能力与测试已就位，接线点已定位（`scan.rs::run` 的 Request 构造处），剩余是 workbench 注入。

## 二、命令级验证（现在就能跑）

### 1. engine 语义层全量（158 项，含 #54 全部模块）

```powershell
cargo test -p rds-engine --lib sql::
```

| 过滤 | 覆盖 |
| --- | --- |
| `sql::semantic::` | 诊断（12 项）：未知表/未知列/`*` 跳过/无目录退化/非 SELECT 不产 |
| `sql::hover::` | hover（10 项）：函数签名+中文/表列列表/列归位 |
| `sql::functions::` | 签名目录（10 项）：系统表解析/内置兜底 50 函数 |
| `sql::function_manual::` | 中文手册（7 项）：中英混搜 157 条/9 类 |
| `sql::jump::` | 跳转（9 项）：裸名→目录限定名/列归位/函数不跳 |

### 2. editor 语义接线（31 项，含 3 项新语义诊断测试）

```powershell
cargo test -p rds-editor --lib language_service
```

新增 3 项：`semantic_diagnostics_report_unknown_table_and_column` /
`semantic_diagnostics_skip_clean_sql_even_with_catalog` /
`semantic_diagnostics_degrade_without_catalog`（空目录→不产，锁住「scan 路径不参与」口径）。

### 3. 全量回归

```powershell
cargo test -p rds-editor --lib          # 590 项全绿
cargo test -p rds-engine --lib          # 653 项全绿
```

## 三、UI 级验证（按可用性分批）

### 批次 A：现在可测（词法诊断 + 既有功能）

| # | 步骤 | 预期 |
| --- | --- | --- |
| A1 | 打开 SQL 文档，输入 `SELECT 'abc`（未闭合字符串） | 该行出现波浪线，悬停提示「未闭合」 |
| A2 | 输入 `SELECT (a;`（括号不平衡） | 「这个「(」没有闭合」波浪线 |
| A3 | 多语句 `SELECT (a;\nSELECT b);` | 两条各自报（按语句判，不互相抵消） |
| A4 | 大文件（>1MB） | 不扫描，无诊断也不卡顿 |
| A5 | 绑定连接后写 `SELECT * FROM 已知表` | 无波浪线（词法层干净） |

### 批次 B：待宿主注入后测（语义诊断上线）

> 2026-10-24 更新：**editor 侧端口已就绪**（`EditorShared::set_catalog_snapshot` + scan 提交链带目录与
> 方言，host `sql_dialect_for_document` 统一方言口径）；workbench 侧「文档绑定连接 → 目录快照推送」
> 为并行线设计，注入后 B1-B4 可测。

| # | 步骤 | 预期 |
| --- | --- | --- |
| B1 | 绑定连接 + 选 Schema，写 `SELECT missing FROM no_such_table` | `no_such_table` 报「未知表」、`missing` 报「未知列」红波浪线，区间精确指向原文 |
| B2 | `SELECT id, total FROM 真实表`（列都在目录里） | 无语义波浪线（宁缺毋滥） |
| B3 | 同一 SQL 在 MySQL / PG / DuckDB 连接间切换 | 方言跟随连接（`driver_to_dialect`），解析行为随方言 |
| B4 | 目录截断（大库） | 只报有把握的，不误报（截断列不产孤儿诊断） |

### 批次 C：hover / 签名 / 手册 / 跳转

| # | 步骤 | 预期 | 状态 |
| --- | --- | --- | --- |
| C1 | 悬停内置函数（如 `SUBSTR`） | 卡片：签名 + 中文描述 + 一句话示例 | ✅ 已接（`cd23301e`）**可测** |
| C2 | 悬停表名 | 目录真实限定名 + 列列表 | ⏳ 无目录时只显示引用词；列列表待目录注入 |
| C3 | 悬停列 | 所属表 + 列名 | ⏳ 同上 |
| C4 | 输入 `(`（函数调用处） | 签名帮助弹层（多参数高亮当前位） | ⏳ 内核无签名槽位，需自建弹层 |
| C5 | `Ctrl+Shift+M` / 工具栏书本图标打开函数手册，输入「截取」→ `SUBSTR` 命中，回车插入函数名到光标 | ✅ 代码完成 `cargo check` 通过；测试被系统内存阻塞（见 §六），跑通后提交即**可测** |
| C6 | Ctrl+Click 表名 | 跳转导航树定位到该表（限定名） | ⏳ 待做 |
| C7 | Ctrl+Click 列名 | 定位所属表并选中该列 | ⏳ 待做 |

## 四、关联遗留（#52/#53，与 #54 并行推进）

| # | 验证 | 步骤 | 预期 |
| --- | --- | --- | --- |
| #52 | 同连接串行/跨连接并发 | 两个连接各发一条长 SQL，再同连接发两条 | 不同连接并行跑；同连接第二条排队等第一条（不阻塞其他连接） |
| #53 | 补全缓存 TTL | `cargo test -p rds-workbench --lib editor_completion`（代码已提交，并行线交口后此命令可验证） | 缓存超时后重建、generation 失效不串数据 |

## 五、已记录但未决的点（不编造、待验证）

- 函数手册官方文档 URL：由 workbench 按「引擎+函数名」生成（真实环境验证后定，不写死链接）。
- 列类型进目录后 hover 补类型：`annotate_types` 现对多数表达式返回 Unknown，展示价值低，暂不做。
- 方言注入链路：workbench 文档绑定连接时把 `driver_to_dialect(驱动)` 写进文档状态 → `scan::Job.dialect` → 语义诊断生效（editor 侧字段已就位）。

## 六、C5 编译验证环境记录（2026-10-24）

- 代码：`function_manual.rs` + host.rs 工具栏书本按钮 + `FunctionManual` action + `Ctrl+Shift+M` + ui.rs 常量 + 测试 `opens_searches_and_confirms`。
- **编译已验证**：`cargo check -p rds-editor` Finished（依赖 `gpui-component 0.8.0` 升级 + check 覆盖 build 缓存，需完整重建）。
- **测试未跑**：系统 **Commit Charge 199%**（32.5GB/16.4GB，Windows 承诺超限）→ `cargo build`/`test` 完整 codegen 时 `rustc-LLVM ERROR: out of memory`（gpui-component 单 crate LLVM 内存不足）。非代码问题。
- 恢复条件：释放系统内存（关 chrome 标签 / 非必要 Doubao 窗口）或临时调大页文件后，跑 `cargo test -p rds-editor --lib` 全量回归（含 `opens_searches_and_confirms`）→ 绿后提交。
- 教训：依赖闭包大时**不要用 `cargo check` 混在 build 流程里**（check 产物覆盖 build rmeta，导致依赖全量重建）。
