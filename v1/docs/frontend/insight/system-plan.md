# RdataStation 洞察体系 — 实施总体规划

> 版本：v11.0
> 创建日期：2026-05-07
> 最后更新：2026-05-07
> 状态：✅ 全阶段完成（含端到端表列探查联动 + 过渡动画 + 质量评分 + 表级质量聚合 + Schema 洞察）

---

## 一、文档索引

| 文档                   | 路径                                    | 版本  |
| ---------------------- | --------------------------------------- | :---: |
| 实施总体规划（本文档） | `docs/frontend/INSIGHT-SYSTEM-PLAN.md`  | v11.0 |
| 技术架构文档           | `docs/frontend/INSIGHT-ARCHITECTURE.md` | v11.0 |
| 开发进度跟踪           | `docs/frontend/INSIGHT-DEV-PROGRESS.md` | v11.0 |

---

## 二、洞察体系全景

```
第一级：列洞察 ✅ → 右侧面板 NTabs { 列洞察 | 多列分析 }
第二级：表探查 ✅ → 中央标签页 + 导航树右键
第三级：Schema 洞察 ⏳ → 规则引擎 + 质量评分
```

### 核心设计原则

> **SQL 定义是数据，Rust 是执行引擎，TOML 是合约**

---

## 三、Phase 划分

### Phase 1：列洞察 MVP + 持久化 + 规则引擎 ✅

- 13 个内置 TOML 规则（`insight-rules/`，`include_dir!` 嵌入）
- `insight/` 模块：`RuleRegistry` + `RuleExecutor` + `OnceLock<RwLock<>>`
- DuckDB SQL 模板全部与 Rust 代码分离
- DuckDB + SQLite 双库持久化 + 版本链 + 三级存储防护
- 前端 `ColumnInsightPanel.vue` 四栏折叠 + Save/存储底栏
- 3 个规则引擎 Tauri Commands

### Phase 1.5：多列分析 + 用户自定义规则 ✅

- 前端 `ColumnInsightPanel` → `NTabs`：列洞察 / 多列分析
- 新增 `MultiColumnView.vue`：列选择器 + 规则列表 + 执行 + 结果渲染
- 4 条多列规则：correlation / grouped-stats / cross-tab / scatter-sample
- `open_project_by_path` 自动扫描 `{project}/.RSMETA/insight-rules/`
- 用户自定义规则即插即用（同名覆盖内置）
- **P0打通修复**: MultiColumnView 改用 insightStore, Store 新增 executeMultiRule/loadMultiRules/cleanupOldSnapshots, 清理按钮 + 适用规则推荐标签

### Phase 2：表探查 (Table Profiling) ✅

- 后端 `TableProfile` struct：`table_name` / `db_type` / `columns: Vec<TableColumnMeta>` / `row_count`
- 查询 `information_schema.columns` 获取列元数据（列名、类型、可空性、主键）
- 3 条 table-level TOML 规则：`table-row-count` / `table-column-overview` / `table-null-overview`
- 前端 `TableProfileView.vue`：NDataTable 列元数据表格 + NTag 行数/dbType
- 导航树表节点右键 → "快速探查" → `CustomEvent('open-table-profile')` → DockviewLayout 动态创建面板
- 去重机制：同表重复点击聚焦已有面板
- 骨架屏优化：ColumnInsightPanel 加载中显示 6 行脉冲动画

### 优化增强 ✅

- **版本历史对比**：ColumnInsightPanel 新增"历史"Tab
  - 版本列表（date + checksum）点击加载完整快照数据
  - Diff 面板：旧值 → 新值，绿增/红减/灰不变颜色
  - 后端 `get_insight_version_detail` 从 DuckDB 取单个版本 `ColumnInsightFull`
- **导出功能**：
  - JSON 导出：Download 按钮 → `insight-{col}-{date}.json` 文件下载
  - Markdown 导出：`exportMarkdown()` → 表格格式 `.md` 文件下载
- **面板联动**：
  - TableProfileView 列名可点击（蓝色 + cursor:pointer）
  - `table-column-click` CustomEvent → `handleTableColumnClick` → `loadColumnFromTable`
  - 后端 `profile_column_from_table` 合并命令（取样→DuckDB→洞察）
  - 端到端链路：TableProfile → SqlService取样 → DuckDB temp → ColumnInsightFull → 面板
- **过渡动画**：
  - NTabs `tab-fade-in` 0.18s（opacity 0.6→1 + translateY 3px→0）

> **已完成**：DuckDB 实例统一（`DuckDBManager` 全局单例，ResultService 与 DuckDBEngine 共用同一实例）✅

### Phase 3：Schema 洞察 + 质量评分 ✅ 100%

- **质量评分 ✅**：四维度加权评分（完整性/唯一性/类型一致/分布均匀）+ 前端徽章 + 进度条
- **表级质量聚合 ✅**：`batch_evaluate_columns` 一次调用全表评估 + TableProfileView 增强
- **Schema 洞察 ✅**：外键推断（4 种命名模式）、跨表类型一致性、孤立表检测、冗余列检测 + Schema 健康评分
- **DuckDB 实例统一 ✅**：`DuckDBManager` 全局单例，`ResultService` + `DuckDBEngine` 共用同一实例

---

## 四、规则引擎架构

```
insight-rules/*.toml ──include_dir!──▶ 编译时嵌入 ──▶ OnceLock<RwLock<RuleRegistry>>
                                                          │
{project}/.RSMETA/insight-rules/ ──load_from_dir()──▶     │  (项目打开时)
                                                          ▼
                                                  RuleExecutor::execute()
                                                          │
                                                  DuckDB SQL → JSON
```

### 规则文件格式

```toml
[meta]
id = "correlation"; name = "Pearson 相关系数"
category = "multi"; applies_to = ["Numeric", "Numeric"]

[query]
template = """SELECT CORR("{col1}", "{col2}") ..."""
parameters = ["table", "col1", "col2"]

[[output]]
sql_name = "corr"; json_name = "correlation"; value_type = "f64?"
```

### 用户自定义规则流程

1. 在项目 `.RSMETA/insight-rules/` 下创建 `*.rule.toml`
2. 重新打开项目（`open_project_by_path` 自动扫描）
3. `list_insight_rules` 包含自定义规则
4. `MultiColumnView` 下拉自动出现

---

## 五、关键决策一览

| 决策                   | 说明                              |
| ---------------------- | --------------------------------- |
| SQL 与 Rust 分离       | `.rule.toml` 独立文件，零编译扩展 |
| `RwLock<RuleRegistry>` | 支持运行时加载用户规则            |
| 同名覆盖               | 用户规则与内置同名 → 用户优先     |
| 多列分析 UI            | NTabs 切换，共用右侧面板          |
| DuckDB 实例暂不统一    | Phase 2                           |
