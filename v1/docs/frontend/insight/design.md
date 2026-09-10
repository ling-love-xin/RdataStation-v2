# RdataStation 洞察模块 — 完整设计文档

> 版本：v1.3
> 创建日期：2026-05-08
> 最后更新：2026-05-09
> 状态：✅ 设计完成（Phase 22 审计修复已同步：render字段 + 死类型清理 + MultiRuleResult收紧 + isOpen清理 + handleCleanup修复 + 文档一致化）
> 定位：洞察模块的权威设计规范，涵盖架构、协议、前端、后端、规则系统、扩展点、性能策略和演进路线

---

## 目录

1. [设计理念与定位](#一设计理念与定位)
2. [架构设计](#二架构设计)
3. [后端设计 (Rust)](#三后端设计-rust)
4. [规则系统设计](#四规则系统设计)
5. [前端设计 (Vue 3 + TypeScript)](#五前端设计-vue-3--typescript)
6. [API 协议设计](#六api-协议设计)
7. [数据流设计](#七数据流设计)
8. [类型系统设计](#八类型系统设计)
9. [性能策略](#九性能策略)
10. [扩展点设计](#十扩展点设计)
11. [安全与错误处理](#十一安全与错误处理)
12. [演进路线图](#十二演进路线图)

---

## 一、设计理念与定位

### 1.1 模块定位

洞察模块是 RdataStation 的**数据质量与统计分析子系统**，对标 DataGrip 的 Quick Documentation + Schema Analysis + DBeaver 的数据探查功能。

**一句话定义：** 对数据库表/列进行自动化的统计分析、质量评估和异常检测，并将结果以可视化的方式呈现给用户。

### 1.2 核心设计原则

```
SQL 是数据，Rust 是执行引擎，TOML 是合约
```

| 原则             | 说明                                                  |
| ---------------- | ----------------------------------------------------- |
| **规则驱动**     | 所有分析逻辑通过 TOML 规则定义，Rust 代码零硬编码 SQL |
| **DuckDB 沙箱**  | 所有计算在 DuckDB 临时表中完成，不污染源数据库        |
| **零拷贝传输**   | 统计结果以 JSON 格式序列化，通过 Tauri IPC 传递       |
| **懒加载**       | 分析按需触发，不在查询时自动执行                      |
| **可插拔**       | 规则引擎、质量评分、前端组件均支持扩展                |
| **前端存算分离** | 原始数据仅在 DuckDB，前端只接收聚合统计结果           |

### 1.3 与相关文档的关系

```
INSIGHT-DESIGN.md (本文档)    — 权威设计规范，定义"应该怎么做"
  └─ INSIGHT-ARCHITECTURE.md  — 当前实现架构，记录"已经做了什么"
  └─ INSIGHT-API-REFERENCE.md — API 接口参考，记录"有哪些接口"
  └─ INSIGHT-RULE-FORMAT.md   — 规则格式参考，记录"怎么写规则"
  └─ INSIGHT-DEV-PROGRESS.md  — 开发进度跟踪，记录"做了哪些阶段"
```

---

## 二、架构设计

### 2.1 四层架构

```
┌──────────────────────────────────────────────────────────────┐
│                    UI Layer (Vue 3 + TS)                     │
│  ColumnInsightPanel  TableProfileView  SchemaInsightPanel   │
│  DataVisualizationPanel  MultiColumnView                     │
├──────────────────────────────────────────────────────────────┤
│                  IPC Layer (Tauri Command)                    │
│  get_column_insight_full  save_insight_snapshot              │
│  get_insight_version_detail  list_insight_versions           │
│  evaluate_quality_rule  batch_evaluate_columns               │
│  execute_insight_rule  list_insight_rules                    │
│  get_schema_insight  profile_column_from_table               │
├──────────────────────────────────────────────────────────────┤
│                 Service Layer (Rust)                         │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────┐  │
│  │InsightEngine │  │QualityScorer │  │ SchemaAnalyzer   │  │
│  │(统计计算)     │  │(质量评分)     │  │(结构分析)        │  │
│  └──────┬───────┘  └──────┬───────┘  └────────┬─────────┘  │
│         │                 │                   │             │
│  ┌──────┴─────────────────┴───────────────────┴─────────┐  │
│  │                  DuckDB Service                       │  │
│  │  (temp table lifecycle, TTL eviction, DuckDBManager) │  │
│  └──────────────────────────────────────────────────────┘  │
├──────────────────────────────────────────────────────────────┤
│                 Insight Core (Rust)                          │
│  ┌────────────────┐  ┌────────────────┐  ┌──────────────┐  │
│  │  RuleRegistry  │  │  RuleExecutor  │  │  RuleTypes    │  │
│  │  (规则加载/查询)│  │  (SQL 模板执行) │  │  (类型定义)    │  │
│  └────────────────┘  └────────────────┘  └──────────────┘  │
├──────────────────────────────────────────────────────────────┤
│                 Persistence Layer                            │
│  ┌──────────────────┐  ┌────────────────────────────────┐  │
│  │ DuckDB Analytics │  │  SQLite (meta/project.db)       │  │
│  │ (洞察数据/版本)   │  │  (快照索引/元数据)               │  │
│  └──────────────────┘  └────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────┘
```

### 2.2 模块依赖关系

```
UI Layer ← IPC Layer ← Service Layer ← Insight Core ← Persistence
                ↑            ↑                ↑
           禁止越界      禁止绕过 Service    Core 不依赖 Datasource
```

**层级规则：**

- UI 层只能通过 Tauri Command 调用后端
- Tauri Command 只能调用 Service 层
- Service 层可以调用 Insight Core 和 DuckDB Service
- Insight Core 独立于任何 datasource 实现
- 禁止 Service 层直接访问 datasource 层

### 2.3 核心模块职责

| 模块           | 文件                         | 职责                                        |
| -------------- | ---------------------------- | ------------------------------------------- |
| RuleRegistry   | `insight/rule_registry.rs`   | 加载、缓存、查询规则（内置+用户）           |
| RuleExecutor   | `insight/rule_executor.rs`   | 解析 SQL 模板 → 填充参数 → 执行 → 返回 JSON |
| InsightEngine  | `services/insight_engine.rs` | 协调列统计计算（类型检测 → 委托规则执行）   |
| QualityScorer  | `services/quality_scorer.rs` | 四维度质量评分 + 表级质量聚合               |
| SchemaAnalyzer | `insight/schema_analyzer.rs` | 外键推断、类型一致性、孤立表、健康评分      |
| DuckDbService  | `services/duckdb_service.rs` | DuckDB 实例管理、临时表生命周期、TTL 淘汰   |

---

## 三、后端设计 (Rust)

### 3.1 核心类型定义

```rust
// ===== insight/rule_types.rs =====

/// 规则元数据（TOML [meta] 段）
struct RuleMeta {
    id: String,              // 唯一标识，如 "numeric-stats"
    name: String,            // 显示名称
    category: RuleCategory,  // column | multi | table | quality
    applies_to: Vec<String>, // 适用数据类型 ["Numeric", "Text"]
    description: Option<String>,
}

enum RuleCategory {
    Column,   // 单列分析
    Multi,    // 多列组合分析
    Table,    // 表级分析
    Quality,  // 质量门控规则
}

/// 完整规则定义（TOML 完整文件）
struct RuleFile {
    meta: RuleMeta,
    query: RuleQuery,
    output: Vec<OutputField>,
    quality: Option<QualityRule>,     // Quality 类规则专用
    render: Option<RenderHint>,       // 前端渲染提示
}

/// SQL 模板定义
struct RuleQuery {
    template: String,        // SQL 模板，支持 {table} {col} 等参数
    parameters: Vec<String>,  // 参数列表
}

/// 输出字段映射
struct OutputField {
    sql_name: String,        // SQL 输出列名
    json_name: String,       // JSON 键名
    value_type: ValueType,   // 数据类型 + 可空性
}

enum ValueType {
    F64,     // 浮点数
    I64,     // 整数
    Str,     // 字符串
    Bool,    // 布尔
    F64Null, // 可空浮点数
    I64Null, // 可空整数
}

/// 图表/组件渲染提示
///
/// 有效 component 值（当前 18 条内置规则中共使用 12 种）：
///
/// | 值                     | 所属规则               | 说明               |
/// |------------------------|------------------------|--------------------|
/// | `NumericStatsTable`    | numeric-stats, numeric-basic | 数值统计表格  |
/// | `TextFrequencyBar`     | text-frequency         | 文本频率柱状图     |
/// | `TextLengthStats`      | text-length            | 文本长度统计表格   |
/// | `DateTimeRangeTable`   | datetime-range         | 日期范围统计表格   |
/// | `DateTimeMonthlyChart` | datetime-monthly       | 日期按月分布图     |
/// | `BooleanStatsInfo`     | boolean-ratio          | 布尔值比例信息卡   |
/// | `HistogramChart`       | histogram              | 数值分布直方图     |
/// | `NullRateBarChart`     | null-check             | 空值率柱状图       |
/// | `CorrelationMatrix`    | correlation            | 多列相关性矩阵     |
/// | `ScatterPlot`          | scatter-sample         | 多列散点采样图     |
/// | `GroupedStatsTable`    | grouped-stats          | 分组聚合统计表格   |
/// | `CrossTabHeatmap`      | cross-tab              | 交叉表热力图       |
///
/// table/ 和 quality/ 规则不使用 RenderHint（由对应的固定面板渲染）。
struct RenderHint {
    component: Option<String>,    // 建议渲染类型，值域见上表
    display_order: Option<u32>,   // 显示优先级
}

/// 质量门控规则（QualityRule）
struct QualityRule {
    gate: QualityGate,
    dimensions: Option<Vec<QualityDimensionDef>>,
}

enum QualityGate {
    Transform,  // QualityRule 结果覆盖 QualityScore
    Merge,      // QualityRule 结果合并到 QualityScore
}

/// 质量报告（规则执行结果）
struct QualityReport {
    overall_score: f64,
    grade: String,        // "excellent" | "good" | "fair" | "poor" | "critical"
    dimensions: Vec<QualityDimension>,
}

struct QualityDimension {
    name: String,
    score: f64,
    label: String,
    weight: f64,
    issues: Vec<QualityCheck>,
}

struct QualityCheck {
    field: String,
    current: serde_json::Value,
    expected: Option<serde_json::Value>,
    severity: CheckSeverity,
    message: String,
}

enum CheckSeverity {
    Info,
    Warning,
    Critical,
}

// ===== services/result_service.rs =====

/// 列洞察完整结果（发送到前端）
struct ColumnInsightFull {
    stats: ColumnStats,           // 统计计算
    histogram: Option<Vec<DistributionBin>>, // 直方图（仅 Numeric）
    sample: Vec<Row>,             // 样本值 (Object { value, row_index })
}

#[derive(Clone, Serialize, Deserialize)]
struct ColumnStats {
    column_name: String,
    data_type: String,            // DuckDB typeof() 返回的原始类型
    total_count: usize,
    null_count: usize,
    null_rate: f64,
    unique_count: Option<usize>,
    stats_detail: ColumnStatsDetail,
}

/// 统计详情（按数据类型变体）
#[derive(Clone, Serialize, Deserialize)]
enum ColumnStatsDetail {
    Numeric(NumericStats),
    Text(TextStats),
    DateTime(DateTimeStats),
    Boolean(BooleanStats),
}

// 各类型统计结构...
```

### 3.2 类型检测与分派

```
DuckDB typeof(column) 返回值
  │
  ├─ BIGINT/INTEGER/SMALLINT/TINYINT/DOUBLE/FLOAT/... → is_numeric_type()  → NumericStats
  ├─ DATE/TIMESTAMP/DATETIME/TIME/...                  → is_datetime_type() → DateTimeStats
  ├─ BOOLEAN                                           → 直接匹配          → BooleanStats
  ├─ VARCHAR/TEXT/...                                  → fallthrough       → TextStats
  └─ BLOB/JSON/ARRAY/...                               → 当前 fallthrough  → TextStats (待修复)
```

**设计决策：** 类型分派基于 DuckDB `typeof()` 返回值，而非源数据库类型。这是因为洞察计算全部在 DuckDB 临时表中完成，数据已经过导入转换。

**待修复：** BLOB/JSON/Array 类型应增加显式处理分支，避免 fallthrough 到 Text 导致的无效统计。

### 3.3 洞察引擎流程

```
get_column_insight_full(temp_table, column_name)
  │
  ├─ 1. get_column_stats_internal()
  │    ├─ COUNT(*), COUNT(col), COUNT(DISTINCT col)
  │    ├─ typeof(col) → data_type
  │    └─ 类型分派:
  │         ├─ Numeric   → compute_numeric_stats()    → 执行 "numeric-stats" 规则
  │         ├─ DateTime  → compute_datetime_stats()   → 执行 "datetime-range" + "datetime-monthly"
  │         ├─ Boolean   → compute_boolean_stats()    → 执行 "boolean-ratio"
  │         └─ Text      → compute_text_stats()       → 执行 "text-frequency" + "text-length"
  │
  ├─ 2. get_column_sample_internal()
  │    └─ SELECT value LIMIT 5（可配置）
  │
  └─ 3. get_column_histogram_internal()   [仅 Numeric]
       └─ 执行 "histogram" 规则
```

### 3.4 质量评分引擎

```
compute_column_quality(ColumnInsightFull) → QualityScore
  │
  ├─ completeness (权重 35%)
  │   = (1.0 - null_rate) * 100
  │
  ├─ uniqueness (权重 25%)
  │   = 基于 unique/non_null 比率的分段评分
  │     0.9+ → 100 | 0.5+ → 80 | 0.2+ → 60 | 0.05+ → 40 | 0.01+ → 20 | else → 10
  │
  ├─ type_consistency (权重 20%)
  │   = 按类型的启发式评分:
  │     Numeric:  null_rate > 0.5 → 40, else → 90
  │     Text:     unique < 2 → 30, null_rate > 0.6 → 40, else → 75
  │     DateTime: has histogram range → 85, else → 60
  │     Boolean:  true_ratio in 0.3-0.7 → 90, else → 70
  │
  └─ distribution (权重 20%)
     = 100 if histogram exists, else 50

整体评分 = Σ(dimension.score * dimension.weight / 100)
等级: Excellent(85+) Good(70+) Fair(50+) Poor(30+) Critical(<30)
```

**设计约束：** 权重应支持项目级或全局配置，当前硬编码在 `quality_scorer.rs` 中。

### 3.5 持久化策略

```
洞察快照存储
  │
  ├─ DuckDB (数据层)
  │   ├─ insight_snapshots (id, project_id, conn_id, table, column, snapshot_json, checksum, created_at)
  │   └─ TTL: 30分钟未访问 → 标记过期，下次 register 时懒惰清理
  │
  └─ SQLite (索引层)
      ├─ insight_version_index (version_id → DuckDB row_id 映射)
      └─ 版本链: parent_version_id → child_version_id
```

**版本对比流程：**

```
用户点击历史版本
  → load_version_detail(version_id)
  → 查询 SQLite 获取 DuckDB row_id
  → 从 DuckDB 读取 JSON snapshot
  → 反序列化为 ColumnInsightFull (diffData)
  → 前端 diffColumns 对比当前 insightData vs diffData
  → diffSummary 生成人类可读对比文本
```

---

## 四、规则系统设计

### 4.1 规则目录结构

```
src-tauri/insight-rules/           # 内置规则（include_dir! 编译时嵌入）
  ├── column/                      # 单列规则
  │   ├── null-check.rule.toml
  │   ├── numeric-basic.rule.toml
  │   ├── numeric-stats.rule.toml
  │   ├── histogram.rule.toml
  │   ├── text-frequency.rule.toml
  │   ├── text-length.rule.toml
  │   ├── datetime-range.rule.toml
  │   ├── datetime-monthly.rule.toml
  │   └── boolean-ratio.rule.toml
  ├── multi/                       # 多列规则
  │   ├── correlation.rule.toml
  │   ├── grouped-stats.rule.toml
  │   ├── cross-tab.rule.toml
  │   └── scatter-sample.rule.toml
  ├── table/                       # 表级规则
  │   ├── table-row-count.rule.toml
  │   ├── table-column-overview.rule.toml
  │   ├── table-null-overview.rule.toml
  │   └── table-quality-overview.rule.toml
  └── quality/                     # 质量门控规则
      └── column-quality-score.rule.toml

{project}/.RSMETA/insight-rules/   # 用户自定义规则（项目打开时加载）
  └── *.rule.toml                  # 同名覆盖内置规则
```

### 4.2 规则文件格式规范

```toml
# ===== [meta] 元数据段（必需）=====
[meta]
id = "numeric-stats"                 # 唯一标识，kebab-case
name = "数值统计"                     # 人类可读名称
category = "column"                  # column | multi | table | quality
applies_to = ["Numeric"]             # 适用数据类型列表
description = "计算数值列的描述性统计"
version = "1.0.0"

# ===== [query] SQL 模板段（必需）=====
[query]
template = """
SELECT
  MIN("{col}")::DOUBLE AS min,
  MAX("{col}")::DOUBLE AS max,
  AVG("{col}")::DOUBLE AS avg,
  MEDIAN("{col}")::DOUBLE AS median,
  STDDEV("{col}")::DOUBLE AS stddev
FROM "{table}"
WHERE "{col}" IS NOT NULL
"""
parameters = ["table", "col"]        # {table} 和 {col} 将被替换

# ===== [[output]] 输出映射段（必需，可多个）=====
[[output]]
sql_name = "min"                     # SQL 查询结果列名
json_name = "min"                    # JSON 输出键名
value_type = "f64"                   # f64 | i64 | str | bool | f64? | i64?

[[output]]
sql_name = "stddev"
json_name = "stddev"
value_type = "f64?"

# ===== [render] 渲染提示段（可选）=====
[render]
component = "bar"                    # bar | line | pie | scatter
display_order = 5                    # 显示优先级

# ===== [quality] 质量门控段（仅 quality 类规则）=====
[quality]
gate = "Transform"                   # Transform | Merge

[quality.default_grade]              # 默认等级标签
"100" = "excellent"
"70" = "good"
"40" = "fair"
"10" = "poor"
"0" = "critical"
```

### 4.3 规则生命周期

```
编译时                            运行时
  │                                │
  include_dir! 嵌入内置规则 ────────┤
                                   │
  App 启动 ──▶ OnceLock<RwLock<RuleRegistry>> 初始化
                                   │
  项目打开 ──▶ load_from_dir(.RSMETA/insight-rules/)
              │ 同名规则 → 用户优先（覆盖内置）
              │
              ▼
          RuleRegistry 就绪
              │
  用户请求洞察 ──▶ registry.get("rule-id") → RuleFile
              │
              ▼
          RuleExecutor::execute(rule, connection, params)
              │
              ├─ 1. 两阶段占位符替换:
              │      {col} → @col@  (避免子串匹配)
              │      @col@  → value (最终替换)
              │
              ├─ 2. 执行 SQL
              │
              ├─ 3. 按 [[output]] 映射构建 JSON
              │
              └─ 4. 返回 serde_json::Value
```

### 4.4 规则扩展规范

**新增一条列级规则：**

1. 在 `insight-rules/column/` 下创建 `{rule-id}.rule.toml`
2. 填写 `[meta]` / `[query]` / `[[output]]` 三要素
3. 如果希望前端图表自动选择，添加 `[render]` 段
4. 如果是质量门控规则，添加 `[quality]` 段
5. 无需修改任何 Rust 代码，重新构建即可生效

**新增一类规则目录（如 geo/）：**

1. 创建 `insight-rules/geo/` 目录
2. 添加规则文件
3. 在 `rule_registry.rs` 的 `load_from_embedded_dir` 中添加目录遍历

---

## 五、前端设计 (Vue 3 + TypeScript)

### 5.1 组件树

```
DockviewLayout.vue
  │
  ├─ LeftPrimarySidebar (数据库导航树)
  │   └─ 右键菜单 → "快速探查" → openTableProfile()
  │
  ├─ CenterArea
  │   ├─ TableProfileView.vue ← tableProfile 面板
  │   │   ├─ NDataTable (列元数据)
  │   │   ├─ NTag (dbType, rowCount)
  │   │   ├─ NButton "评估质量" → batch_evaluate_columns()
  │   │   ├─ QualityScoreCard (表级质量聚合)
  │   │   └─ 列名点击 → openColumnInsight()
  │   │
  │   ├─ SchemaInsightPanel.vue ← schemaInsight 面板
  │   │   ├─ NCollapse: 外键推断 / 类型不一致 / 孤立表 / 冗余列
  │   │   └─ NProgress: Schema 健康评分
  │   │
  │   └─ DataVisualizationPanel.vue ← dataVisualization 面板
  │       ├─ NSelect: 图表类型 (bar/line/pie/scatter)
  │       └─ ECharts 图表实例
  │
  └─ RightPrimarySidebar
      ├─ ColumnInsightsPanel.vue ← 快速统计 (~140行)
      │   └─ count/null/type/unique 四行摘要
      │
      └─ ColumnInsightPanel.vue ← 完整洞察 (~276行 orchestrator)
          └─ NTabs { default: 'column' }
              ├─ NTabPane "列洞察"
              │   ├─ [Empty / Loading(骨架屏) / Error / Data] 四状态
              │   ├─ QualityScoreCard.vue       (评分徽章+四维度进度条)
              │   ├─ InsightStatsSection.vue    (NCollapse: 统计/分布/质量/采样)
              │   ├─ NTag: 适用规则推荐
              │   └─ NButton: 保存快照 / 导出JSON / 导出Markdown
              │
              ├─ NTabPane "多列分析"
              │   └─ MultiColumnView.vue
              │       ├─ NSelect multiple: 列选择
              │       ├─ NSelect: 规则选择
              │       ├─ NButton "执行分析"
              │       └─ NDataTable / KV 对: 结果渲染
              │
              └─ NTabPane "历史"
                  └─ InsightHistoryTab.vue
                      ├─ [Empty / 版本列表] 两状态
                      ├─ 版本条目: version_id + checksum + 日期
                      └─ Diff 面板: 变更字段名 + diffSummary 文本
```

### 5.2 状态管理 (Pinia Store)

```typescript
// insight-store.ts
export const useInsightStore = defineStore('insight', () => {
  // ===== State =====
  const column = ref<string>('')
  const connId = ref<string>('')
  const tempTable = ref<string>('')
  const dbType = ref<string>('')

  // 核心数据
  const insightData = ref<ColumnInsightFull | null>(null)       // 当前洞察结果
  const qualityScore = ref<QualityScore | null>(null)            // 质量评分

  // 多列分析
  const allColumns = ref<string[]>([])
  const multiResult = ref<Record<string, unknown> | null>(null)
  const multiColumnRules = ref<RuleMeta[]>([])

  // 历史
  const historyVersions = ref<HistoryVersion[]>([])
  const diffData = ref<ColumnInsightFull | null>(null)

  // 可视化请求信号
  const pendingVisualizationRequest = ref<{
    data: Record<string, unknown>[]
    columns: string[]
    title?: string
    chartType?: string           // 来自 RenderHint
  } | null>(null)

  // ===== Getters (computed) =====
  const isLoading = computed(() => /* 加载状态 */)
  const hasData = computed(() => insightData.value !== null)
  const statsKind = computed<'Numeric' | 'Text' | 'DateTime' | 'Boolean' | 'Unknown'>(() =>
    insightData.value?.stats?.stats_detail?.kind ?? 'Unknown'
  )
  const diffColumns = computed<string[]>(() => /* 变更字段名列表 */)
  const diffSummary = computed<Record<string, string>>(() => /* 可读对比文本 */)

  // ===== Actions =====
  async function loadColumnFromTable(connId, dbType, database, table, column) { /* ... */ }
  async function loadColumnInsight(col, table, conn) { /* ... */ }
  async function saveSnapshot() { /* ... */ }
  async function loadHistory() { /* ... */ }
  async function loadVersionDetail(versionId) { /* ... */ }
  function clearDiff() { /* ... */ }
  async function executeMultiRule(selectedCols, rule) { /* ... */ }
  async function loadMultiRules() { /* ... */ }
  async function cleanupOldSnapshots() { /* ... */ }
  function clearVisualizationRequest() { /* ... */ }
})
```

### 5.3 前端类型定义

```typescript
// result-analysis.ts

// ===== 核心洞察类型 =====
interface ColumnInsightFull {
  stats: ColumnStats
  histogram: DistributionBin[] | null
  sample: Record<string, unknown>[]
}

interface ColumnStats {
  column_name: string
  data_type: string
  total_count: number
  null_count: number
  null_rate: number
  unique_count: number | null
  stats_detail: ColumnStatsDetail
}

// ===== 类型变体（tagged union） =====
type ColumnStatsDetail =
  | ({ kind: 'Numeric' } & NumericStatsDetail)
  | ({ kind: 'Text' } & TextStatsDetail)
  | ({ kind: 'DateTime' } & DateTimeStatsDetail)
  | ({ kind: 'Boolean' } & BooleanStatsDetail)

interface NumericStatsDetail {
  min: number
  max: number
  avg: number
  median: number
  p25: number
  p75: number
  stddev: number | null
  skewness: number | null
  is_extreme: ExtremeValue[]
}

interface TextStatsDetail {
  min_length: number
  max_length: number
  top_values: TextFrequency[]
}

interface DateTimeStatsDetail {
  earliest: string
  latest: string
  span_days: number
  monthly_distribution: MonthlyBin[]
}

interface BooleanStatsDetail {
  true_count: number
  false_count: number
  true_ratio: number
}

// ===== 质量评分 =====
interface QualityScore {
  overall_score: number
  grade: 'excellent' | 'good' | 'fair' | 'poor' | 'critical'
  dimensions: QualityDimension[]
}

// ===== 规则元数据 =====
interface RuleMeta {
  id: string
  name: string
  category: 'column' | 'multi' | 'table' | 'quality'
  applies_to: string[]
  description: string | null
  render: RenderHint | null
}

interface RenderHint {
  component: string | null // bar | line | pie | scatter
  display_order: number | null
}

// ===== Schema 洞察 =====
interface SchemaInsightReport {
  database: string
  schema: string
  table_count: number
  health_score: number
  foreign_keys: ForeignKeyCandidate[]
  type_mismatches: TypeMismatch[]
  orphan_tables: OrphanTable[]
  redundant_columns: RedundantColumn[]
}

// ===== 表探查 =====
interface TableProfile {
  table_name: string
  db_type: string
  row_count: number
  columns: TableColumnMeta[]
  quality_scores?: QualityScore[]
}
```

### 5.4 UI 状态机

```
┌──────────┐  loadColumnInsight  ┌──────────┐
│  EMPTY   │ ──────────────────▶ │ LOADING  │
│ (初始状态) │                    │ (骨架屏)  │
└──────────┘                    └────┬─────┘
                                     │
                       ┌─────────────┼─────────────┐
                       ▼             ▼             ▼
                 ┌──────────┐  ┌──────────┐  ┌──────────┐
                 │  DATA    │  │  EMPTY   │  │  ERROR   │
                 │ (洞察结果)│  │ (无数据)  │  │ (错误提示)│
                 └────┬─────┘  └──────────┘  └──────────┘
                      │
          ┌───────────┼────────────┐
          ▼           ▼            ▼
    保存快照       导出JSON      导出MD
    (→历史列表)     (← 下载)     (← 下载)
```

### 5.5 跨组件通信机制

| 通信方式     | 使用场景     | 示例                                               |
| ------------ | ------------ | -------------------------------------------------- |
| Pinia Store  | 洞察数据共享 | `insightStore.insightData`                         |
| CustomEvent  | 跨面板联动   | `open-column-insight` / `open-table-profile`       |
| Dockview API | 面板管理     | `api.addPanel()` / `api.focusPanel()`              |
| Store Signal | 异步请求     | `pendingVisualizationRequest` → watcher → addPanel |

---

## 六、API 协议设计

### 6.1 Tauri Command 清单

```
列洞察
  get_column_insight_full       → 完整列洞察（统计+直方图+采样）
  get_column_insights           → 仅统计信息（轻量级）
  profile_column_from_table     → 从源表取样→DuckDB→返回洞察（端到端一站式）

质量评分
  evaluate_quality_rule         → 执行质量门控规则
  batch_evaluate_columns        → 批处理评估全表所有列的 QualityScore

Schema 洞察
  get_schema_insight            → Schema 结构分析（FK/类型/孤立/冗余+健康评分）

持久化
  save_insight_snapshot         → 保存当前洞察为版本快照
  list_insight_versions         → 列出某列的所有历史版本
  get_insight_version_detail    → 获取特定版本的完整洞察数据
  cleanup_old_snapshots         → 手动清理旧版本

规则引擎
  list_insight_rules            → 列出所有可用规则（内置+用户）
  execute_insight_rule          → 手动执行指定规则（返回 JSON）

表探查
  get_table_profile             → 表元数据（列信息+行数）
```

### 6.2 请求/响应格式

```typescript
// 请求: get_column_insight_full
invoke('get_column_insight_full', {
  connId: 'mysql-1',
  tempTable: 'rs_query_20260508_001',
  columnName: 'amount',
})

// 响应: ColumnInsightFull
{
  "stats": {
    "column_name": "amount",
    "data_type": "DOUBLE",
    "total_count": 10000,
    "null_count": 150,
    "null_rate": 0.015,
    "unique_count": 8500,
    "stats_detail": {
      "kind": "Numeric",
      "min": 0.01,
      "max": 9999.99,
      "avg": 245.67,
      "median": 198.50,
      "p25": 45.00,
      "p75": 380.00,
      "stddev": 312.45,
      "skewness": 2.34,
      "is_extreme": [{ "value": 9999.99, "z_score": 31.2 }]
    }
  },
  "histogram": [
    { "label": "0-1000", "count": 7500, "ratio": 0.75 },
    { "label": "1000-2000", "count": 1500, "ratio": 0.15 },
    // ...
  ],
  "sample": [
    { "amount": 123.45, "row_index": 0 },
    { "amount": 678.90, "row_index": 1 },
    // ...最多 5 条
  ]
}

// 请求: get_schema_insight
invoke('get_schema_insight', {
  connId: 'pg-1',
  database: 'mydb',
  schemaName: 'public',
})

// 响应: SchemaInsightReport
{
  "database": "mydb",
  "schema": "public",
  "table_count": 42,
  "health_score": 78.5,
  "foreign_keys": [
    {
      "source_table": "orders",
      "source_column": "user_id",
      "target_table": "users",
      "target_column": "id",
      "pattern": "table_id",
      "confidence": 0.95
    }
  ],
  "type_mismatches": [
    {
      "column_name": "status",
      "tables": [
        { "table": "orders", "type": "VARCHAR" },
        { "table": "order_archive", "type": "INTEGER" }
      ]
    }
  ],
  "orphan_tables": [
    { "table": "deprecated_audit_log", "reason": "无外键关联" }
  ],
  "redundant_columns": [
    { "source": "users.email", "duplicate": "profiles.user_email", "similarity": 1.0 }
  ]
}
```

---

## 七、数据流设计

### 7.1 端到端数据流（列洞察）

```
Step 1: 用户点击查询结果表格列头
  │  QueryResultPanel 触发 CustomEvent('open-column-insight', { column, tempTable, connId })
  ▼
Step 2: DockviewLayout 事件监听
  │  handleColumnInsight() → insightStore.loadColumnFromTable(...)
  ▼
Step 3: Tauri Command
  │  invoke('get_column_insight_full', { connId, tempTable, columnName })
  ▼
Step 4: Service 层
  │  InsightEngine::get_column_insight_full()
  │  ├─ DuckDB typeof() → 类型检测
  │  ├─ 类型分派 → 选择统计函数
  │  └─ RuleExecutor::execute() → SQL 执行
  ▼
Step 5: DuckDB 计算
  │  AVG/MEDIAN/STDDEV/COUNT DISTINCT 等聚合
  │  返回 JSON 数组
  ▼
Step 6: 组装 ColumnInsightFull
  │  stats + histogram(Numeric only) + sample(LIMIT 5)
  ▼
Step 7: JSON 序列化 → Tauri IPC → 前端
  ▼
Step 8: insightStore.insightData = response
  │  qualityScorer.compute_column_quality() 自动计算
  ▼
Step 9: Vue 响应式更新
  │  ColumnInsightPanel 显示统计数据
  │  QualityScoreCard 显示评分徽章
  │  InsightStatsSection 显示折叠详情
```

### 7.2 RenderHint 图表自动选择流

```
TOML Rule [render] component = "pie"
  │
  ▼
RuleRegistry::parse() → RuleMeta.render = Some(RenderHint { component: "pie" })
  │
  ▼
list_insight_rules → 前端 ruleMeta.render.component = "pie"
  │
  ▼
ColumnInsightPanel.openVisualization()
  │  applicableRules[0].render?.component → chartType = "pie"
  │  insightStore.pendingVisualizationRequest = { chartType: "pie", data, columns }
  ▼
DockviewLayout.watcher → openVisualization(request)
  │  dockviewApi.addPanel({ params: { chartType: "pie", data, columns } })
  ▼
DataVisualizationPanel
  │  props.chartType → chartType ref 初始值 = "pie"
  │  ECharts 渲染饼图
```

### 7.3 表探查端到端流

```
导航树右键 "快速探查"
  │  CustomEvent('open-table-profile', { connId, dbType, database, schema, table })
  ▼
DockviewLayout → dockviewApi.addPanel({ component: 'tableProfile', params })
  │
  ├─ get_table_profile() → information_schema.columns + DuckDB COUNT(*)
  │   返回 TableProfile { columns, row_count }
  │
  └─ 可选: batch_evaluate_columns() → 全表质量评估
      返回 TableQuality { column_scores, table_score }

TableProfileView 渲染:
  ├─ NDataTable: 列元数据（列名/类型/可空/PK）
  ├─ NTag: dbType + rowCount
  ├─ 列名可点击 → openColumnInsight()
  └─ "评估质量" 按钮 → batch_evaluate_columns()
```

### 7.4 Schema 洞察流

```
Database 详情页 / 手动触发
  │
  ▼
invoke('get_schema_insight', { connId, database, schemaName })
  │
  ▼
SchemaAnalyzer::analyze()
  │
  ├─ 1. 查询 information_schema.tables → 表列表
  ├─ 2. 查询 information_schema.columns → 所有列元数据
  │
  ├─ 3. detect_foreign_keys() → 外键推断
  │     patterns: {table}_id, {table}_uuid, id_{table}, {table}_ref
  │
  ├─ 4. detect_type_mismatches() → 同名列类型不一致
  ├─ 5. detect_orphan_tables() → 无外键关联的表
  ├─ 6. detect_redundant_columns() → 不同表中内容相同的列
  │
  └─ 7. compute_health_score() → 加权评分
       FK覆盖(30) + 类型一致(25) + 关联度(20) + 表规范化(15) + 冗余度(-10)

SchemaInsightPanel 渲染:
  ├─ NProgress: 健康评分
  └─ NCollapse: 四类洞察折叠面板
```

---

## 八、类型系统设计

### 8.1 类型分类矩阵

```
DuckDB typeof()       →  洞察类型     →  适用规则
────────────────────────────────────────────────────
BIGINT / INTEGER      →  Numeric     →  numeric-basic, numeric-stats, histogram
SMALLINT / TINYINT    →  Numeric     →  (同上)
HUGEINT               →  Numeric     →  (同上) [待验证: HUGEINT 转 DOUBLE 精度损失]
DOUBLE / FLOAT / REAL →  Numeric     →  (同上)
DECIMAL / NUMERIC     →  Numeric     →  (同上)

VARCHAR / TEXT / CHAR →  Text        →  text-frequency, text-length, null-check
BLOB / BYTEA          →  [未处理]    →  [当前 fallthrough → Text, 待修复]
JSON / JSONB          →  [未处理]    →  [当前 fallthrough → Text, 待修复]

DATE                  →  DateTime    →  datetime-range, datetime-monthly
TIMESTAMP / DATETIME  →  DateTime    →  (同上)
TIME                  →  DateTime    →  (同上)
TIMESTAMPTZ           →  DateTime    →  (同上)

BOOLEAN / BOOL        →  Boolean     →  boolean-ratio, null-check

NULL (全空列)          →  [未处理]    →  [当前 typeof() 返回 NULL, 待修复]
INTERVAL              →  [未处理]    →  [当前 fallthrough → Text, 待修复]
ARRAY / LIST          →  [未处理]    →  [当前 fallthrough → Text, 待修复]
UUID                  →  [未处理]    →  [当前 fallthrough → Text, 可接受]
ENUM                  →  [未处理]    →  [当前 fallthrough → Text, 可接受]
```

### 8.2 类型扩展设计（待实现）

```rust
/// 类型分类器 trait（新增，替代 is_numeric_type / is_datetime_type）
trait TypeClassifier {
    /// 将原始 DB 类型映射到洞察类型
    fn classify(raw_type: &str) -> InsightDataType
}

enum InsightDataType {
    Numeric,           // 数值型
    Text,              // 文本型
    DateTime,          // 日期时间型
    Boolean,           // 布尔型
    Json,              // JSON 型（新增）
    Binary,            // 二进制型（新增）
    Array(Box<InsightDataType>), // 数组型（新增）
    Spatial,           // 空间型（新增，预留）
    Unknown,           // 未知（跳过统计或仅基础统计）
}
```

---

## 九、性能策略

### 9.1 分层缓存策略

```
L1: Pinia Store 缓存 (内存)
  │  insightData 引用，同项目同列不重复请求
  │  失效: 重新执行查询时清除
  │
L2: DuckDB 临时表缓存 (进程内)
  │  DuckDBManager 全局单例
  │  TTL: 30分钟 → evict_oldest_tables() 懒惰清理
  │  失效: 超时 / 连接断开
  │
L3: SQLite 快照缓存 (磁盘)
  │  历史版本持久化
  │  失效: 手动清理 / 超出版本数限制
```

### 9.2 大表优化策略

| 数据量级    | 策略                                                  |
| ----------- | ----------------------------------------------------- |
| < 10K 行    | 直接 DuckDB 全表计算                                  |
| 10K ~ 1M 行 | DuckDB 全表 + 采样统计（APPROX_QUANTILE）             |
| 1M ~ 10M 行 | 使用 DuckDB 分区并行 + 只计算关键统计                 |
| > 10M 行    | 采样分析（TABLESAMPLE） + 延迟加载 + 建议用户缩小范围 |

**DuckDB 优化：** 利用 DuckDB 的列式存储和向量化执行，避免多次全表扫描。在单个 `get_column_insight_full` 调用中尽量合并多个聚合到一个 SQL 中。

### 9.3 前端性能优化

- **虚拟滚动**：AG Grid 用于大结果集渲染
- **懒加载**：ColumnInsightPanel 的折叠面板默认收起，用户主动展开
- **骨架屏**：加载过程中显示脉冲动画，降低感知延迟
- **computed 拆分**：按职责拆分 computed（stats vs quality vs history），避免连锁重算
- **防抖**：多列分析的"执行分析"按钮加 debounce

---

## 十、扩展点设计

### 10.1 新增数据类型支持

以 JSON 类型为例，完整扩展步骤：

```
Step 1: Rust 端
  ├─ insight_engine.rs: 在 get_column_stats_internal 中添加 JSON 分支
  │   if is_json_type(&dt_lower) { compute_json_stats(...) }
  │
  ├─ duckdb_service.rs: 实现 compute_json_stats_internal()
  │   统计: 嵌套深度、顶层键集合、数组长度分布、null 键比例
  │
  ├─ result_service.rs: 新增 JsonStats 结构体
  │   struct JsonStats {
  │       max_depth: usize,
  │       top_keys: Vec<String>,
  │       avg_array_len: f64,
  │   }
  │
  └─ ColumnStatsDetail: 新增 JsonVariant

Step 2: 规则
  ├─ 创建 insight-rules/column/json-stats.rule.toml
  └─ 创建 insight-rules/column/json-keys.rule.toml

Step 3: 前端
  ├─ result-analysis.ts: 新增 JsonStatsDetail 类型 + ColumnStatsDetail 联合类型扩展
  ├─ InsightStatsSection.vue: 新增 statsKind === 'Json' 渲染分支
  └─ i18n: 新增 JSON 相关翻译键
```

### 10.2 新增洞察维度（如时间序列分析）

```
Step 1: 创建 insight-rules/column/ts-trend.rule.toml
  │  SQL: 按日/周/月分组，计算移动平均和趋势线
  │
Step 2: Rust 端 (如果 SQL 模板不够用)
  │  实现 compute_time_series_stats() → ColumnStatsDetail::TimeSeries(...)
  │
Step 3: 前端
  │  ColumnInsightPanel → 新增 "时序" Tab
  │  ECharts 时间序列折线图
```

### 10.3 新增前端可视化类型

```
DataVisualizationPanel 的 chartType 枚举:
  'bar' | 'line' | 'pie' | 'scatter'

扩展为:
  'bar' | 'line' | 'pie' | 'scatter' | 'heatmap' | 'boxplot' | 'treemap'

渲染方式:
  ECharts 配置对象工厂:
    createBarOption(data) → EChartsOption
    createLineOption(data) → EChartsOption
    ...
```

### 10.4 用户自定义质量门控

```
用户编写 quality 类规则:
  {project}/.RSMETA/insight-rules/my-quality-gate.rule.toml

  [quality]
  gate = "Transform"

  SQL 输出: { overall_score: 85, grade: "good", dimensions: [...] }

  → RuleExecutor::execute_qualified()
  → 覆盖 / 合并原有的 QualityScore
  → 前端自动应用新评分
```

---

## 十一、安全与错误处理

### 11.1 SQL 注入防护

**两阶段占位符替换**（已在 rule_executor.rs 实现）：

```
阶段1: {col} → @col@  (占位符转义，防止子串匹配)
阶段2: @col@  → "amount"  (最终值替换，加引号)

示例:
  SQL 模板: "SELECT {col} FROM {table}"
  参数: col="amount", table="rs_001"

  阶段1: "SELECT @col@ FROM @table@"
  阶段2: 'SELECT "amount" FROM "rs_001"'
```

**设计约束：** 参数值必须经过 DuckDB SQL 安全引用（双引号包裹标识符），禁止直接在参数中拼接用户输入。

### 11.2 错误处理链

```
RuleExecutor::execute() 失败
  → CoreError::common("Rule execution failed: {rule_id}")

InsightEngine::get_column_insight_full() 失败
  → CoreError::common("DuckDB stats failed for '{column}': {error}")

Tauri Command 失败
  → Tauri::Error → 前端 catch
  → insightStore.error = error_message
  → ColumnInsightPanel Error 状态渲染
```

**设计约束：**

- 禁止 `unwrap()` / `expect()` 在生产代码中
- 所有错误必须通过 `CoreError` 传递
- 错误信息不得暴露内部路径或连接凭据
- 前端使用统一的 Error 组件渲染错误状态

### 11.3 数据安全

- DuckDB 临时表仅包含用户主动查询的数据列
- 临时表名称包含时间戳随机后缀，不可预测
- 30 分钟 TTL 限制数据驻留时间
- 质量评分仅在前端展示，不自动上报
- 洞察快照存储在用户本地的项目 SQLite 文件中

---

## 十二、演进路线图

### 当前版本: v2.0 (已完成)

- ✅ 18 条内置规则覆盖 4 种数据类型
- ✅ 端到端列洞察 + 表探查 + Schema 洞察
- ✅ 四维度质量评分 + QualityRule 门控
- ✅ DuckDB 双库持久化 + TTL 淘汰
- ✅ RenderHint 图表自动选择
- ✅ 组件化拆分 (orchestrator + 子组件)

### v2.1 (下一阶段) — 类型覆盖补齐

| 任务                   | 优先级 | 预估影响                        |
| ---------------------- | :----: | ------------------------------- |
| BLOB 类型显式处理      |   P0   | 修复 fallthrough 到 Text 的 bug |
| 全 NULL 列类型检测增强 |   P0   | 边缘情况鲁棒性                  |
| JSON 列基础统计 MVP    |   P1   | 高频需求，Number 1 backlog item |
| Array 列元素分布统计   |   P2   | PostgreSQL 用户常用             |

### v2.2 — 分析能力增强

| 任务                         |       优先级       |
| ---------------------------- | :----------------: |
| 异常检测: IQR + Z-Score      |         P1         |
| 时序分析: 趋势 + 周期检测    |         P1         |
| ✅ 规则热加载 (文件监听)     | ✅ Phase 20 已完成 |
| 洞察结果缓存 (checksum 驱动) |         P1         |
| 质量评分权重可配置           |         P2         |
| 规则数量扩充 18→30+          |         P2         |

### v2.3 — 用户体验提升

| 任务              | 优先级 |
| ----------------- | :----: |
| PDF/HTML 报告导出 |   P2   |
| 洞察调度/定时任务 |   P2   |
| 质量告警/通知     |   P2   |
| 跨数据库对比分析  |   P3   |
| AI 建议引擎       |   P3   |

### v3.0 — 生态化

| 任务                                   | 优先级 |
| -------------------------------------- | :----: |
| 规则分享/导入/市场                     |   P3   |
| 数据血缘分析                           |   P3   |
| 跨平台洞察（MySQL→DuckDB→PG 联合分析） |   P3   |
| REST API 暴露洞察服务                  |   P3   |

---

## 附录 A：文档索引

| 文档   | 路径                       | 用途               |
| ------ | -------------------------- | ------------------ |
| 本文档 | `INSIGHT-DESIGN.md`        | 权威设计规范       |
| 架构   | `INSIGHT-ARCHITECTURE.md`  | 当前实现架构       |
| API    | `INSIGHT-API-REFERENCE.md` | 接口参考           |
| 规则   | `INSIGHT-RULE-FORMAT.md`   | 规则格式参考       |
| 进度   | `INSIGHT-DEV-PROGRESS.md`  | 开发进度跟踪       |
| 计划   | `INSIGHT-SYSTEM-PLAN.md`   | 原始规划（已归档） |

## 附录 B：关键决策记录

| 决策                       | 日期       | 原因                                          |
| -------------------------- | ---------- | --------------------------------------------- |
| DuckDB 作为唯一计算引擎    | 2026-05-07 | 统一分析层，避免各数据源 SQL 方言差异         |
| TOML 规则与 Rust 代码分离  | 2026-05-07 | 支持零编译扩展，用户可自定义规则              |
| DuckDB + SQLite 双库持久化 | 2026-05-07 | DuckDB 存数据、SQLite 存索引，职责分离        |
| 质量评分四维度固定权重     | 2026-05-08 | MVP 阶段简化，v2.2 支持可配置                 |
| 临时表 30 分钟 TTL         | 2026-05-08 | 平衡内存占用与复用效率                        |
| 组件 300 行拆分原则        | 2026-05-08 | 提取 QualityScoreCard/StatsSection/HistoryTab |
| RenderHint 图表自动选择    | 2026-05-08 | 规则作者声明最佳可视化，减少用户决策          |
