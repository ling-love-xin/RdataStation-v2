# RdataStation 洞察模块 — 规则文件格式规范

> 版本：v1.0
> 创建日期：2026-05-08
> 最后更新：2026-05-08
> 状态：✅ 完整

---

## 一、文件格式

规则文件使用 **TOML** 格式，扩展名 `.rule.toml`。

### 加载路径

| 类型     | 路径                                             | 加载时机                     |
| -------- | ------------------------------------------------ | ---------------------------- |
| 内置规则 | `src-tauri/insight-rules/{category}/*.rule.toml` | 首次调用 `global_registry()` |
| 用户规则 | `{project}/.RSMETA/insight-rules/*.rule.toml`    | 项目打开时                   |

### 冲突策略

同名 `rule_id` → 后加载的（用户规则）覆盖先加载的（内置规则）。

---

## 二、完整字段定义

```toml
# ─── 元数据 ──────────────────────────────
[meta]
id          = "numeric-stats"       # 唯一标识符（snake_case）
name        = "数值统计"             # 显示名称
description = "计算 min/max/avg/median/p25/p75"  # 描述
version     = "1.0"                 # 语义版本号
category    = "column"              # 分类: column | multi | table | quality
applies_to  = "numeric"             # 适用列类型: numeric | text | datetime | boolean | any
builtin     = true                  # 是否为内置规则

# ─── 查询定义 ──────────────────────────────
[query]
sql         = """                    # SQL 模板，支持参数替换
    SELECT
      MIN("{col}") as min_val,
      MAX("{col}") as max_val,
      AVG("{col}") as avg_val
    FROM "{table}"
    WHERE "{col}" IS NOT NULL
    """
result_type = "single"              # single | list
parameters  = ["table", "col"]      # 参数名列表

# ─── 输出字段映射 ──────────────────────────
[[output]]
sql_name    = "min_val"             # SQL 返回的列名
json_name   = "min"                 # JSON 输出的键名
value_type  = "f64"                 # 值类型（见类型表）

[[output]]
sql_name    = "max_val"
json_name   = "max"
value_type  = "f64"

# ─── 质量门控（可选）───────────────────────
[[quality]]
field       = "min"                 # 检查的输出字段
rule        = ">= 0"                # 约束表达式: ">= 0", "<= 100", "> 0.0"
severity    = "error"               # error | warning | info

# ─── 渲染提示（可选）────────────────────────
[render]
component     = "bar-chart"         # 推荐前端组件
display_order = 1                   # 显示排序
```

---

## 三、字段详解

### 3.1 `[meta]` — 元数据

| 字段          | 类型      | 必填 | 说明                                                |
| ------------- | --------- | :--: | --------------------------------------------------- |
| `id`          | `string`  |  ✅  | 唯一标识符，snake_case，用于规则查询                |
| `name`        | `string`  |  ✅  | 人类可读名称                                        |
| `description` | `string`  |  ✅  | 功能描述                                            |
| `version`     | `string`  |  ✅  | 语义版本号（`x.y`）                                 |
| `category`    | `string`  |  ✅  | `column` / `multi` / `table` / `quality`            |
| `applies_to`  | `string`  |  ✅  | `numeric` / `text` / `datetime` / `boolean` / `any` |
| `builtin`     | `boolean` |  ✅  | 内置规则为 `true`，用户规则为 `false`               |

### 3.2 `[query]` — 查询定义

| 字段          | 类型       | 必填 | 说明                                              |
| ------------- | ---------- | :--: | ------------------------------------------------- |
| `sql`         | `string`   |  ✅  | DuckDB SQL 模板                                   |
| `result_type` | `string`   |  ✅  | `single`（单行结果对象）或 `list`（多行结果数组） |
| `parameters`  | `string[]` |  ✅  | 参数列表，前端和 SQL 模板共享                     |

### 3.3 `[[output]]` — 输出字段映射（可多个）

| 字段         | 类型     | 必填 | 说明                       |
| ------------ | -------- | :--: | -------------------------- |
| `sql_name`   | `string` |  ✅  | SQL `SELECT` 中的别名/列名 |
| `json_name`  | `string` |  ✅  | 输出 JSON 对象的键名       |
| `value_type` | `string` |  ✅  | 值类型（见下表）           |

### 3.4 值类型表

| value_type            | Rust 类型        | 说明                  |
| --------------------- | ---------------- | --------------------- |
| `f64`                 | `f64`            | 浮点数（非空）        |
| `f64?`                | `Option<f64>`    | 浮点数（可空）        |
| `i64`                 | `i64`            | 整数（非空）          |
| `i64?`                | `Option<i64>`    | 整数（可空）          |
| `String` / `string`   | `String`         | 字符串（非空）        |
| `String?` / `string?` | `Option<String>` | 字符串（可空）        |
| `bool`                | `bool`           | 布尔值（非空）        |
| `bool?`               | `Option<bool>`   | 布尔值（可空）        |
| `usize`               | `i64 → usize`    | 通过 i64 中转的 usize |

### 3.5 `[[quality]]` — 质量门控（可选，可多个）

| 字段       | 类型     | 必填 | 说明                                     |
| ---------- | -------- | :--: | ---------------------------------------- |
| `field`    | `string` |  ✅  | 检查的 `json_name`                       |
| `rule`     | `string` |  ✅  | 约束表达式，如 `>= 0`, `<= 100`, `> 0.0` |
| `severity` | `string` |  ✅  | `error` / `warning` / `info`             |

执行规则后，`evaluate_quality()` 会逐个检查 `quality` 条目：

- 提取 `field` 对应的输出值 → 转为 `f64`
- 检查 `rule` 约束 → 生成 `QualityCheck { passed, field, rule, actual, severity, message }`
- 所有检查通过 → `QualityReport.passed = true`

### 3.6 `[render]` — 渲染提示（可选）

| 字段            | 类型      | 说明                                                 |
| --------------- | --------- | ---------------------------------------------------- |
| `component`     | `string?` | 推荐前端组件名，如 `bar-chart`, `pie-chart`, `table` |
| `display_order` | `number?` | 在结果面板中的排序位置                               |

> 注意：RenderHint 当前仅定义为类型，前端尚未消费。预留用于未来的自动可视化匹配。

---

## 四、SQL 模板语法

### 参数替换

使用 `{param_name}` 引用 `parameters` 中定义的参数：

```toml
[query]
parameters = ["table", "col"]
sql = """
  SELECT COUNT(*) as cnt
  FROM "{table}"
  WHERE "{col}" IS NOT NULL
  """
```

执行时传入 `{ table: "rs_abc123", col: "amount" }` → 生成：

```sql
SELECT COUNT(*) as cnt
FROM "rs_abc123"
WHERE "amount" IS NOT NULL
```

### 替换安全性

替换采用两步法防止子串匹配：

1. 将所有 `{param}` → 临时标记 `@param@`
2. 将 `@param@` → 实际值

这确保 `{col}` 和 `{col_name}` 等相似参数不会互相误匹配。

### 表名/列名规范

- 表名和列名 **必须用双引号包裹**：`"{table}"`, `"{col}"`
- 这是 DuckDB 的标识符规范

### SQL 沙箱限制

- ✅ 允许：`SELECT`, `CREATE TABLE`（仅 `rs_` 前缀）、`DROP TABLE`（仅 `rs_` 前缀）
- ❌ 禁止：`ATTACH`, `DETACH`, `INSTALL`, `LOAD`, `EXPORT DATABASE`
- ❌ 禁止：非 `rs_` 前缀的 `CREATE TABLE` / `DROP TABLE`

---

## 五、完整示例

### 示例 1：单列数值统计（single 结果）

```toml
[meta]
id          = "numeric-stats"
name        = "数值统计"
description = "计算 min/max/avg/median/p25/p75/sum/stddev"
version     = "1.0"
category    = "column"
applies_to  = "numeric"
builtin     = true

[query]
sql = """
    SELECT
      MIN("{col}") as min_val,
      MAX("{col}") as max_val,
      AVG("{col}") as avg_val,
      MEDIAN("{col}") as median_val,
      QUANTILE_CONT("{col}", 0.25) as p25_val,
      QUANTILE_CONT("{col}", 0.75) as p75_val,
      SUM("{col}") as sum_val,
      STDDEV("{col}") as stddev_val,
      SKEWNESS("{col}") as skewness_val,
      KURTOSIS("{col}") as kurtosis_val
    FROM "{table}"
    WHERE "{col}" IS NOT NULL
    """
result_type = "single"
parameters  = ["table", "col"]

[[output]]
sql_name    = "min_val"
json_name   = "min"
value_type  = "f64"

[[output]]
sql_name    = "max_val"
json_name   = "max"
value_type  = "f64"

[[output]]
sql_name    = "avg_val"
json_name   = "avg"
value_type  = "f64"

[[output]]
sql_name    = "median_val"
json_name   = "median"
value_type  = "f64"

[[output]]
sql_name    = "stddev_val"
json_name   = "stddev"
value_type  = "f64?"
```

### 示例 2：文本频率（list 结果）

```toml
[meta]
id          = "text-frequency"
name        = "文本频率统计"
description = "计算文本列 Top 10 值和频次"
version     = "1.0"
category    = "column"
applies_to  = "text"
builtin     = true

[query]
sql = """
    SELECT
      "{col}" as value,
      COUNT(*) as count,
      ROUND(COUNT(*) * 1.0 / (SELECT COUNT(*) FROM "{table}" WHERE "{col}" IS NOT NULL), 4) as ratio
    FROM "{table}"
    WHERE "{col}" IS NOT NULL
    GROUP BY "{col}"
    ORDER BY count DESC
    LIMIT 10
    """
result_type = "list"
parameters  = ["table", "col"]

[[output]]
sql_name    = "value"
json_name   = "value"
value_type  = "string"

[[output]]
sql_name    = "count"
json_name   = "count"
value_type  = "i64"

[[output]]
sql_name    = "ratio"
json_name   = "ratio"
value_type  = "f64"
```

### 示例 3：多列相关性（multi）

```toml
[meta]
id          = "correlation"
name        = "Pearson 相关系数"
description = "计算两列之间的 Pearson 相关系数"
version     = "1.0"
category    = "multi"
applies_to  = "numeric"
builtin     = true

[query]
sql = """
    SELECT
      CORR("{col1}", "{col2}") as coefficient
    FROM "{table}"
    WHERE "{col1}" IS NOT NULL AND "{col2}" IS NOT NULL
    """
result_type = "single"
parameters  = ["table", "col1", "col2"]

[[output]]
sql_name    = "coefficient"
json_name   = "coefficient"
value_type  = "f64?"

[[quality]]
field       = "coefficient"
rule        = ">= -1.0"
severity    = "error"

[[quality]]
field       = "coefficient"
rule        = "<= 1.0"
severity    = "error"
```

---

## 六、目录组织

```
insight-rules/
├── column/           # 单列分析规则
│   ├── null-check.rule.toml
│   ├── numeric-stats.rule.toml
│   ├── numeric-basic.rule.toml
│   ├── text-frequency.rule.toml
│   ├── text-length.rule.toml
│   ├── datetime-range.rule.toml
│   ├── datetime-monthly.rule.toml
│   ├── boolean-ratio.rule.toml
│   └── histogram.rule.toml
├── multi/            # 多列分析规则
│   ├── correlation.rule.toml
│   ├── grouped-stats.rule.toml
│   ├── cross-tab.rule.toml
│   └── scatter-sample.rule.toml
├── table/            # 表级规则
│   ├── table-row-count.rule.toml
│   ├── table-column-overview.rule.toml
│   ├── table-size-estimate.rule.toml
│   └── table-quality-overview.rule.toml
└── quality/          # 质量评分规则
    └── column-quality-score.rule.toml
```

---

## 七、规则生命周期

```
规则文件 (.rule.toml)
  │
  ▼ Registry 加载
rule_registry::load_from_embedded_dir()
  │  parse_toml() → RuleFile struct
  │  失败 → tracing::warn! + 跳过
  ▼
RwLock<RuleRegistry>
  │  HashMap<String, RuleFile>  (by id)
  │  Vec<String> sorted_ids
  ▼
RuleExecutor::execute(rule, conn, params)
  │  validate()  → 检查参数完整性
  │  build_sql()  → 参数替换 → SQL
  │  execute_single() / execute_list()
  │    → extract_field_value() → JSON Value
  ▼
RuleExecutor::execute_qualified()
  │  execute() → Value
  │  evaluate_quality() → QualityReport
  ▼
ExecutionResult { data, quality }
```
