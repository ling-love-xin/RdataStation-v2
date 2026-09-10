# RdataStation 洞察模块 — API 接口参考

> 版本：v1.0
> 创建日期：2026-05-08
> 最后更新：2026-05-08
> 状态：✅ 完整

---

## 一、Tauri Commands（Rust → 前端）

所有命令通过 `tauri.invoke('<command>', args)` 调用。

### 1.1 列洞察

#### `get_column_insight_full`

获取单列的完整洞察数据（统计 + 采样 + 直方图）。

| 参数         | 类型     | 说明                         |
| ------------ | -------- | ---------------------------- |
| `tempTable`  | `string` | DuckDB 临时表名（rs\_ 前缀） |
| `columnName` | `string` | 列名                         |

**返回**: [ColumnInsightFull](#columninsightfull)

```typescript
const insight = await tauri.invoke('get_column_insight_full', {
  tempTable: 'rs_abc123',
  columnName: 'amount',
})
```

#### `get_column_insights`

获取单列统计信息（不含采样和直方图）。

| 参数         | 类型     | 说明            |
| ------------ | -------- | --------------- |
| `tempTable`  | `string` | DuckDB 临时表名 |
| `columnName` | `string` | 列名            |

**返回**: [ColumnStats](#columnstats)

---

### 1.2 规则引擎

#### `execute_insight_rule`

执行一条洞察规则。内部经过质量门控（QualityRule），返回 [ExecutionResult](#executionresult)。

| 参数         | 类型                     | 说明                          |
| ------------ | ------------------------ | ----------------------------- |
| `rule_id`    | `string`                 | 规则 ID，如 `numeric-stats`   |
| `params`     | `Record<string, string>` | 规则参数（如 `{table, col}`） |
| `temp_table` | `string`                 | DuckDB 临时表名               |

**返回**: [ExecutionResult](#executionresult)（序列化 JSON）

```typescript
const result = await tauri.invoke('execute_insight_rule', {
  ruleId: 'numeric-stats',
  params: { col: 'amount', table: 'rs_abc123' },
  tempTable: 'rs_abc123',
})
// result.data → 规则输出
// result.quality → QualityReport | null
```

#### `list_insight_rules`

列出所有可用规则。

| 参数       | 类型      | 说明                                               |
| ---------- | --------- | -------------------------------------------------- |
| `category` | `string?` | 可选过滤：`column` / `multi` / `table` / `quality` |

**返回**: `RuleMeta[]`

#### `list_rules_for_column`

列出适用于指定列类型的规则。

| 参数         | 类型     | 说明                                                |
| ------------ | -------- | --------------------------------------------------- |
| `columnType` | `string` | 列类型：`numeric` / `text` / `datetime` / `boolean` |

**返回**: `RuleMeta[]`

---

### 1.3 表探查

#### `get_table_profile`

获取表的列元信息和行数。

| 参数       | 类型     | 说明                                       |
| ---------- | -------- | ------------------------------------------ |
| `connId`   | `string` | 连接 ID                                    |
| `dbType`   | `string` | 数据库类型（mysql/postgres/sqlite/duckdb） |
| `database` | `string` | 数据库名                                   |
| `schema`   | `string` | Schema 名                                  |
| `table`    | `string` | 表名                                       |

**返回**: [TableProfile](#tableprofile)

#### `profile_column_from_table`

从源表取样 → DuckDB 临时表 → 洞察（端到端）。

| 参数       | 类型     | 说明      |
| ---------- | -------- | --------- |
| `connId`   | `string` | 连接 ID   |
| `database` | `string` | 数据库名  |
| `schema`   | `string` | Schema 名 |
| `table`    | `string` | 表名      |
| `column`   | `string` | 列名      |

**返回**: [ColumnInsightFull](#columninsightfull)

---

### 1.4 质量评分

#### `get_column_quality`

获取单列质量评分。

| 参数         | 类型     | 说明            |
| ------------ | -------- | --------------- |
| `columnName` | `string` | 列名            |
| `tempTable`  | `string` | DuckDB 临时表名 |

**返回**: [QualityScore](#qualityscore)

#### `batch_evaluate_columns`

批量评估表中所有列的质量。

| 参数       | 类型     | 说明      |
| ---------- | -------- | --------- |
| `conn_id`  | `string` | 连接 ID   |
| `database` | `string` | 数据库名  |
| `schema`   | `string` | Schema 名 |
| `table`    | `string` | 表名      |

**返回**: [TableQuality](#tablequality)

---

### 1.5 Schema 洞察

#### `get_schema_insight`

获取 Schema 级分析报告。

| 参数       | 类型     | 说明      |
| ---------- | -------- | --------- |
| `conn_id`  | `string` | 连接 ID   |
| `database` | `string` | 数据库名  |
| `schema`   | `string` | Schema 名 |

**返回**: [SchemaInsightReport](#schemainsightreport)

---

### 1.6 DuckDB 管理

#### `create_duckdb_temp_table`

将查询结果写入 DuckDB 内存临时表。

| 参数      | 类型        | 说明                          |
| --------- | ----------- | ----------------------------- |
| `columns` | `string[]`  | 列名列表                      |
| `rows`    | `Value[][]` | 数据行（JSON Value 二维数组） |

**返回**: `string`（临时表名，如 `rs_a1b2c3d4`）

#### `get_duckdb_pool_info`

获取 DuckDB 连接池信息。

**返回**: `{ poolSize, preferredSize, minSize, maxSize }`

#### `set_duckdb_pool_size`

设置 DuckDB 连接池偏好大小（1-32）。

| 参数      | 类型       | 说明                         |
| --------- | ---------- | ---------------------------- |
| `size`    | `number`   | 连接池大小                   |
| `restart` | `boolean?` | 是否立即重启池（清空临时表） |

#### `restart_duckdb_pool`

重建 DuckDB 连接池并清空所有临时表。

---

## 二、前端 API 函数（TypeScript）

定义位置: [result-analysis.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/services/result-analysis.ts)

```typescript
// 列洞察
getColumnInsightFull(tempTable: string, columnName: string): Promise<ColumnInsightFull>
getColumnInsights(tempTable: string, columnName: string): Promise<ColumnStats>

// 规则引擎
executeInsightRule(input: { ruleId, params, tempTable }): Promise<MultiRuleResult>
listInsightRules(category?: string): Promise<RuleMeta[]>
listRulesForColumn(columnType: string): Promise<RuleMeta[]>

// 表探查
getTableProfile(input: { connId, dbType, database, schema, table }): Promise<TableProfile>
profileColumnFromTable(input: { connId, database, schema, table, column }): Promise<ColumnInsightFull>

// 质量评分
getColumnQuality(columnName: string, tempTable: string): Promise<QualityScore>
batchEvaluateColumns(input: { connId, database, schema, table }): Promise<TableQuality>

// Schema 洞察
getSchemaInsight(input: { connId, database, schema }): Promise<SchemaInsightReport>

// DuckDB 管理
createDuckDbTempTable(columns: string[], rows: unknown[][]): Promise<string>
```

---

## 三、前端 Store（Pinia）

定义位置: [insight-store.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/stores/insight-store.ts)

### State

| 字段          | 类型                        | 说明             |
| ------------- | --------------------------- | ---------------- |
| `insightData` | `ColumnInsightFull \| null` | 当前列洞察数据   |
| `isLoading`   | `boolean`                   | 加载中标志       |
| `error`       | `string \| null`            | 错误信息         |
| `tempTable`   | `string \| null`            | 当前临时表名     |
| `column`      | `string \| null`            | 当前列名         |
| `isOpen`      | `boolean`                   | 洞察面板是否打开 |

### 质量评分 State

| 字段           | 类型                   | 说明           |
| -------------- | ---------------------- | -------------- |
| `qualityScore` | `QualityScore \| null` | 当前列质量评分 |
| `tableQuality` | `TableQuality \| null` | 当前表质量聚合 |

### Schema State

| 字段              | 类型                          | 说明            |
| ----------------- | ----------------------------- | --------------- |
| `schemaInsight`   | `SchemaInsightReport \| null` | Schema 分析报告 |
| `isSchemaLoading` | `boolean`                     | Schema 加载中   |

### 多列分析 State

| 字段               | 类型                      | 说明         |
| ------------------ | ------------------------- | ------------ |
| `multiResult`      | `MultiRuleResult \| null` | 多列分析结果 |
| `multiColumnRules` | `RuleMeta[]`              | 可用多列规则 |

### 历史对比 State

| 字段              | 类型                              | 说明            |
| ----------------- | --------------------------------- | --------------- |
| `historyVersions` | `VersionEntry[]`                  | 历史版本列表    |
| `diffVersionId`   | `string \| null`                  | 当前对比版本 ID |
| `diffData`        | `Record<string, unknown> \| null` | 对比差异数据    |

### Actions

```typescript
loadColumnInsight(tempTable: string, column: string): Promise<void>
loadColumnFromTable(input: { connId, database, schema, table, column }): Promise<void>
loadQualityScore(): Promise<void>
loadTableQuality(input: { connId, database, schema, table }): Promise<void>
loadSchemaInsight(input: { connId, database, schema }): Promise<void>
loadMultiRules(): Promise<void>
executeMultiRule(ruleId: string, columns: string[]): Promise<void>
loadHistory(): Promise<void>
loadVersionDetail(versionId: string): Promise<void>
clearDiff(): void
closeInsight(): void
filterByValue(value: string): Promise<void>
```

---

## 四、数据类型参考

### ColumnInsightFull

```typescript
interface ColumnInsightFull {
  stats: ColumnStats
  sample: Value[] // 最多 5 条采样值
  histogram: DistributionBin[] | null
}
```

### ColumnStats

```typescript
interface ColumnStats {
  column_name: string
  data_type: string // DuckDB typeof() 结果
  total_count: number
  null_count: number
  null_rate: number // 0.0 ~ 1.0
  unique_count: number | null
  stats_detail: ColumnStatsDetail
}
```

### ColumnStatsDetail（判别联合）

```typescript
type ColumnStatsDetail =
  | { Numeric: NumericStats }
  | { Text: TextStats }
  | { DateTime: DateTimeStats }
  | { Boolean: BooleanStats }
  | { Unknown: null }
```

### NumericStats

```typescript
interface NumericStats {
  min: number
  max: number
  avg: number
  median: number
  p25: number
  p75: number
  sum: number
  stddev: number | null
  skewness: number | null
  kurtosis: number | null
  is_extreme: ExtremeValue[]
}
```

### TextStats

```typescript
interface TextStats {
  min_length: number
  max_length: number
  top_values: TextFrequency[]
}

interface TextFrequency {
  value: string
  count: number
  ratio: number
}
```

### DateTimeStats

```typescript
interface DateTimeStats {
  earliest: string
  latest: string
  span_days: number
  monthly_distribution: TextFrequency[]
}
```

### BooleanStats

```typescript
interface BooleanStats {
  true_count: number
  false_count: number
  true_ratio: number
}
```

### QualityScore

```typescript
interface QualityScore {
  column_name: string
  overall_score: number // 0 ~ 100
  level: string // 优秀/良好/一般/较差/差
  dimensions: QualityDimension[]
  summary: string
}

interface QualityDimension {
  name: string // 完整性/唯一性/类型一致/分布均匀
  score: number // 0 ~ 100
  weight: number // 权重（0.35/0.25/0.20/0.20）
  detail: string
}
```

### TableQuality

```typescript
interface TableQuality {
  table_name: string
  overall_score: number
  level: string
  column_scores: ColumnQualityEntry[]
  summary: string
  scored_count: number
  total_columns: number
}

interface ColumnQualityEntry {
  column_name: string
  quality_score: number
  level: string
  null_rate: number
}
```

### SchemaInsightReport

```typescript
interface SchemaInsightReport {
  database: string
  schema: string
  table_count: number
  column_count: number
  foreign_keys: ForeignKeyCandidate[]
  type_mismatches: TypeMismatchEntry[]
  orphan_tables: OrphanTable[]
  redundant_columns: RedundantColumn[]
  health_score: number
  health_level: string
  health_summary: string
}
```

### ExecutionResult

```typescript
interface ExecutionResult {
  data: Record<string, unknown> // 规则执行输出
  quality: QualityReport | null // 质量门控结果
}

interface QualityReport {
  passed: boolean
  checks: QualityCheck[]
}

interface QualityCheck {
  field: string
  passed: boolean
  rule: string // 如 ">= 0", "<= 100"
  actual: number | null
  severity: string // "error" | "warning" | "info"
  message: string
}
```

### TableProfile

```typescript
interface TableProfile {
  table_name: string
  schema: string
  database: string
  db_type: string
  columns: TableColumnMeta[]
  row_count: number | null
}

interface TableColumnMeta {
  column_name: string
  data_type: string
  is_nullable: string
  column_key: string // PRI / MUL / "" ...
  ordinal_position: number
}
```

---

## 五、错误处理

所有 Tauri 命令返回标准错误格式：

```typescript
try {
  const result = await tauri.invoke('get_column_insight_full', args)
} catch (e) {
  // e 为 string，格式: "错误描述信息"
  console.error('[insight]', e)
}
```

Store 中的 actions 在 catch 块记录 `console.error` 并设置 `error` state：

```typescript
catch {
  error.value = '加载洞察数据失败'
  console.error('[insightStore] loadColumnInsight failed')
}
```
