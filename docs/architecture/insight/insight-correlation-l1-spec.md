# 洞察相关性候选视图（L1）规格

- 状态：待评审（设计稿，未落代码）
- 日期：2026-11-07
- 适用范围：`crates/insight`（关联性区 / 报表 Interactions 段的数据源）
- 依据：`docs/analysis/洞察报表能力评估-ydata式报表与DuckDB统计分析扩展.md` §5；本文所有成本数字均为 DuckDB 1.5.5 实机实测（100 万行，脚本 `.rds/tmp/duckdb_probe4.py`）

---

## 1. 目标与非目标

**目标**：用户选中一列后，回答"**和它最相关的是谁**"，并给出可解释、可回连的候选列表。

**非目标**：

- 不做默认的 N×N 全景矩阵（那是 L3，用户显式触发）；
- 不做显著性检验（无 t 分布能力，且不引社区扩展，理由见分析文档 §2.5）；
- 不做时序相关（日期/时间列另立"时序"段）。

---

## 2. 输入与前置数据

| 输入 | 来源 | 说明 |
| --- | --- | --- |
| 焦点列 `focus` | 用户选择 | 必须已通过 §4 的排除规则，否则直接空态 |
| 列类型 | 列画像 | `Numeric` / `Text` / `Boolean` / `Datetime` / 其他 |
| `approx_unique` | 列画像或一次 `approx_count_distinct` | 判常量与高基数；列画像的 `unique_count` 是 `Option<u32>`（`model/types.rs:34`），为 `None` 时补一次查询 |
| `null_rate` | 列画像或一次多列 `count` | 判高缺失；`null_count` 已在 `model/types.rs:32` 常驻，无需补查 |
| `count` | 表级 | 有效样本上限 |

列画像缺失时补一次：实测 30 列 `approx_count_distinct` **0.8 ms**、30 列 `count` **1.1 ms**（100 万行），可忽略。

---

## 3. 可比性矩阵（什么和什么才能算相关）

| 焦点类型 | 候选类型 | 方法 | 说明 |
| --- | --- | --- | --- |
| Numeric | Numeric | `pearson` + `spearman` | 两个都算，默认展示 pearson；差异大时提示（见 §5） |
| Numeric | Boolean | point-biserial | 等价于 0/1 编码后的 pearson，标注为 point-biserial |
| Numeric | Categorical（≤50 类） | **不做相关** | 引导到 `grouped-stats`（分组均值对比） |
| Categorical | Categorical | `cramer-v` | 2×2 时数值上等价于 phi 系数 |
| Categorical | Numeric | **不做相关** | 引导到 `grouped-stats` |
| 任意 | Datetime | **排除** | 时序相关另段处理（fg 也是独立 timeseries 段） |
| 任意 | Text（distinct > 50） | **排除** | 文本相似度不是"相关"，且列联表无意义 |
| Text（distinct ≤ 50） | 任意 | 视为 Categorical | 低基数文本才有统计意义 |
| 任意 | Binary / JSON / Array / UUID | **排除** | 无定义或纯 ID |

---

## 4. 排除与降权规则（阈值与理由）

| 条件 | 处理 | 理由 |
| --- | --- | --- |
| `approx_unique ≤ 1` | 排除 | 常量列，相关系数无定义 |
| `null_rate > 40%` | 排除 | 剩余样本太少，结论不稳 |
| 有效样本 `n < 30` | 排除 | 相关系数在小样本上方差极大 |
| 类别 `distinct > 50`（或 `> n/20`） | 排除出 cramer-v | 列联表自由度爆炸，且多是 ID 类 |
| 数值列 `top1 占比 ≥ 99%` 或 `IQR = 0` | **降权**（排到末尾 + 标"近似常量"） | 仍可能有用，但不该占据候选头部 |
| 列名匹配 `(^id$|_id$|Id$)` 且 `distinct ≈ count` | 排除（可配置关闭） | 启发式识别主键/外键 |
| 焦点列本身命中上述任一排除项 | 空态 + 说明原因 | 不静默返回空列表 |

**判定顺序**（顺序本身是规则的一部分）：类型可比性 → 缺失（含全 NULL）→ 样本量 → 常量 / 近似常量 → ID 启发式。顺序错了会给出错误的理由，也会放过极端输入——实测把样本量判在缺失之前时，"全 NULL 列"被标成 `TooFewRows`（见 §15 A）。

**为什么阈值取这几个数**：40% 缺失、50 类、n<30 都是"再往下就不该给结论"的经验边界，不是统计定理。它们必须可配置（写在规则/设置里），并且**在 UI 上可见**（例如"已隐藏 3 个高缺失列"），否则用户会以为列的候选就这么少。

---

## 5. 排序、K 与判读

- **K**：默认 12，上限 20。K 不随列数放大——候选列表是"给你看重点"，不是"给你看全部"。
- **排序**：`|value|` 降序；同值按 `n` 降序；再同则按列名字典序（保证结果稳定，便于缓存命中）。
- **两阶段取法**：phase 1 **只批量算 pearson**；**spearman 只对 Top-K 逐对算**。批量秩物化实测 3.0 s / 10 列且吃内存，逐对只要 80 ms / 对（见 §15 D）。
- **不做 p 值**：没有 t 分布能力，且不引社区扩展。改用**经验门槛**，并在 UI 明示"未做显著性检验"：
  - `|r| ≥ 0.7` → 强
  - `0.3 ≤ |r| < 0.7` → 中
  - `|r| < 0.3` → 弱（仅当 Top-K 不足时展示）
- **线性 vs 单调**：同时算 pearson 与 spearman，若 `|pearson - spearman| > 0.15`，在该行标"单调强于线性"，提示存在非线性或异常值。
- **只展示不算全**：Top-K 之外的结果仍可保留在缓存里（用户点"看更多"时不必重算），但不渲染。

---

## 6. 计算形态（本次实测支撑，直接决定实现方式）

| 方案 | 45 对 / 100 万行 | 435 对 / 100 万行 |
| --- | --- | --- |
| **一条 SQL 多聚合** | **1.2 ms** | **12.6 ms** |
| 逐对发 SQL | 168.2 ms | 未测（按比例更差） |
| 10% 抽样 + 一条 SQL | — | 14.8 ms（**比全量更慢**） |

三条结论：

1. **相关性不能走"每对一条规则"的执行方式**。现有规则引擎是单对模板（`parameters = ["table","col1","col2"]`），45 对就是 45 次查询，慢约 140 倍。
2. **一次 SQL 多聚合即可**，N² 在这个量级根本不"重"——`workspace/rel.rs` 里"矩阵化两两相关负载重"的判断，只在逐对执行的前提下成立。
3. **抽样无收益**：相关性计算本身极便宜，抽样只是多一次扫描。仅为"限流"而抽样是负优化。
4. **但第 2 条只对 pearson 成立**。spearman 要先有秩：单对 80.4 ms（可接受），而 10 列平均秩物化 **3,015.3 ms**（裸 `RANK()` 1,449.0 ms），秩表还占大块内存——同一会话累积多张时直接 OOM（12.5 GiB 上限）。所以**pearson 批量、spearman 逐对（仅 Top-K）**，是两条不同的路径。

### SQL 骨架（phase 1：全部候选值）

```sql
SELECT
    corr("c1", "c2") AS "c1|c2",
    corr("c1", "c3") AS "c1|c3",
    ...
FROM "tbl"
-- 若带行过滤：WHERE <filter>
```

### phase 2：只为 Top-K 补 `n`

每对的非空样本数不同，不能共用 `count(*)`。但只对 Top-K（≤20）补一次 `count(a)`/`count(b)` 的联合非空计数即可，不必给全部 N² 对都算，避免聚合数翻倍。

```sql
SELECT
    COUNT(*) FILTER (WHERE "c1" IS NOT NULL AND "c2" IS NOT NULL) AS "n:c1|c2",
    ...
FROM "tbl"
```

---

## 7. 输出模型

```rust
struct CorrelationCandidate {
    column: String,
    column_type: ColumnType,
    method: CorrMethod,      // Pearson | Spearman | CramerV | PointBiserial
    value: f64,
    n: u32,
    flags: Vec<CandidateFlag>, // NearConstant | MonotonicGtLinear | LowSample
    basis: Basis,            // Full | Sample(n)
}
```

`basis` 必须随结果一起返回：报表/Markdown 输出时要能写清"基于 N 行"。

---

## 8. 呈现

- **列表**：`table` 组件（列名 / 类型 / 方法 / 值 + 横向 `BarChart` 条 / n / 标记）。横向条用现成的 `BarAlignment` + `Bar::fill`，按强/中/弱三档着色。
- **交互**：点某一行 → L2（该对散点 + 明细 + 回连原表）。这是"抄结构不抄死格子"的落点。
- **方法切换**：pearson / spearman 用 `TabBar::segmented()` 切换，切换不重算（两个值 phase 1 已一起算好）。
- **空态**：必须说明原因——"当前表只有 1 个数值列"/"其余列均为高基数 ID"/"3 列因缺失率过高被隐藏"。
- **键盘可达**：行内可点元素走 `Button`/`List`，不自绘 `div().on_click()`。

---

## 9. 缓存与失效

- **键**：`(temp_table, focus_col, 候选列集合 hash, method_version, 数据版本)`
- **不含 `filter`**：行过滤在**建临时表**阶段就固化了（`ProbeConfig.filter`，`model/probe.rs:57`），换过滤 = 换临时表，键自然变（见 §12.2）
- **失效**：表结构变更、数据版本变更、口径（方法版本）变更
- 切换方法不失效（同一批聚合已覆盖）

---

## 10. 验收标准

| 项 | 标准 |
| --- | --- |
| 正确性 | 与 numpy/pandas 对拍，pearson/spearman 容差 `1e-9` |
| 边界 | 常量列（排除）、全 NULL 列（排除）、两列完全相同（r=1）、反向（r=-1）、n=29（排除） |
| 性能 | ≤30 列 × 100 万行，phase 1 ≤ 30 ms（实测 12.6 ms，留 2 倍余量） |
| Spearman | Top-K 逐对（K=12）≤ 1.5 s（实测 80.4 ms/对）；允许先出 pearson 列表、spearman 异步回填 |
| 交互 | 点行能打开 L2；Tab 能聚焦到行；方法切换不触发重算 |
| 文案 | 空态给出具体原因，不输出"暂无数据"这种无语义文案 |

---

## 11. 开放问题（需产品侧拍板）

1. **分类×分类是否也给"值重叠"候选**（`overlap`）？现在关联性区已有"值重叠"子 Tab，两者是否合并成一张候选表。
2. **行过滤是否参与相关计算**？若参与，缓存键必须带 filter，且 UI 要标注"基于筛选后的 N 行"。
3. **K 是否自适应**？例如 `K = min(20, N)`，列很多时适度放宽。
4. **是否允许用户手动加入被排除的列**（"我知道它是 ID，但我想看"）？倾向允许，但要标"已排除但手动加入"。

---

## 12. 决策落地（2026-11-07 已确认）

### 12.1 分类 × 分类 **不并进**"值重叠"候选表

"值重叠"（`RelTab::Overlap`，`workspace/rel.rs:39`）是**跨表**排查同名列的集合关系（T19 关系引擎），语义是"两张表的这列值覆盖多少"；相关性是**表内**统计关系。两者合并会让"候选"这个词同时指两种东西，还会把跨表跳转塞进表内列表。保持分开。

### 12.2 行过滤参与计算——**实际上已经自动满足**（代码事实，不是我新加的设计）

`ProbeConfig.filter`（`model/probe.rs:57`，`WHERE` 主体串）在**建分析临时表**阶段就被用掉了。L1 计算读的是 `sample_table`（`model/panel.rs:45`）返回的那张临时表，所以：

- 用户筛了行 → 临时表里就是筛过的行 → 相关性天然基于筛选后的数据；
- **缓存键不需要 `filter` 字段**（§9 已修正）：换过滤等于换临时表，键本身就变了；
- 副作用是：**结果必须说明口径**，否则用户不知道自己看的是"全量"还是"前 500 行"。

### 12.3 由 12.2 追加的硬要求：`basis` 必须来自探查配置

`InsightView::last_probe_config: Option<ProbeConfig>`（`insight_view/mod.rs:311`）里已有 `amount`（`RowCount(n)` / `Percent(p)` / `Full`）与 `filter`。L1 结果必须带一句人话口径，例如：

- "全量"
- "前 500 行"
- "12.5% 抽样（≈124,000 行）"
- 带过滤时追加"已应用行过滤"

**并且**：当口径不是 `Full` 时，界面顶部要有一条提示——"当前为样本口径，强相关可能不稳定，建议切全量复核"。默认探查是前 500 行（`ProbeConfig::sample_default()`），在 500 行上算出的 `r=0.82` 大概率站不住，这条提示是必需项而不是装饰。

---

## 13. 实施任务书（三刀，每刀独立可验证、可回滚）

### 刀 1：模型 + 查询 + 测试（**不改任何 UI**，风险最低）

| 动作 | 位置 |
| --- | --- |
| 新增模型 | `crates/insight/src/model/correlation.rs`（`CorrMethod` / `CorrFlag` / `Basis` / `CorrelationCandidate` / `CorrelationCandidates { focus, focus_kind, candidates, excluded, basis_note }`），在 `model/mod.rs` re-export |
| 纯函数筛选 | `plan_candidates(focus, cols, limits) -> Plan`：输入 `ColumnMeta { name, kind, unique_count: Option<u32>, null_count, count }`，输出 `targets` + `excluded: Vec<(String, ExclusionReason)>`。理由枚举：`Constant / TooManyNulls / TooFewRows / HighCardinality / LooksLikeId / NotComparableType` |
| SQL 构造 | `sql::values(temp_table, focus, targets, method)`：一条 SELECT，字段名做 `"` → `""` 转义；`sql::pair_n(...)`：phase 2 只为 Top-K 补联合非空 `n` |
| 事件 | `InsightEvent::CorrelationCandidatesRequested { temp_table, focus_column, k }`，加在 `insight_view/mod.rs:111` 附近 |
| 分发 | `jobs/mod.rs::handle_event` 加一臂；新增 `jobs/correlation.rs::request_correlation_candidates` |
| 写回 | `PanelData`（`model/panel.rs:30`）加 `corr: Option<CorrelationCandidates>`——**所有构造点都要补 `None`**（含集成测试里的构造点，漏了会 E0063） |
| 测试 | ①`plan_candidates` 六类排除各一例 + 正常例；②SQL 字符串快照（固定列名、含中文列名与含 `"` 的列名各一例）；③小表数值对拍（pearson/spearman 与手算一致，容差 `1e-9`） |

**刀 1 的范围收窄说明**：只做**数值族**（`Numeric × Numeric` = pearson + spearman；`Numeric × Boolean` = point-biserial）。类别 × 类别（cramer's V）需要"每对一个列联表"，形态与数值路径不同，收益也低——先不做，继续走已有的"分类关联"子 Tab（单对执行）。

验收：`cargo test -p insight` 全绿；界面零变化。

### 刀 2：L1 视图（数值族）

- `workspace/rel_corr` 主体改为：焦点列 `Select` + 候选行列表 + 空态 + 排除提示；
- 候选行：列名 / 类型 / 方法 / 值 + 横向条 / `n` / 标记（`NearConstant`、`MonotonicGtLinear`）。横向条用 `chart` 的 `BarAlignment` + `Bar::fill`，三档色（强/中/弱）——**取色走主题 token，不写裸色**；
- 方法切换用 `TabBar::segmented()`，切换**不重算**（两个值 phase 1 已一起算好）；
- 空态用 `empty::Empty`（禁手搓居中块）；排除提示用 `Badge` + 一句"已隐藏 N 列"，点开可见原因；
- 现有"手工选列对 → 跑多列规则"路径**保留**，收进 `Collapsible` 作高级入口，不删能力；
- `ElementId` 用列名，不用下标；行内可点区域用 `Button`/`List`。

验收：`cargo test -p ui_guard` 通过（token 契约）；键盘能走到候选行；空态文案具体（"当前表只有 1 个数值列" / "3 列因缺失率过高被隐藏"）。

### 刀 3：报表 + 缓存 + 口径标注

- `workspace/markdown.rs` 关联性段插入 Top-K 候选表，末尾注明"未做显著性检验"；
- 缓存：`InsightWorkspacePanel` 加 `corr_cache: Option<(CacheKey, CorrelationCandidates)>`，命中不重算，界面与 Markdown 共用同一份；
- `basis_note` 由 `last_probe_config` 生成；非 `Full` 时界面顶部挂样本口径提示条；
- 验收：界面数字与 Markdown 数字逐位一致（同一 cache）。

**回滚**：三刀各自独立，刀 1 合入后行为不变；刀 2/3 任一不适可单独 revert，不影响底层。

---

## 14. 不做什么 / 已知风险

**不做**（本轮）：

- L3 N×N 全景矩阵与 `MatrixGrid`（§5 已降级；计算虽只要 12.6ms，但渲染与交互是另一件事）；
- 类别族 L1（cramer 多对同查形态不同，先走单对现有链路）；
- 显著性检验 / p 值；引社区统计扩展；
- 为相关性抽样（实测是负优化）；
- 值重叠与相关候选合并。

**风险**：

1. **默认 500 行取样**：这是最容易误导用户的一条——数字漂亮但没有统计意义。§12.3 的提示条是硬性验收项。
2. **常量列返回 NULL**：DuckDB 的 `corr()` 在零方差列上给 `NULL`，解析必须容错，不能 `unwrap`。
3. **标识符转义**：列名含 `"`、含中文、含空格都要走双引号转义（`"` → `""`），必须有单测，否则 SQL 注入/语法错误。
4. **`PanelData` 加字段**：仓库内多处构造点（含集成测试），漏改是编译期错误还好，若走 `Default` 则可能静默为空——按"构造点全量搜一遍"处理。
5. **常量列返回的是 `NaN` 而不是 `NULL`**（实测）：`corr(常量, x)` 给 `NaN`，`corr(x, 全NULL)` 才给 `NULL`。Rust 侧按 `f64` 接收时必须 `is_finite()` 过滤，否则排序里 `partial_cmp` 返回 `None`、界面会显示 `nan`。
6. **只有 1 行数据时 `corr` 返回 `1.0`**（实测，而不是 `NULL`）——伪值。`n < 30` 的排除不是"优化"，是防止把无意义输入渲染成满格强相关。

---

## 15. 验证记录（2026-11-07，第二轮：临时库验证 L1 假设，未改仓库代码）

环境：Python `duckdb 1.5.5`，内存库，`max_memory = 12.5 GiB`、8 线程。
脚本：`.rds/tmp/duckdb_probe5.py` / `probe6.py` / `probe7.py`（过程产物，未入库）。

### 15.1 A：`plan_candidates` 在脏表上的行为

表：5,000 行，含常量列、全 NULL、98% 缺失、仅 29 行、ID 列、200 类文本、5 类文本、布尔，以及列名含中文 / 空格 / 双引号 / 保留字 `select`。

| 列 | 判定 | 是否符合预期 |
| --- | --- | --- |
| `const_col` | `Constant` | 是 |
| `all_null` | `TooFewRows` | **理由不准**，应为 `AllNull` / `TooManyNulls` → 已改判定顺序（§4） |
| `sparse_98` | `TooManyNulls` | 是 |
| `tiny`（29 行） | `TooFewRows` | 是 |
| `cat_hi`（200 类） | `NotComparableType` | 是 |
| `cat_low`（5 类） | `NotComparableType` | 是（数值焦点下文本不参与相关） |
| `user_id`（bigint） | `NotComparableType` | 被类型先拦下，**没走到 `LooksLikeId`** → ID 启发式只在数值族内生效，仍需保留 |

### 15.2 B：批量 pearson + 列名转义 + 布尔强转

6 个候选列（含中文、空格、双引号、保留字四种地雷名），**一条 SQL，1.67 ms**；与纯 Python 手算逐项对拍，最大误差 **3.7e-16**（容差要求 1e-9）。

结论：`"` → `""` 的转义规则可用；布尔列需 `CAST(... AS INT)` 后才能进 `corr`。

### 15.3 C：退化输入的返回形态

| 输入 | `corr` 返回 |
| --- | --- |
| 常量列 | `nan` |
| 全 NULL 列 | `NULL` |
| 仅 29 行 | 1.0（正常值） |
| 仅 1 行 | **1.0（伪值）** |

→ 已写入 §14 风险 5 / 6。

### 15.4 D：Spearman 的两件事

**① 现有规则有口径偏差。** `spearman-correlation.rule.toml` 用裸 `RANK()`（竞赛排名），在"并列组大小不等"的数据上与平均秩口径不一致：rho `0.83784` vs `0.84402`，偏差 **0.0062**。并列组等大时两者恰好构成仿射关系，所以普通数据上看不出差异——这类偏差是隐藏的。

**② 正确口径可以一条 SQL 算。** 平均秩 = `RANK() OVER (ORDER BY c) + (COUNT(*) OVER (PARTITION BY c) - 1) / 2.0`；用它算出的 rho 与 Python 平均秩口径一致到 **1e-10**。

**③ 但批量代价不小**（100 万行 / 10 列）：

| 操作 | 耗时 |
| --- | --- |
| 单对 spearman（2 个窗口） | 80.4 ms |
| 10 列平均秩物化 | **3,015.3 ms** |
| 10 列裸 `RANK()` 物化 | 1,449.0 ms |
| 10 列 × 10 万平均秩物化 | 406.3 ms |
| 秩表上 45 对 `corr` | 84.4 ms |
| 原表上 45 对 `corr` | 12.6 ms |

结论：**pearson 批量、spearman 逐对（仅 Top-K）**。批量秩物化的写表与内存开销远大于逐对查询之和；同一会话累积多张秩表会撞 12.5 GiB 上限（第一轮 `probe5` 就 OOM 了一次）。

### 15.5 由此对规格的修改

1. §4 增加"判定顺序"，全 NULL 列不再被标成 `TooFewRows`；
2. §5 增加"两阶段取法"（pearson 批量 / spearman 逐对）；
3. §6 结论加第 4 条（批量只对 pearson 成立）；
4. §10 增加 Spearman 性能行；
5. §14 增加 `NaN` 与 `n=1` 两条风险。

### 15.6 仍未验证

- 以上是 Python 绑定（同版本内核），**未在项目 `duckdb-rs` 调用链上复跑**；
- 列名转义在 `duckdb-rs` 的 identifier 路径上是否一致，未测；
- 中文列名从 Rust 侧拼进 SQL 的编码通路未测；
- 批量 spearman 的 OOM 阈值未系统测，只观察到"10 列 + 多张大表并存"会撞上限。
