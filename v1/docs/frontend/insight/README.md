# RdataStation 洞察体系 — 技术架构文档

> 版本：v19.0
> 创建日期：2026-05-07
> 最后更新：2026-05-09
> 状态：✅ 完整架构（DuckDB 实例统一 + 端到端联动 + 动画 + 质量评分 + 表聚合 + Schema 洞察 + QualityRule 质量门控 + DuckDB TTL + API 参考 + RenderHint 图表自动选择 + 组件类型安全强化 + 动态类型分类 + 可配置常量 + Phase 20 归档修复 + Phase 21 全局主题 + Phase 22 全栈审计修复）

---

## 一、数据流（含多列分析）

```
用户点击列头
  │  CustomEvent 'open-column-insight' { column, tempTable, allColumns }
  ▼
ColumnInsightPanel.vue
  │
  ├─ [Tab: 列洞察] → invoke('get_column_insight_full')
  │                   └─ RuleExecutor (column/*.toml)
  │
  └─ [Tab: 多列分析] → MultiColumnView.vue
       ├─ listInsightRules('multi') → 获取多列规则
       ├─ 用户选择列 + 规则
       └─ executeInsightRule({ rule_id, params, temp_table })
            └─ RuleExecutor (multi/*.toml)
```

### 表探查数据流

```
导航树 → 右键"快速探查"
  │  CustomEvent('open-table-profile') { connId, dbType, database, schema, table }
  ▼
DockviewLayout.handleTableProfile()
  │  api.addPanel({ component: 'tableProfile', params })
  ▼
TableProfileView.vue
  │  invoke('get_table_profile') → information_schema.columns + COUNT(*)
  │  → TableProfile { columns, row_count, db_type }
  └─ 点击列名 → CustomEvent('table-column-click') ← NEW
```

### 表列探查联动（端到端）← NEW

```
TableProfileView → 点击列名
  │  CustomEvent('table-column-click') { column, table, database, schema, connId }
  ▼
ColumnInsightPanel.handleTableColumnClick()
  │  insightStore.loadColumnFromTable({ connId, database, schema, table, column })
  ▼
invoke('profile_column_from_table')
  │  ResultService::profile_column_from_table()
  ├─ SqlService.execute() → SELECT * LIMIT 500 (取样)
  ├─ create_duckdb_temp_table() → DuckDB temp table
  └─ get_column_insight_full() → ColumnInsightFull (洞察)
      ▼
  洞察面板展示（NCollapse/basic/dist/quality/sample）
```

---

## 二、规则引擎模块

### 全局注册表（可变）

```rust
// insight/mod.rs
static GLOBAL_REGISTRY: OnceLock<RwLock<RuleRegistry>> = OnceLock::new();

pub fn global_registry() -> &'static RwLock<RuleRegistry> { ... }
pub fn load_user_rules(project_path: &Path) { ... }  // 项目打开时调用
```

### 内置规则 (18 条)

| 目录       | ID                                                                                                                                | 类型     |
| ---------- | --------------------------------------------------------------------------------------------------------------------------------- | -------- |
| `column/`  | null-check, numeric-stats, numeric-basic, text-frequency, text-length, datetime-range, datetime-monthly, boolean-ratio, histogram | 单列     |
| `multi/`   | correlation, grouped-stats, cross-tab, scatter-sample                                                                             | 多列     |
| `table/`   | table-row-count, table-column-overview, table-null-overview, table-quality-overview                                               | 表级     |
| `quality/` | column-quality-score                                                                                                              | 质量门控 |

### Tauri Commands

| 命令                         | 输入                                          | 输出                                |
| ---------------------------- | --------------------------------------------- | ----------------------------------- |
| `get_column_insight_full`    | `{ connId, tempTable, columnName }`           | `ColumnInsightFull` (完整列洞察)    |
| `get_column_insights`        | `{ connId, tempTable, columnName }`           | `ColumnStats` (轻量统计)            |
| `execute_insight_rule`       | `{ rule_id, params, temp_table }`             | `Value` (动态JSON)                  |
| `list_insight_rules`         | `category?`                                   | `RuleMeta[]`                        |
| `list_rules_for_column`      | `{ column_type }`                             | `RuleMeta[]`                        |
| `reload_insight_rules`       | `{}`                                          | `bool`                              |
| `get_table_profile`          | `{ connId, dbType, database, schema, table }` | `TableProfile` (表探查)             |
| `profile_column_from_table`  | `{ connId, database, schema, table, column }` | `ColumnInsightFull` (表列探查)      |
| `evaluate_quality_rule`      | `{ rule_id, temp_table }`                     | `QualityReport` (质量门控)          |
| `batch_evaluate_columns`     | `{ conn_id, database, schema, table }`        | `TableQuality` (表级质量)           |
| `get_schema_insight`         | `{ conn_id, database, schema }`               | `SchemaInsightReport` (Schema 洞察) |
| `save_insight_snapshot`      | `{ connId, table, column, data }`             | `String` (version_id)               |
| `list_insight_versions`      | `{ connId, table, column }`                   | `VersionInfo[]`                     |
| `get_insight_version_detail` | `{ version_id }`                              | `ColumnInsightFull` (历史版本)      |
| `cleanup_old_snapshots`      | `{ connId, table, column }`                   | `u32` (清理数量)                    |

---

## 三、前端组件树

```
ColumnInsightPanel.vue (~276 行 orchestrator) ← Phase 17 拆分
└─ NTabs { default: 'column' }
    ├─ NTabPane name="column" tab="列洞察"
    │   ├─ [Empty/Loading(骨架屏)/Error/Data] 四状态
    │   ├─ HeaderActions: Download(导出JSON) + FileText(导出Markdown) + Save(保存快照)
    │   ├─ QualityScoreCard.vue → 评分徽章(分数+等级+颜色) + 四维度进度条 ← extracted
    │   ├─ InsightStatsSection.vue (NCollapse: basic/dist/quality/sample) ← extracted
    │   │   ├─ NCollapseItem basic → 基础统计 (count/null/unique/min/max/mean/median/mode)
    │   │   ├─ NCollapseItem dist → 直方图/分布图 (按数据类型)
    │   │   ├─ NCollapseItem quality → 质量评分维度明细
    │   │   └─ NCollapseItem sample → 样本值 Top 10
    │   ├─ StorageFooter → 版本数 + "清理旧数据"按钮 + columnRules 推荐标签
    │   └─ uses: insightStore.qualityScore (自动加载)
    │
    ├─ NTabPane name="multi" tab="多列分析"
    │   └─ MultiColumnView.vue → 使用 insightStore (不再直接调用 API)
    │       ├─ NSelect multiple → 列选择
    │       ├─ NSelect → 规则选择 (insightStore.multiColumnRules)
    │       ├─ NButton "执行分析" → insightStore.executeMultiRule()
    │       └─ NDataTable / KV 对 → 结果渲染 (insightStore.multiResult)
    │
    └─ NTabPane name="history" tab="历史"
        └─ InsightHistoryTab.vue ← extracted
            ├─ [Empty/List] 两状态
            ├─ 版本列表 → entry.version_id + checksum + 日期
            ├─ onClick → insightStore.loadVersionDetail(version_id) → 加载 diffData
            ├─ DiffPanel → 旧值 → 新值（绿增/红减/灰不变）
            └─ 取消对比 → clearDiff()

TableProfileView.vue (dockview 'tableProfile')
├─ props: { connId, dbType, database, schema, table }
├─ [Loading / Error / Data] 三状态
├─ 导航树右键 → CustomEvent('open-table-profile') → DockviewLayout 动态创建
├─ NDataTable → 列元数据表 (列名/类型/可空/PK)
├─ NTag → dbType + RowCount
└─ 去重: 同表点击聚焦已有面板
```

### MultiColumnView 渲染

| 规则 result_type | 渲染                  |
| :--------------: | --------------------- |
|     `single`     | KV 行 (`密钥: 值` 对) |
|      `list`      | `NDataTable` 虚拟表格 |

---

## 四、用户自定义规则

```
{project}/.RSMETA/insight-rules/my-rule.rule.toml
```

加载时机：`open_project_by_path` → `insight::load_user_rules(&path_buf)`

冲突策略：同名 `rule_id` — 后加载的（用户）覆盖先加载的（内置）

---

## 五、文件清单

### 新增 (28)

```
src-tauri/insight-rules/column/ (9)
src-tauri/insight-rules/multi/ (4)
src-tauri/insight-rules/table/ (4) ← Phase 2 + 表质量
src-tauri/insight-rules/quality/ (1) ← 质量评分
src-tauri/src/core/duckdb.rs ← DuckDB 全局单例
src-tauri/src/core/insight/mod.rs
src-tauri/src/core/insight/rule_registry.rs
src-tauri/src/core/insight/rule_executor.rs
src-tauri/src/core/insight/rule_types.rs
src-tauri/src/core/insight/schema_analyzer.rs ← Schema 洞察
src-tauri/src/core/services/insight_engine.rs ← 洞察引擎 (Phase 6)
src-tauri/src/core/services/quality_scorer.rs ← 质量评分器 (Phase 10)
src-tauri/src/core/services/duckdb_service.rs ← DuckDB 服务 (Phase 14)
src/.../components/panels/MultiColumnView.vue
src/.../components/panels/TableProfileView.vue
src/.../components/panels/SchemaInsightPanel.vue ← Schema 洞察
src/.../components/panels/DataVisualizationPanel.vue ← 数据可视化
src/.../components/panels/ColumnInsightsPanel.vue ← 快速统计
src/.../components/panels/insight/QualityScoreCard.vue ← Phase 17 提取
src/.../components/panels/insight/InsightStatsSection.vue ← Phase 17 提取
src/.../components/panels/insight/InsightHistoryTab.vue ← Phase 17 提取
src/.../stores/insight-store.ts ← Pinia Store
src/.../services/result-analysis.ts ← 前端 API 层
src/.../composables/use-context-menu-actions.ts ← 快速探查菜单
```

### 修改 (23 → 多轮迭代)

```
src-tauri/Cargo.toml (添加 toml)
src-tauri/src/core/mod.rs (添加 insight + duckdb + re-export)
src-tauri/src/core/services/result_service.rs (compute_* + 规则API + TableProfile + profile_column_from_table + DuckDBManager委托)
src-tauri/src/commands/result_commands.rs (15个 insight 命令 + TableProfile + 版本详情 + 表列探查 + reload)
src-tauri/src/commands/project_commands.rs (load_user_rules)
src-tauri/src/lib.rs (注册 23 个 insight 命令)
src-tauri/src/core/persistence/mod.rs (ResourceVersion 导出修复)
src-tauri/src/core/dbi/engine/duckdb_engine.rs (DuckDB 实例统一: tokio Mutex → std Mutex)
src/.../components/panels/ColumnInsightPanel.vue (NTabs + MultiColumnView + P0打通 + 骨架屏 + 历史/对比/导出 + 过渡动画 + table-column-click)
src/.../components/panels/ColumnInsightsPanel.vue (快速统计 + 主题统一)
src/.../components/panels/DataVisualizationPanel.vue (ECharts 图表 + RenderHint 自动选择 + 主题统一)
src/.../components/panels/QueryResultPanel.vue (allColumns in event)
src/.../services/result-analysis.ts (RuleMeta + TableProfile + API 全部 + reloadInsightRules)
src/.../stores/insight-store.ts (多列 state + P0打通 + 版本对比 + 表列探查 + isOpen移除)
src/.../components/panels/MultiColumnView.vue (P0打通: 改用 Store + 主题统一)
src/.../components/DockviewLayout.vue (注册 tableProfile + dataVisualization + 动态创建面板)
src/.../composables/use-context-menu-actions.ts (快速探查菜单 + getDbTypeForConnection)
src/shared/styles/tokens.css (新增品牌色/字体/间距/圆角令牌 Phase 21)
src/.../types/result.ts (MultiRuleResult收紧 + 死类型清理 Phase 22)
```

---

## 六、质量门控管线（Phase 13）

```
RuleFile (.rule.toml)
  │  [[quality]] 定义
  ▼
RuleExecutor::execute_qualified(rule, conn, params)
  ├── execute(rule, conn, params) → Value (原始结果)
  └── evaluate_quality(rule, &data) → QualityReport
        │  逐个检查 quality[].field vs quality[].rule
        │  生成 QualityCheck { field, passed, rule, actual, severity, message }
        ▼
ExecutionResult { data: Value, quality: Option<QualityReport> }
  │  insight_engine → result_service → result_commands
  ▼
serde_json::to_value(exec_result) → JSON → 前端
```

## 七、DuckDB 临时表生命周期（Phase 14）

```
register_temp_table("rs_xxx")
  ├── ① 注册到 table_registry: HashMap<String, Instant>
  ├── ② evict_oldest_tables()         # 计数淘汰: > 50 个 → 删最旧
  └── ③ cleanup_expired_tables()      # TTL 淘汰: > 30 分钟 → DROP TABLE
                                         TEMP_TABLE_TTL_SECS = 1800
```

## 八、RenderHint 图表自动选择管线（Phase 17）

```
RuleFile (.rule.toml)
  │  [rule.render] 定义
  │  component = "pie"   # 建议图表类型
  │  display_order = 5
  ▼
RuleRegistry::parse() → RuleMeta { render: Some(RenderHint { component: "pie", .. }) }
  │
  ▼
RuleExecutor::rule_to_json() → JSON { "render": { "component": "pie" } }
  │  insight_engine → result_service → result_commands
  ▼
前端 ColumnInsightPanel.openVisualization()
  ├─ applicableRules[0].render.component → chartType
  └─ insightStore.pendingVisualizationRequest = { chartType: "pie", ... }
      │
      ▼
DockviewLayout watcher → openVisualization(request)
  │  params.chartType = detail.chartType
  ▼
DataVisualizationPanel
  │  props.chartType → chartType ref 初始值
  │  (仅在首次打开时生效，用户可切换)
```

### 支持图表类型

| TOML component | 前端 chartType | 图表类型 |
| :------------: | :------------: | -------- |
|    `"bar"`     |    `'bar'`     | 柱状图   |
|    `"line"`    |    `'line'`    | 折线图   |
|    `"pie"`     |    `'pie'`     | 饼图     |
|  `"scatter"`   |  `'scatter'`   | 散点图   |

### 规则示例

```toml
[rule]
id = "monthly-distribution"
name = "月度分布"
type = "distribution"
description = "按月聚合数据分布"

[rule.render]
component = "bar"
display_order = 5
```

## 九、历史对比数据流（Phase 18 修复）

```
insightStore.loadVersionDetail(version_id)
  │  invoke('get_insight_version_detail')
  ▼
diffData = ColumnInsightFull（历史快照）
  │
  ▼
diffColumns computed: string[]  ← 变更字段名列表
  │  比较 insightData.stats vs diffData.stats
  │  如: ["total_count", "null_count", "null_rate"]
  ▼
diffSummary computed: Record<string, string>  ← 可读对比文本
  │  如: { total_count: "1000 → 1500", null_count: "5 → 10 (+5)" }
  ▼
InsightHistoryTab.vue
  │  v-for colName in diffColumns
  │  <span>{{ diffSummary[colName] }}</span>
  │  （所有 diff 条目均为 val-changed 样式）
```

## 十、相关文档索引

| 文档                                                                             | 说明                                     |
| -------------------------------------------------------------------------------- | ---------------------------------------- |
| [INSIGHT-DEV-PROGRESS.md](./INSIGHT-DEV-PROGRESS.md)                             | 开发进度跟踪、变更日志                   |
| [INSIGHT-API-REFERENCE.md](./INSIGHT-API-REFERENCE.md)                           | Tauri Commands + 前端 API + 数据类型参考 |
| [INSIGHT-RULE-FORMAT.md](./INSIGHT-RULE-FORMAT.md)                               | 规则文件 TOML 格式规范                   |
| [INSIGHT-SYSTEM-PLAN.md](./INSIGHT-SYSTEM-PLAN.md)                               | 洞察系统总体规划                         |
| [QUERY-RESULT-OPTIMIZATION-PROGRESS.md](./QUERY-RESULT-OPTIMIZATION-PROGRESS.md) | 查询结果优化（交叉影响追踪）             |
