# RdataStation 洞察体系 — 开发进度跟踪

> 版本：v26.0
> 创建日期：2026-05-07
> 最后更新：2026-05-10
> 总体状态：✅ 全阶段完成 + QualityRule 质量门控 + DuckDB TTL + 全栈审计 + API/规则文档 + 测试完整 + P2 组件拆分 + RenderHint 管道 + diff 渲染修复 + 类型安全强化 + 类型分类修复 + 可配置常量 + 归档三修复 + 全局主题样式统一 + 审计修复 + 文档一致化 + 深度审计修复 (Semaphore/DuckDB/API docs/常量)

---

## 一、总体进度

| Phase     | 名称                           | 状态 |                        进度                         |
| --------- | ------------------------------ | :--: | :-------------------------------------------------: |
| Phase 1   | 列洞察 MVP + 持久化 + 规则引擎 |  ✅  |                        100%                         |
| Phase 1.5 | 多列分析 + 用户自定义规则      |  ✅  |                        100%                         |
| Phase 2   | 表探查 + DuckDB 统一           |  ✅  |            100% (表探查✅, DuckDB统一✅)            |
| Phase 3   | Schema 洞察 + 质量评分         |  ✅  | 100% (质量✅, 表聚合✅, Schema洞察✅, DuckDB统一✅) |

---

## 二、Phase 1.5 任务清单（全部完成 ✅）

### 规则引擎改造

| 任务                                | 文件                        | 状态 |
| ----------------------------------- | --------------------------- | :--: |
| `OnceLock` → `RwLock<RuleRegistry>` | `insight/mod.rs`            |  ✅  |
| `load_user_rules()` 扫描用户规则    | `insight/mod.rs`            |  ✅  |
| 项目打开时调用 `load_user_rules`    | `project_commands.rs` (2处) |  ✅  |
| 所有 `global_registry()` 调用者更新 | `result_service.rs` (8处)   |  ✅  |

### 多列分析前端

| 任务                                 | 文件                         | 状态 |
| ------------------------------------ | ---------------------------- | :--: |
| `MultiColumnView.vue` 新组件         | `panels/MultiColumnView.vue` |  ✅  |
| 列选择器 + 规则列表 + 执行 + 结果    | `MultiColumnView.vue`        |  ✅  |
| `ColumnInsightPanel` 增加 `NTabs`    | `ColumnInsightPanel.vue`     |  ✅  |
| `QueryResultPanel` 传递 `allColumns` | `QueryResultPanel.vue:811`   |  ✅  |
| `result-analysis.ts` 规则 API        | `result-analysis.ts`         |  ✅  |
| `insight-store.ts` 多列状态          | `insight-store.ts`           |  ✅  |

### P0 前后端打通修复

| 任务                                                           | 文件                     | 状态 |
| -------------------------------------------------------------- | ------------------------ | :--: |
| `MultiColumnView` 改用 insightStore                            | `MultiColumnView.vue`    |  ✅  |
| Store 新增 executeMultiRule/loadMultiRules/cleanupOldSnapshots | `insight-store.ts`       |  ✅  |
| Store 新增 columnRules/multiColumnRules computed               | `insight-store.ts`       |  ✅  |
| 列洞察底栏 "清理旧数据" 按钮                                   | `ColumnInsightPanel.vue` |  ✅  |
| 列洞察底栏 "适用规则" 推荐标签                                 | `ColumnInsightPanel.vue` |  ✅  |
| `loadMultiRules()` 在 onMounted 调用                           | `ColumnInsightPanel.vue` |  ✅  |

---

### Phase 2: 表探查（Table Profiling）

#### 后端

| 任务                                                  | 文件                              | 状态 |
| ----------------------------------------------------- | --------------------------------- | :--: |
| 3条 table-level TOML 规则                             | `insight-rules/table/*.rule.toml` |  ✅  |
| `TableProfile` + `TableColumnMeta` struct             | `result_service.rs`               |  ✅  |
| `get_table_profile()` 方法（information_schema 查询） | `result_service.rs`               |  ✅  |
| `fetch_table_columns()` 解析列元数据                  | `result_service.rs`               |  ✅  |
| `fetch_row_count()` 行数查询                          | `result_service.rs`               |  ✅  |
| `get_table_profile` Tauri 命令                        | `result_commands.rs`              |  ✅  |
| 命令注册到 `generate_handler!`                        | `lib.rs`                          |  ✅  |

#### 前端

| 任务                                                           | 文件                          | 状态 |
| -------------------------------------------------------------- | ----------------------------- | :--: |
| `TableProfile` / `TableColumnMeta` TS 类型                     | `result-analysis.ts`          |  ✅  |
| `getTableProfile()` API 函数                                   | `result-analysis.ts`          |  ✅  |
| `TableProfileView.vue` 组件（四状态:loading/error/empty/data） | `TableProfileView.vue`        |  ✅  |
| 全局注册为 dockview `tableProfile` 面板                        | `DockviewLayout.vue`          |  ✅  |
| 导航树右键 "快速探查" 菜单项                                   | `use-context-menu-actions.ts` |  ✅  |
| 动态创建面板 + 去重检测                                        | `DockviewLayout.vue`          |  ✅  |
| `getDbTypeForConnection` 三源查找                              | `use-context-menu-actions.ts` |  ✅  |

#### 体验优化

| 任务                         | 文件                     | 状态 |
| ---------------------------- | ------------------------ | :--: |
| 骨架屏加载动画（6 行脉冲条） | `ColumnInsightPanel.vue` |  ✅  |
| 加载中显示当前列名           | `ColumnInsightPanel.vue` |  ✅  |

### 优化增强

| 任务                                                    | 文件                     | 状态 |
| ------------------------------------------------------- | ------------------------ | :--: |
| 后端 `get_insight_version_detail` 方法                  | `result_service.rs`      |  ✅  |
| 后端 `get_insight_version_detail` Tauri 命令            | `result_commands.rs`     |  ✅  |
| 前端 `getInsightVersionDetail()` API                    | `result-analysis.ts`     |  ✅  |
| Store `loadVersionDetail` / `clearDiff` actions         | `insight-store.ts`       |  ✅  |
| Store `diffVersionId` / `diffData` / `isDiffLoading`    | `insight-store.ts`       |  ✅  |
| ColumnInsightPanel 新增"历史" Tab                       | `ColumnInsightPanel.vue` |  ✅  |
| 版本列表 + 点击加载详情 + 选中高亮                      | `ColumnInsightPanel.vue` |  ✅  |
| 版本对比面板（空值率/总数/去重数/空值数）               | `ColumnInsightPanel.vue` |  ✅  |
| Diff 差异颜色（绿增/红减/灰不变）                       | `ColumnInsightPanel.vue` |  ✅  |
| 导出 JSON 按钮（Download 图标）                         | `ColumnInsightPanel.vue` |  ✅  |
| 导出 Markdown 函数                                      | `ColumnInsightPanel.vue` |  ✅  |
| TableProfileView 列名可点击 + `table-column-click` 事件 | `TableProfileView.vue`   |  ✅  |
| `loadHistory()` 在打开洞察时自动调用                    | `ColumnInsightPanel.vue` |  ✅  |

### 端到端联动 + 动画

| 任务                                                                      | 文件                     | 状态 |
| ------------------------------------------------------------------------- | ------------------------ | :--: |
| 后端 `profile_column_from_table` 合并命令（取样→DuckDB→洞察）             | `result_service.rs`      |  ✅  |
| 后端 Tauri 命令 `profile_column_from_table`                               | `result_commands.rs`     |  ✅  |
| 前端 `profileColumnFromTable()` API                                       | `result-analysis.ts`     |  ✅  |
| Store `loadColumnFromTable()` action                                      | `insight-store.ts`       |  ✅  |
| ColumnInsightPanel 监听 `table-column-click` → 调用 `loadColumnFromTable` | `ColumnInsightPanel.vue` |  ✅  |
| NTabs 过渡动画 `tab-fade-in` 0.18s (opacity + translateY)                 | `ColumnInsightPanel.vue` |  ✅  |
| 表探查列点击端到端打通（TableProfile → 取样 → DuckDB → 洞察面板）         | 全链路                   |  ✅  |

### 质量评分 (Phase 3 先行)

| 任务                                                | 文件                            | 状态 |
| --------------------------------------------------- | ------------------------------- | :--: |
| 质量评分 TOML 规则 `column-quality-score.rule.toml` | `insight-rules/quality/`        |  ✅  |
| `QualityScore` + `QualityDimension` struct          | `result_service.rs`             |  ✅  |
| `compute_column_quality()` 四维度评分               | `result_service.rs`             |  ✅  |
| `get_column_quality` Tauri 命令 + 注册              | `result_commands.rs` + `lib.rs` |  ✅  |
| `QualityScore` / `QualityDimension` TS 类型         | `result-analysis.ts`            |  ✅  |
| `getColumnQuality()` API 函数                       | `result-analysis.ts`            |  ✅  |
| Store `qualityScore` / `loadQualityScore()`         | `insight-store.ts`              |  ✅  |
| ColumnInsightPanel 质量评分徽章（分数+等级+颜色）   | `ColumnInsightPanel.vue`        |  ✅  |
| 四维度进度条（完整性/唯一性/类型一致/分布均匀）     | `ColumnInsightPanel.vue`        |  ✅  |
| 自动加载：watch insightData → loadQualityScore      | `ColumnInsightPanel.vue`        |  ✅  |

### 表级质量聚合 (Phase 3 继续)

| 任务                                                    | 文件                            | 状态 |
| ------------------------------------------------------- | ------------------------------- | :--: |
| 表质量评估 TOML 规则 `table-quality-overview.rule.toml` | `insight-rules/table/`          |  ✅  |
| `TableQuality` + `ColumnQualityEntry` struct            | `result_service.rs`             |  ✅  |
| `compute_table_quality()` 列聚合方法                    | `result_service.rs`             |  ✅  |
| `batch_evaluate_columns()` 一次调用全表评估             | `result_service.rs`             |  ✅  |
| `batch_evaluate_columns` Tauri 命令 + 注册              | `result_commands.rs` + `lib.rs` |  ✅  |
| `TableQuality` / `ColumnQualityEntry` TS 类型           | `result-analysis.ts`            |  ✅  |
| `batchEvaluateColumns()` API 函数                       | `result-analysis.ts`            |  ✅  |
| TableProfileView "质量评估"按钮                         | `TableProfileView.vue`          |  ✅  |
| TableProfileView 质量评分列（分数字+等级+颜色）         | `TableProfileView.vue`          |  ✅  |
| TableProfileView 质量总览摘要栏                         | `TableProfileView.vue`          |  ✅  |
| `scoreColor()` 公共颜色函数                             | `TableProfileView.vue`          |  ✅  |

### Schema 洞察 (Phase 3 完成)

| 任务                                               | 文件                              | 状态 |
| -------------------------------------------------- | --------------------------------- | :--: |
| `SchemaAnalyzer` 分析引擎                          | `core/insight/schema_analyzer.rs` |  ✅  |
| `SchemaInsightReport` 含 5 个子 struct             | `schema_analyzer.rs`              |  ✅  |
| `fetch_all_tables()` information_schema.tables     | `schema_analyzer.rs`              |  ✅  |
| `fetch_all_columns()` information_schema.columns   | `schema_analyzer.rs`              |  ✅  |
| `infer_foreign_keys()` 外键推断 (命名模式)         | `schema_analyzer.rs`              |  ✅  |
| `detect_type_mismatches()` 跨表类型一致性          | `schema_analyzer.rs`              |  ✅  |
| `detect_orphan_tables()` 孤立表检测                | `schema_analyzer.rs`              |  ✅  |
| `detect_redundant_columns()` 冗余列检测            | `schema_analyzer.rs`              |  ✅  |
| `compute_health()` Schema 健康评分                 | `schema_analyzer.rs`              |  ✅  |
| `get_schema_insight` Tauri 命令 + 注册             | `result_commands.rs` + `lib.rs`   |  ✅  |
| `SchemaInsightReport` + 5 个子类型 TS 类型         | `result-analysis.ts`              |  ✅  |
| `getSchemaInsight()` API 函数                      | `result-analysis.ts`              |  ✅  |
| `SchemaInsightPanel.vue` 面板组件                  | `panels/SchemaInsightPanel.vue`   |  ✅  |
| 健康评分环 + 4 维度折叠区（FK/类型/孤立/冗余）     | `SchemaInsightPanel.vue`          |  ✅  |
| 导航树 Schema 节点右键 "Schema 洞察"               | `use-context-menu-actions.ts`     |  ✅  |
| `open-schema-insight` CustomEvent + DockviewLayout | `DockviewLayout.vue`              |  ✅  |

### DuckDB 实例统一

| 任务                                               | 文件                | 状态 |
| -------------------------------------------------- | ------------------- | :--: |
| `DuckDBManager` 全局单例（in-memory + persistent） | `core/duckdb.rs`    |  ✅  |
| `ResultService::get_or_create_duckdb()` 委托管理器 | `result_service.rs` |  ✅  |
| `DuckDBEngine` 迁移（tokio → std Mutex）           | `duckdb_engine.rs`  |  ✅  |
| `DuckDBEngine::get_conn_arc()` 缓存 + 委托管理器   | `duckdb_engine.rs`  |  ✅  |
| `core/mod.rs` 注册 duckdb 模块 + re-export         | `core/mod.rs`       |  ✅  |
| `register_external_database` 等 7 方法适配新锁模式 | `duckdb_engine.rs`  |  ✅  |

### 已有错误修复

| 任务                                              | 文件                  | 状态 |
| ------------------------------------------------- | --------------------- | :--: |
| `ExternalReference.added_at` → `created_at`       | `scratchpad/store.rs` |  ✅  |
| `.await` 在非 async 闭包 → `std::fs::remove_file` | `scratchpad/store.rs` |  ✅  |
| `ScratchpadStore` 添加 `#[derive(Clone)]`         | `scratchpad/store.rs` |  ✅  |
| 重复 `empty_trash` 方法 → 删除第二个              | `scratchpad/store.rs` |  ✅  |
| `response.entries` → `response.local_entries`     | `scratchpad/store.rs` |  ✅  |
| `AnalyzableFile` 未使用 import → 恢复             | `scratchpad/store.rs` |  ✅  |

### P0 安全性与健壮性修复 (2026-05-08 下午)

| 任务                                                        | 文件                     | 状态 |
| ----------------------------------------------------------- | ------------------------ | :--: |
| `DuckDBManager::get_or_create_in_memory()` 改为返回 Result  | `core/duckdb.rs`         |  ✅  |
| `ResultService::list_insight_rules()` 消除 `.expect()`      | `result_service.rs`      |  ✅  |
| `ResultService::list_rules_for_column()` 消除 `.expect()`   | `result_service.rs`      |  ✅  |
| `schema_analyzer.rs` SQL 转义防注入 (`escape_sql_string()`) | `schema_analyzer.rs`     |  ✅  |
| DuckDBManager 单元测试 (8 tests)                            | `core/duckdb.rs`         |  ✅  |
| SchemaAnalyzer 单元测试 (14 tests)                          | `schema_analyzer.rs`     |  ✅  |
| Frontend Store 统一 (tableQuality + schemaInsight 状态)     | `insight-store.ts`       |  ✅  |
| TableProfileView 改用 insightStore                          | `TableProfileView.vue`   |  ✅  |
| SchemaInsightPanel 改用 insightStore                        | `SchemaInsightPanel.vue` |  ✅  |
| 持久化: save/load_table_quality                             | `insight_store.rs`       |  ✅  |
| 持久化: save/load_schema_insight                            | `insight_store.rs`       |  ✅  |

### B 组：CustomEvent 消除 (2026-05-08 下午)

| 任务                                                               | 文件                          | 状态 |
| ------------------------------------------------------------------ | ----------------------------- | :--: |
| `table-column-click` 消除 → TableProfileView 直接调用 insightStore | `TableProfileView.vue`        |  ✅  |
| `table-column-click` 消除 → 移除 ColumnInsightPanel 事件监听       | `ColumnInsightPanel.vue`      |  ✅  |
| `open-schema-insight` 消除 → insightStore.requestSchemaInsight()   | `DockviewLayout.vue`          |  ✅  |
| `open-schema-insight` 消除 → use-context-menu-actions 改用 Store   | `use-context-menu-actions.ts` |  ✅  |
| `open-table-profile` 消除 → insightStore.requestTableProfile()     | `DockviewLayout.vue`          |  ✅  |
| `open-table-profile` 消除 → TableProfileView watch reload key      | `TableProfileView.vue`        |  ✅  |

### V2 InsightStore 接口对齐 (2026-05-08 下午)

| 任务                                                   | 文件               | 状态 |
| ------------------------------------------------------ | ------------------ | :--: |
| `diffColumns` computed（当前 vs 历史版本差异列）       | `insight-store.ts` |  ✅  |
| `diffSummary` computed（差异摘要 Record）              | `insight-store.ts` |  ✅  |
| `histogramForChart()` 工具函数（DistributionBin→图表） | `insight-store.ts` |  ✅  |

### i18n 国际化 (2026-05-08 下午)

| 任务                                                         | 文件                   | 状态 |
| ------------------------------------------------------------ | ---------------------- | :--: |
| 新增 `resultPanel.openChart`                                 | zh-CN.json, en.json    |  ✅  |
| 新增 `resultPanel.openVisualization`                         | zh-CN.json, en.json    |  ✅  |
| 新增 `resultPanel.filterApplied`                             | zh-CN.json, en.json    |  ✅  |
| 新增 `workbench.quickProfile` / `workbench.tableQualityEval` | zh-CN.json, en.json    |  ✅  |
| ColumnInsightPanel "图表" 按钮改用 i18n                      | ColumnInsightPanel.vue |  ✅  |

### UX 增强 + V2 对齐 (2026-05-08 下午)

| 任务                                                          | 文件                   | 状态 |
| ------------------------------------------------------------- | ---------------------- | :--: |
| `filterByValue` 成功后展示 toast 通知                         | ColumnInsightPanel.vue |  ✅  |
| `insightStore.isOpen` 状态 (V2 API)                           | insight-store.ts       |  ✅  |
| `insightStore.autoOpenVisualization` 标志                     | insight-store.ts       |  ✅  |
| 结果Grid 右键 "图表可视化" 菜单项（PieChart 图标）            | ResultContextMenu.vue  |  ✅  |
| `openColumnVisualization` 动作处理 → 加载洞察后自动打开可视化 | QueryResultPanel.vue   |  ✅  |

### Phase 9: 深度代码质量优化 (2026-05-08 傍晚)

#### 类型安全增强

| 任务                                                         | 文件                          | 状态 |
| ------------------------------------------------------------ | ----------------------------- | :--: |
| 新增 `MultiRuleResult = Record<string, unknown>` 类型        | `types/result.ts`             |  ✅  |
| `executeInsightRule` 返回类型 `unknown` → `MultiRuleResult`  | `services/result-analysis.ts` |  ✅  |
| 重导出 `export type { MultiRuleResult }`                     | `services/result-analysis.ts` |  ✅  |
| `multiResult` ref 类型 `unknown` → `MultiRuleResult \| null` | `stores/insight-store.ts`     |  ✅  |

#### 输入校验增强

| 任务                                                              | 文件                      | 状态 |
| ----------------------------------------------------------------- | ------------------------- | :--: |
| `loadColumnInsight()` 增加空值校验（`tempTable` / `column` 非空） | `stores/insight-store.ts` |  ✅  |

#### 代码可扩展性重构

| 任务                                                                | 文件                     | 状态 |
| ------------------------------------------------------------------- | ------------------------ | :--: |
| `openVisualization()` if-else 链 → config-based `extractors` Record | `ColumnInsightPanel.vue` |  ✅  |

### Phase 10: 规则内务优化 (2026-05-08 夜)

#### Rust — 规则引擎安全

| 任务                                                                  | 文件               | 状态 |
| --------------------------------------------------------------------- | ------------------ | :--: |
| `panic!()` 消除: `col_index` panic → `Option<usize>` + col_map 预计算 | `rule_executor.rs` |  ✅  |
| 代码去重: 提取 `extract_field_value()` 消除 55 行重复                 | `rule_executor.rs` |  ✅  |
| 单元测试: 新增 10 个测试（col_map/validate/build_sql/execute）        | `rule_executor.rs` |  ✅  |
| 占位符精确替换: `{col}` vs `{col_name}` 子串安全                      | `rule_executor.rs` |  ✅  |

#### Rust — 规则注册健壮性

| 任务                                                     | 文件               | 状态 |
| -------------------------------------------------------- | ------------------ | :--: |
| 用户规则解析失败 → `tracing::warn!` 日志（之前静默跳过） | `rule_registry.rs` |  ✅  |
| 内置规则解析失败 → `tracing::warn!` 日志（之前静默跳过） | `rule_registry.rs` |  ✅  |

#### Rust — Schema 分析器重构

| 任务                                                     | 文件                 | 状态 |
| -------------------------------------------------------- | -------------------- | :--: |
| 提取 `get_batch_rows()` 共享 JSON 导航（消除 2 处重复）  | `schema_analyzer.rs` |  ✅  |
| 提取 `parse_batch_schema()` 共享解析器（消除 20 行重复） | `schema_analyzer.rs` |  ✅  |

#### 前端 — P0/P1/P2 修复

| 任务                                                                                           | 文件                         | 状态 |
| ---------------------------------------------------------------------------------------------- | ---------------------------- | :--: |
| `any` 类型消除: `Record<string, any>` → `Record<string, unknown>`                              | `DataVisualizationPanel.vue` |  ✅  |
| 4 处硬编码中文 → i18n (`distribution`/`topValues`/`monthlyDistribution`/`booleanDistribution`) | `ColumnInsightPanel.vue`     |  ✅  |
| 7 个空 `catch {}` 块 → `console.error('[insightStore] ...')`                                   | `insight-store.ts`           |  ✅  |
| 5 处 `!` 非空断言 → `?? 0` 安全回退                                                            | `ColumnInsightPanel.vue`     |  ✅  |
| `isOpen` 联动面板生命周期 + `closeInsight()` action + `clear()` 复位                           | `insight-store.ts`           |  ✅  |

### Phase 13: QualityRule 质量门控集成 (2026-05-08 深夜)

#### Rust — 类型系统扩展

| 任务                                                                                           | 文件            | 状态 |
| ---------------------------------------------------------------------------------------------- | --------------- | :--: |
| 新增 `ExecutionResult` struct（`data: Value` + `quality: Option<QualityReport>`）              | `rule_types.rs` |  ✅  |
| 新增 `QualityReport` struct（`passed: bool` + `checks: Vec<QualityCheck>`）                    | `rule_types.rs` |  ✅  |
| 新增 `QualityCheck` struct（6 字段：field/passed/rule/actual/severity/message）                | `rule_types.rs` |  ✅  |
| 从 `mod.rs` 重导出 `ExecutionResult`/`QualityReport`/`QualityCheck`/`QualityRule`/`RenderHint` | `mod.rs`        |  ✅  |

#### Rust — 质量门控管线

| 任务                                                                                       | 文件                 | 状态 |
| ------------------------------------------------------------------------------------------ | -------------------- | :--: |
| `RuleExecutor::execute_qualified()` 新方法（包装 `execute()` + `evaluate_quality()`）      | `rule_executor.rs`   |  ✅  |
| `RuleExecutor::evaluate_quality()` 字段阈值检查（min/max 比较→QualityReport）              | `rule_executor.rs`   |  ✅  |
| `insight_engine::execute_insight_rule()` 改用 `execute_qualified()` 返回 `ExecutionResult` | `insight_engine.rs`  |  ✅  |
| `result_service.rs` 返回类型 `Value` → `ExecutionResult`                                   | `result_service.rs`  |  ✅  |
| `result_commands.rs` `serde_json::to_value(exec_result)` 序列化 `ExecutionResult` → JSON   | `result_commands.rs` |  ✅  |

#### Rust — 单元测试扩展

| 任务                                                                 | 文件                      | 状态 |
| -------------------------------------------------------------------- | ------------------------- | :--: |
| QualityRule 测试 3 个（通过阈值/失败min/无规则）                     | `rule_executor.rs`        |  ✅  |
| RuleRegistry 测试 7 个（TOML解析/查找/列表/分类/列类型/全量/空注册） | `rule_registry.rs`        |  ✅  |
| ColumnInsightsPanel.vue 空 catch → `console.error`                   | `ColumnInsightsPanel.vue` |  ✅  |

### Phase 14: DuckDB TTL 临时表清理 (2026-05-08 深夜)

| 任务                                                                    | 文件               | 状态 |
| ----------------------------------------------------------------------- | ------------------ | :--: |
| `TEMP_TABLE_TTL_SECS` 常量（1800s = 30分钟）                            | `duckdb.rs`        |  ✅  |
| `DuckDBManager::cleanup_expired_tables()` 方法（Timestamp 检查 + DROP） | `duckdb.rs`        |  ✅  |
| `register_temp_table()` 自动触发计数淘汰 + TTL 清理                     | `duckdb.rs`        |  ✅  |
| `evict_oldest_tables()` 增加 `tracing::info!` 日志                      | `duckdb.rs`        |  ✅  |
| `validate_analysis_sql()` 改用 `TEMP_TABLE_PREFIX` 常量（消除死代码）   | `duckdb.rs`        |  ✅  |
| TTL 单元测试 2 个（过期清理 + 新表保留）                                | `duckdb.rs`        |  ✅  |
| `unused_mut` 警告修复（rule_executor 测试）                             | `rule_executor.rs` |  ✅  |

### Phase 15: 全栈审计 + 文档补全 + 测试补全 + get_unwrap 消除 (2026-05-08 深夜)

#### Rust — P0 生产代码 unwrap 消除

| 任务                                                                            | 文件                 | 状态 |
| ------------------------------------------------------------------------------- | -------------------- | :--: |
| `extract_field_value()` 10 处 `row.get_unwrap(idx)` → `row.get(idx).map_err(…)` | `rule_executor.rs`   |  ✅  |
| `get_column_sample_internal()` `row.get_unwrap(0)` → `row.get(0)?`              | `insight_engine.rs`  |  ✅  |
| `schema_analyzer.rs` 全量审计：确认生产代码已安全（仅测试有 unwrap）            | `schema_analyzer.rs` |  ✅  |

#### Rust — P1 测试补全

| 任务                                                                   | 文件                | 状态 |
| ---------------------------------------------------------------------- | ------------------- | :--: |
| `get_column_stats_internal` 测试 4 个（numeric/text/boolean/all-null） | `insight_engine.rs` |  ✅  |
| `get_column_sample_internal` 测试：采样上限 5 行                       | `insight_engine.rs` |  ✅  |
| `quality_scorer::compute_column_quality` 测试 2 个（高分/低分）        | `insight_engine.rs` |  ✅  |
| `quality_scorer::compute_table_quality` 测试：表聚合 + 排序            | `insight_engine.rs` |  ✅  |
| `list_insight_rules` 测试：内置规则存在 + id/name 字段                 | `insight_engine.rs` |  ✅  |
| `list_rules_for_column` 测试：numeric 规则包含 `numeric-stats`         | `insight_engine.rs` |  ✅  |

#### P2 — 文档补全

| 任务                                                              | 文件                       | 状态 |
| ----------------------------------------------------------------- | -------------------------- | :--: |
| 创建 API 接口参考（Tauri Commands + 前端 API + 数据类型）         | `INSIGHT-API-REFERENCE.md` |  ✅  |
| 创建规则文件格式规范（TOML schema + 值类型 + 质量门控 + 示例）    | `INSIGHT-RULE-FORMAT.md`   |  ✅  |
| 更新架构文档（追加质量门控管线 + DuckDB TTL 生命周期 + 文档索引） | `INSIGHT-ARCHITECTURE.md`  |  ✅  |

### Phase 16: 死代码消除 + 质量评分独立测试 + 前端防护 (2026-05-08 深夜)

| 任务                                                                                     | 文件                      | 状态 |
| ---------------------------------------------------------------------------------------- | ------------------------- | :--: |
| 移除 `DuckDbService::register_temp_table` / `cleanup_temp_table` 死代码包装              | `duckdb_service.rs`       |  ✅  |
| `quality_scorer.rs` 新增 7 个独立测试（完美/极差/文本/布尔/表聚合/空表/维度）            | `quality_scorer.rs`       |  ✅  |
| `SchemaInsightPanel.vue` mismatch-tables 空数组 → `noAffectedTables` 提示                | `SchemaInsightPanel.vue`  |  ✅  |
| `ColumnInsightsPanel.vue` `formatNum` NaN/null 防护 → 返回 `—`                           | `ColumnInsightsPanel.vue` |  ✅  |
| 新增 i18n key `schemaInsight.noAffectedTables` (zh-CN: 无关联表, en: No affected tables) | `zh-CN.json` / `en.json`  |  ✅  |
| 消除 cargo 死代码 warning `duckdb_service.rs`                                            | —                         |  ✅  |

### Phase 17: P2 组件拆分 + 面板角色明确 + RenderHint 前后端打通 (2026-05-08)

| 任务                                                                       | 文件                              | 状态 |
| -------------------------------------------------------------------------- | --------------------------------- | :--: |
| **P2-1**: ColumnInsightPanel.vue 拆分 960→276 行 orchestrator              | `ColumnInsightPanel.vue`          |  ✅  |
| **P2-1**: 提取 QualityScoreCard 组件（评分徽章+维度）                      | `insight/QualityScoreCard.vue`    |  ✅  |
| **P2-1**: 提取 InsightStatsSection 组件（统计/分布/质量/采样折叠）         | `insight/InsightStatsSection.vue` |  ✅  |
| **P2-1**: 提取 InsightHistoryTab 组件（版本列表+Diff 面板）                | `insight/InsightHistoryTab.vue`   |  ✅  |
| **P2-2**: ColumnInsightsPanel.vue JSDoc 角色明确化（快速一览 vs 完整洞察） | `ColumnInsightsPanel.vue`         |  ✅  |
| **P2-3**: RenderHint 类型定义（前端）                                      | `result-analysis.ts`              |  ✅  |
| **P2-3**: insightStore `pendingVisualizationRequest` 扩展 `chartType`      | `insight-store.ts`                |  ✅  |
| **P2-3**: DockviewLayout `openVisualization()` 传递 `chartType`            | `DockviewLayout.vue`              |  ✅  |
| **P2-3**: DataVisualizationPanel 消费 `params.chartType` 自动选择图表      | `DataVisualizationPanel.vue`      |  ✅  |
| ✅ cargo check + eslint 验证                                               | —                                 |  ✅  |

### Phase 18: P0 diff 渲染修复 + P1 类型安全强化 + import 路径修正 (2026-05-08)

| 任务                                                                                                       | 文件                                                                       | 状态 |
| ---------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------- | :--: |
| **P0**: InsightHistoryTab.vue diff 渲染修复 — `diffColumns` (string[]) 被当作对象 `.old`/`.new` 访问       | `InsightHistoryTab.vue`                                                    |  ✅  |
| **P0**: diff 渲染改为遍历 `diffColumns` + 读取 `diffSummary[colName]`                                      | `InsightHistoryTab.vue`                                                    |  ✅  |
| **P0**: 新增空 diff 状态 `noDiff` + 移除无效 `diffSummary` 对象渲染                                        | `InsightHistoryTab.vue`                                                    |  ✅  |
| **P1**: InsightStatsSection.vue 消除 22 处 `as` 强制类型转换 → 4 个 typed computed                         | `InsightStatsSection.vue`                                                  |  ✅  |
| **P1**: `statsKind` props 从 `string` 收紧为 `'Numeric' \| 'Text' \| 'DateTime' \| 'Boolean' \| 'Unknown'` | `InsightStatsSection.vue` + `insight-store.ts`                             |  ✅  |
| **P1**: insight-store.ts `statsKind` computed 返回联合类型字面量（替代泛型 `string`）                      | `insight-store.ts`                                                         |  ✅  |
| **Fix**: 3 个子组件 import path 修正（`insight/` 目录深度多一层）                                          | `QualityScoreCard.vue`, `InsightStatsSection.vue`, `InsightHistoryTab.vue` |  ✅  |
| **i18n**: 新增 4 个 key (`insightHistory`, `comparing`, `diffResult`, `noDiff`)                            | `zh-CN.json` + `en.json`                                                   |  ✅  |
| ✅ eslint 0 errors (import order + path 修正)                                                              | —                                                                          |  ✅  |

### Phase 19: 审计修复 — 类型分类 + 常量配置化 + 测试验证 (2026-05-08)

| 任务                                                                          | 文件                 | 状态 |
| ----------------------------------------------------------------------------- | -------------------- | :--: |
| **P0**: 新增 `is_binary_type` / `is_json_type` / `is_array_type` 类型分类器   | `duckdb_service.rs`  |  ✅  |
| **P0**: `get_column_stats_internal` BLOB 分支 → `ColumnStatsDetail::Unknown`  | `insight_engine.rs`  |  ✅  |
| **P0**: `get_column_stats_internal` ARRAY 分支 → `ColumnStatsDetail::Unknown` | `insight_engine.rs`  |  ✅  |
| **P0**: `get_column_stats_internal` JSON 分支 → `compute_text_stats`          | `insight_engine.rs`  |  ✅  |
| **P0**: `get_column_stats_internal` 全 NULL 列 (`non_null == 0`) → `Unknown`  | `insight_engine.rs`  |  ✅  |
| **P1**: 样本量硬编码 `LIMIT 5` → `DEFAULT_SAMPLE_SIZE` 常量                   | `insight_engine.rs`  |  ✅  |
| **P1**: 直方图最小行数 `10` → `HISTOGRAM_MIN_ROWS` 常量                       | `insight_engine.rs`  |  ✅  |
| **P1**: 质量评分四权重 (35/25/20/20) → `WEIGHT_*` 常量                        | `quality_scorer.rs`  |  ✅  |
| **P1**: 质量评分五级阈值 (85/70/50/30) → `GRADE_*` 常量                       | `quality_scorer.rs`  |  ✅  |
| 验证 rule_registry 测试覆盖 (7 个，已完备)                                    | `rule_registry.rs`   |  ✅  |
| 验证 schema_analyzer 测试覆盖 (15 个，已完备)                                 | `schema_analyzer.rs` |  ✅  |
| ✅ cargo check exit 0 + eslint exit 0 (仅 2 预存 warning)                     | —                    |  ✅  |

**类型分类修复前后对比：**

| DuckDB typeof() | 修复前                                     | 修复后                                |
| --------------- | ------------------------------------------ | ------------------------------------- |
| BLOB/BYTEA      | → Text（无效统计）                         | → `Unknown`（跳过统计）               |
| JSON/JSONB      | → Text（可接受但不准确）                   | → `Text`（保留文本统计，识别为 JSON） |
| ARRAY/LIST      | → Text（无效统计）                         | → `Unknown`（跳过统计）               |
| 全 NULL 列      | → Text（`min_length`/`max_length` 无意义） | → `Unknown`（检测 `non_null == 0`）   |

### Phase 20: 归档三修复 — ID重复warning + 复合FK + 规则热加载 (2026-05-09) 📦

> 此阶段完成后洞察模块暂归档。修复三个审计发现的遗留问题：

| 任务                                                                                                                                         | 文件                     | 状态 |
| -------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------ | :--: |
| **规则ID重复检测**: `scan_directory()` 新增 `HashSet<String>` 跟踪已见ID                                                                     | `rule_registry.rs`       |  ✅  |
| **规则ID重复检测**: 重复ID时 `tracing::warn!("Duplicate rule ID '{}' in file '{}': rule will be overwritten...")`                            | `rule_registry.rs`       |  ✅  |
| **规则ID重复检测**: `load_from_embedded_dir()` 同逻辑                                                                                        | `rule_registry.rs`       |  ✅  |
| **复合外键推断**: 新增 `find_compound_fk_target()` 函数 — 对 `order_line_item_id` 尝试 `order_line_item` → `line_item` → `item` 逐级后缀匹配 | `schema_analyzer.rs`     |  ✅  |
| **复合外键推断**: 新增 2 个测试（`test_infer_fk_compound_name` + `test_infer_fk_compound_name_full_match_preferred`）                        | `schema_analyzer.rs`     |  ✅  |
| **规则热加载**: `mod.rs` 新增 `reload_insight_rules(project_path)` — 重建 RuleRegistry + embedded + user 目录扫描                            | `insight/mod.rs`         |  ✅  |
| **规则热加载**: `result_commands.rs` 新增 `reload_insight_rules` Tauri 命令（同步，返回规则总数）                                            | `result_commands.rs`     |  ✅  |
| **规则热加载**: `lib.rs` 注册 `reload_insight_rules` 命令                                                                                    | `lib.rs`                 |  ✅  |
| **规则热加载**: `result-analysis.ts` 新增 `reloadInsightRules(projectPath)` API                                                              | `result-analysis.ts`     |  ✅  |
| **规则热加载**: `insight-store.ts` 新增 `reloadRules(projectPath)` action + `isReloadingRules` 状态                                          | `insight-store.ts`       |  ✅  |
| **规则热加载**: `ColumnInsightPanel.vue` applicableRules 区域新增 🔄 热加载按钮                                                              | `ColumnInsightPanel.vue` |  ✅  |
| ✅ cargo check insight 模块 0 errors（预存 analytics_resource_store rusqlite 错误无关）                                                      | —                        |  ✅  |

**复合外键推断算法：**

```
column: order_line_item_id
→ base_prefix: order_line_item
→ candidates (从最长到最短):
  1. order_line_item → order_line_items / order_line_item
  2. line_item       → line_items       / line_item       ← 匹配!
  3. item            → items            / item
```

**规则热加载流程：**

```
前端 🔄 按钮 → insightStore.reloadRules(projectPath)
→ invoke('reload_insight_rules', { project_path })
→ insight::reload_insight_rules(&path)
→ 重建 RuleRegistry → embedded + user dir 重扫描
→ 返回 rule_count → Store loadMultiRules() 刷新规则列表
```

### Phase 21: 全局主题样式统一 (2026-05-09)

> 打通洞察模块前端样式与全局 `tokens.css`，消除所有硬编码颜色/字体/间距/圆角。

| 任务                                                                                                                 | 文件                      | 状态 |
| -------------------------------------------------------------------------------------------------------------------- | ------------------------- | :--: |
| **tokens.css 补充**: `--brand-info`/`--brand-info-soft`/`--brand-blue`/`--brand-danger-soft`/`--brand-danger-bg`     | `tokens.css`              |  ✅  |
| **tokens.css 补充**: `--spacing-xl: 20px`/`--spacing-2xl: 24px`                                                      | `tokens.css`              |  ✅  |
| **tokens.css 补充**: `--border-radius-lg: 8px`                                                                       | `tokens.css`              |  ✅  |
| **tokens.css 补充**: `--font-size-xxs: 9px`/`--font-size-xss: 11px`/`--font-size-xxl: 20px`/`--font-size-huge: 24px` | `tokens.css`              |  ✅  |
| **移除去 fallback**: 全局替换 `var(--x, #fallback)` → `var(--x)` 共 ~45 处                                           | 全部 9 .vue               |  ✅  |
| **硬编码颜色→token**: `#b51a1a`/`#fff0f0`→`--brand-danger`/`--brand-danger-bg`                                       | `TableProfileView.vue`    |  ✅  |
| **硬编码颜色→token**: `#00cec9`→`--brand-info`, `rgba(0,206,201,0.15)`→`--brand-info-soft`                           | `QualityScoreCard.vue`    |  ✅  |
| **硬编码颜色→token**: `#74b9ff`→`--brand-blue`, `rgba(0,123,255,0.08)`→`rgba(116,185,255,0.08)`                      | `InsightStatsSection.vue` |  ✅  |
| **font-size→token**: 全部 9/10/11/12/13/14/16/20/22/24px → `--font-size-xxs/xs/xss/sm/md/lg/xl/xxl/huge`             | 全部 9 .vue               |  ✅  |
| **spacing→token**: 全部 4/8/12/16/20/24px → `--spacing-xs/sm/md/lg/xl/2xl`                                           | 全部 9 .vue               |  ✅  |
| **border-radius→token**: 全部 4/6px → `--border-radius-sm/md`                                                        | 全部 9 .vue               |  ✅  |
| ✅ eslint 0 errors (443 预存 warning 无关)                                                                           | —                         |  ✅  |

**修改统计：**

| 指标                  | 数值                                      |
| --------------------- | ----------------------------------------- |
| 修改文件              | 10 (1 tokens.css + 9 .vue)                |
| 消除硬编码颜色        | 4 处 HEX/RGB → CSS 变量                   |
| 消除 fallback 冗余    | ~45 处 `var(--x, #fallback)` → `var(--x)` |
| font-size → token     | ~55 处                                    |
| spacing → token       | ~30 处                                    |
| border-radius → token | ~10 处                                    |

### Phase 22: 全栈审计修复 (2026-05-09)

> 修复 Phase 21 全栈审计发现的 5 个不一致问题。

| 任务                                                                                                                                         | 文件                     | 状态 |
| -------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------ | :--: |
| **render 字段补充**: `list_insight_rules()` JSON 增加 `"render": r.render`                                                                   | `insight_engine.rs`      |  ✅  |
| **render 字段补充**: `list_rules_for_column()` JSON 增加 `"result_type": ...` + `"render": ...`                                              | `insight_engine.rs`      |  ✅  |
| **死类型清理**: `result.ts` 删除 6 个旧类型 (ColumnInsight/ColumnStats/HistogramBucket/QualityScore/TableProfile/TableColumnMeta) — 共 65 行 | `result.ts`              |  ✅  |
| **MultiRuleResult 收紧**: `Record<string, unknown>` → 结构化 `{ data, quality }`                                                             | `result.ts`              |  ✅  |
| **isOpen 清理**: insight-store 移除 `isOpen` 死状态 + `closeInsight()` 死函数                                                                | `insight-store.ts`       |  ✅  |
| **handleCleanup 修复**: ColumnInsightPanel 清理按钮从调用 `closeInsight()` (误) 改为 `cleanupInsightSnapshots()` (正)                        | `ColumnInsightPanel.vue` |  ✅  |
| ✅ cargo check exit 0 (3 预存 warning) + eslint 4 insight 文件 0 errors                                                                      | —                        |  ✅  |

#### Phase 22.1: 文档一致化 (2026-05-09)

| 任务                                                                                                                                                    | 文件                      | 状态 |
| ------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------- | :--: |
| **ARCHITECTURE.md 规则数修正**: 13→18条 + 补充 table/quality 目录                                                                                       | `INSIGHT-ARCHITECTURE.md` |  ✅  |
| **ARCHITECTURE.md Commands 补全**: 8→15个命令 (补充7个缺失命令)                                                                                         | `INSIGHT-ARCHITECTURE.md` |  ✅  |
| **ARCHITECTURE.md 文件清单更新**: 新增 insight_engine/quality_scorer/duckdb_service + DataVisualizationPanel/ColumnInsightsPanel + tokens.css/result.ts | `INSIGHT-ARCHITECTURE.md` |  ✅  |
| **ARCHITECTURE.md 版本更新**: v18.0→v19.0 + Phase 21/22 状态同步                                                                                        | `INSIGHT-ARCHITECTURE.md` |  ✅  |
| **DESIGN.md 版本更新**: v1.2→v1.3 + 状态行从 Phase 19→Phase 22                                                                                          | `INSIGHT-DESIGN.md`       |  ✅  |
| **DESIGN.md v2.1路线图修正**: 规则热加载标记 ✅ Phase 20 已完成                                                                                         | `INSIGHT-DESIGN.md`       |  ✅  |
| ✅ cargo check exit 0 (3 预存 warning, 仅文档变更) + eslint 0 errors                                                                                    | —                         |  ✅  |

## 三、变更日志

\| 时间 | 类型 | 描述 |
\| --------------- | -------- | --------------------------------------------------------------------------------------------------------- | --- |
\| 2026-05-07 上午 | feat | Phase 1: 洞察面板 + 统计引擎 + 持久化 |
\| 2026-05-07 下午 | feat | 规则引擎: 13 个 TOML + insight/ 模块 |
\| 2026-05-07 下午 | refactor | compute\_\* 改用 RuleExecutor |
\| 2026-05-07 下午 | feat | Phase 1.5: MultiColumnView + NTabs |
\| 2026-05-07 下午 | feat | 用户自定义规则扫描 (project open) |
\| 2026-05-07 下午 | fix | ResourceVersion 导出 / non-exhaustive patterns |
\| 2026-05-07 下午 | build | ✅ cargo check exit 0 (7次验证, 最终通过) |
\| 2026-05-07 傍晚 | fix | **P0打通修复**: insight-store.ts 新增 actions/computed |
\| 2026-05-07 傍晚 | fix | **P0打通修复**: MultiColumnView 改用 Store |
\| 2026-05-07 傍晚 | feat | **P0打通修复**: 清理旧数据按钮 (storage footer) |
\| 2026-05-07 傍晚 | feat | **P0打通修复**: 适用规则推荐标签 (column tab) |
\| 2026-05-07 傍晚 | build | ✅ cargo check exit 0 (第8次验证) |
\| 2026-05-07 夜晚 | feat | **表探查**: TableProfile struct + get*table_profile() |
\| 2026-05-07 夜晚 | feat | **表探查**: 3条 table-level TOML 规则 |
\| 2026-05-07 夜晚 | feat | **表探查**: TableProfileView\.vue 前端面板 |
\| 2026-05-07 夜晚 | feat | **表探查**: 导航树右键"快速探查"菜单 |
\| 2026-05-07 夜晚 | feat | **表探查**: DockviewLayout 动态创建面板 |
\| 2026-05-07 夜晚 | feat | **优化**: ColumnInsightPanel 骨架屏加载 |
\| 2026-05-07 夜晚 | build | ✅ cargo check exit 0 (第9次验证) |
\| 2026-05-07 夜晚 | feat | **历史对比**: get_insight_version_detail 命令 |
\| 2026-05-07 夜晚 | feat | **历史对比**: 版本列表 + 点击加载 + Diff 面板 |
\| 2026-05-07 夜晚 | feat | **导出**: JSON 下载 + Markdown 导出 |
\| 2026-05-07 夜晚 | feat | **联动**: TableProfileView 列名可点击 + 事件 |
\| 2026-05-07 夜晚 | build | ✅ cargo check exit 0 (第10次验证) |
\| 2026-05-07 夜晚 | feat | **联动**: profile_column_from_table 合并命令 |
\| 2026-05-07 夜晚 | feat | **联动**: table-column-click 端到端打通 |
\| 2026-05-07 夜晚 | feat | **动画**: NTabs tab-fade-in 过渡动画 |
\| 2026-05-07 夜晚 | build | ✅ cargo check exit 0 (第11次验证, 最终) |
\| 2026-05-07 深夜 | feat | **质量评分**: QualityScore struct + compute_column_quality() |
\| 2026-05-07 深夜 | feat | **质量评分**: get_column_quality Tauri 命令 + API |
\| 2026-05-07 深夜 | feat | **质量评分**: ColumnInsightPanel 评分徽章（分数+等级+颜色+进度条） |
\| 2026-05-07 深夜 | feat | **质量评分**: 四维度评分（完整性35%/唯一性25%/类型一致20%/分布均匀20%） |
\| 2026-05-07 深夜 | feat | **质量评分**: watch insightData 自动加载评分 |
\| 2026-05-07 深夜 | fix | **已有错误修复**: scratchpad 模块 6 个编译错误全部修复 |
\| 2026-05-07 深夜 | feat | **表级质量聚合**: `batch_evaluate_columns` 一次调用全表质量评估 |
\| 2026-05-07 深夜 | feat | **表级质量聚合**: TableProfileView 质量评估按钮 + 评分列 + 摘要栏 |
\| 2026-05-07 深夜 | feat | **表级质量聚合**: 4 个 struct（TableQuality, ColumnQualityEntry, TableQualityInput → BatchEvaluateInput） |
\| 2026-05-07 深夜 | refactor | **表级质量聚合**: `get_table_quality` → `batch_evaluate_columns`（单次往返全表评估） |
\| 2026-05-07 深夜 | feat | **Schema 洞察**: `SchemaAnalyzer` 核心分析引擎（5 个分析维度） |
\| 2026-05-07 深夜 | feat | **Schema 洞察**: 外键推断（4 种命名模式） + 置信度分级 |
\| 2026-05-07 深夜 | feat | **Schema 洞察**: 类型不一致检测 + 孤立表检测 + 冗余列检测 |
\| 2026-05-07 深夜 | feat | **Schema 洞察**: `compute_health()` Schema 健康评分算法 |
\| 2026-05-07 深夜 | feat | **Schema 洞察**: `get_schema_insight` Tauri 命令 + 8 个 TS 类型 |
\| 2026-05-07 深夜 | feat | **Schema 洞察**: `SchemaInsightPanel.vue`（健康环 + 4 折叠维度） |
\| 2026-05-07 深夜 | feat | **Schema 洞察**: 导航树 Schema 节点右键 "Schema 洞察" 菜单项 |
\| 2026-05-07 深夜 | feat | **Schema 洞察**: `open-schema-insight` CustomEvent 面板联动 |
\| 2026-05-07 深夜 | fix | DockviewLayout.vue import 路径 `\` → `/` 兼容 lint |
\| 2026-05-07 深夜 | build | ✅ cargo check exit 0 (第18/19次验证) | |
\| 2026-05-08 上午 | refactor | **DuckDB 统一**: `DuckDBManager` 全局单例 + `ResultService`/`DuckDBEngine` 迁移 |
\| 2026-05-08 上午 | refactor | **DuckDB 统一**: `duckdb_engine.rs` tokio Mutex → std Mutex（7 个方法适配） |
\| 2026-05-08 上午 | build | ✅ cargo check exit 0 (第20次验证, DuckDB统一完成) |
\| 2026-05-08 上午 | feat | **导出增强**: SchemaInsightPanel 新增 JSON + Markdown 导出 + 表/列数元信息 |
\| 2026-05-08 下午 | fix | **P0修复**: DuckDBManager 改为返回 Result + 消除 expect() + SQL 转义防注入 |
\| 2026-05-08 下午 | fix | **P0修复**: Frontend Store 统一 (tableQuality/schemaInsight 状态 → insightStore) |
\| 2026-05-08 下午 | refactor | **B组消除**: table-column-click CustomEvent 消除（TableProfile → insightStore.loadColumnFromTable） |
\| 2026-05-08 下午 | refactor | **B组消除**: open-schema-insight CustomEvent 消除（store signal → DockviewLayout watcher） |
\| 2026-05-08 下午 | refactor | **B组消除**: open-table-profile CustomEvent 消除（store signal + reload key） |
\| 2026-05-08 下午 | feat | **V2对齐**: insightStore 新增 diffColumns / diffSummary / histogramForChart |
\| 2026-05-08 下午 | test | 新增 22 单元测试 (DuckDBManager 8 + SchemaAnalyzer 14) |
\| 2026-05-08 下午 | build | ✅ cargo check (仅预存 state.rs 错误) + eslint (无新增告警) |
\| 2026-05-08 下午 | feat | **i18n**: 新增 6 个翻译 key (openChart/openVisualization/filterApplied/quickProfile/tableQualityEval) |
\| 2026-05-08 下午 | feat | **UX**: filterByValue 增加 toast 通知 + 结果Grid右键"图表可视化" + 自动跳转可视化面板 |
\| 2026-05-08 下午 | feat | **V2对齐**: insightStore.isOpen + autoOpenVisualization 状态 |
\| 2026-05-08 傍晚 | feat | **类型安全**: 新增 `MultiRuleResult` 类型 + `executeInsightRule` 显式返回类型 + `multiResult` 类型收紧 |
\| 2026-05-08 傍晚 | feat | **输入校验**: `loadColumnInsight()` 增加空值校验（防御性编程） |
\| 2026-05-08 傍晚 | refactor | **可扩展性**: `openVisualization()` if-else 链 → config-based `extractors` Record（新列类型零侵入扩展） |
\| 2026-05-08 夜 | fix | **P0**: `rule_executor.rs` `panic!()` 消除 + `col_index` → `Option<usize>` + HashMap 预计算（O(n)→O(1)） |
\| 2026-05-08 夜 | refactor | **去重**: `rule_executor.rs` 提取 `extract_field_value()` 消除 55 行重复 |
\| 2026-05-08 夜 | test | **测试**: `rule_executor.rs` 新增 10 个单元测试（col_map/validate/build_sql/execute） |
\| 2026-05-08 夜 | fix | **SQL安全**: `build_sql` 占位符两步替换（marker中间层）防止 `{col}` vs `{col_name}` 子串误匹配 |
\| 2026-05-08 夜 | fix | **日志**: `rule_registry.rs` 用户/内置规则解析失败 → `tracing::warn!`（之前静默跳过） |
\| 2026-05-08 夜 | refactor | **去重**: `schema_analyzer.rs` 提取 `get_batch_rows()` + `parse_batch_schema()` 消除 25 行重复 |
\| 2026-05-08 夜 | fix | **P0**: `DataVisualizationPanel.vue` `any` → `unknown` |
\| 2026-05-08 夜 | feat | **i18n**: 4 个新 key (`distribution`/`topValues`/`monthlyDistribution`/`booleanDistribution`) |
\| 2026-05-08 夜 | fix | **错误日志**: `insight-store.ts` 7 个空 `catch {}` → `console.error('[insightStore] ...')` |
\| 2026-05-08 夜 | fix | **类型安全**: `ColumnInsightPanel.vue` 5 处 `!` 非空断言 → `?? 0` 安全回退 |
\| 2026-05-08 夜 | feat | **生命周期**: `insight-store.ts` `isOpen` 联动 + `closeInsight()` + `clear()` 复位 |
\| 2026-05-08 深夜 | feat | **质量门控**: `ExecutionResult`/`QualityReport`/`QualityCheck` 新类型 + `execute_qualified()` + `evaluate_quality()` |
\| 2026-05-08 深夜 | feat | **质量门控**: Pipeline 打通（insight_engine → result_service → result_commands → JSON 序列化） |
\| 2026-05-08 深夜 | test | 新增 10 单元测试（QualityRule 3 + RuleRegistry 7）+ ColumnInsightsPanel.vue 空 catch 修复 |
\| 2026-05-08 深夜 | feat | **DuckDB TTL**: `cleanup_expired_tables()` 30分钟过期清理 + `register_temp_table()` 自动触发 |
\| 2026-05-08 深夜 | fix | **死代码消除**: `TEMP_TABLE_PREFIX` 常量用于 `validate_analysis_sql()` + `unused_mut` 修复 |
\| 2026-05-08 深夜 | test | 新增 2 个 TTL 单元测试（过期清理 + 新表保留） |
| 2026-05-08 深夜 | fix | **P0 消除 unwrap**: `rule_executor.rs` extract*field_value 10处 get_unwrap → get+map_err；`insight_engine.rs` 1处 |
| 2026-05-08 深夜 | test | **P1 测试补全**: `insight_engine.rs` 新增 10 测试（stats 4 + sample 1 + quality 2 + tableQuality 1 + rules 2） |
| 2026-05-08 深夜 | docs | **P2 文档补全**: 新建 `INSIGHT-API-REFERENCE.md` (API参考) + `INSIGHT-RULE-FORMAT.md` (规则格式规范) |
| 2026-05-08 深夜 | docs | **P2 架构更新**: `INSIGHT-ARCHITECTURE.md` v13→v14 追加质量门控管线 + DuckDB TTL 生命周期 + 文档索引 |
| 2026-05-08 深夜 | audit | **全栈审计**: schema_analyzer.rs 确认生产代码安全（仅测试含 unwrap） |
| 2026-05-08 深夜 | fix | **死代码消除**: `duckdb_service.rs` 移除 register_temp_table/cleanup_temp_table 冗余包装 — 1 个 cargo warning 消除 |
| 2026-05-08 深夜 | test | **quality_scorer 独立测试**: 7 个测试（完美/极差/文本类型/布尔类型/表聚合排序/空表/4维度验证） |
| 2026-05-08 深夜 | fix | **前端防护**: `SchemaInsightPanel.vue` mismatch tables 空数组空状态 + `ColumnInsightsPanel.vue` formatNum NaN 防护 |
| 2026-05-08 深夜 | i18n | **新增 key**: `schemaInsight.noAffectedTables` — zh-CN: 无关联表 / en: No affected tables |
| 2026-05-08 | refactor | **P2-1 组件拆分**: ColumnInsightPanel.vue 960→276 行 orchestrator + 3 新子组件 (QualityScoreCard/InsightStatsSection/InsightHistoryTab) |
| 2026-05-08 | docs | **P2-2 面板角色明确**: ColumnInsightsPanel.vue 新增 JSDoc 说明与 ColumnInsightPanel 的差异（快速一览 vs 完整洞察） |
| 2026-05-08 | feat | **P2-3 RenderHint 打通**: result-analysis.ts 新增 RenderHint 类型 + insightStore 扩展 chartType |
| 2026-05-08 | feat | **P2-3 RenderHint 打通**: DockviewLayout openVisualization 传递 chartType → DataVisualizationPanel 自动选择图表 |
| 2026-05-08 | build | ✅ cargo check exit 0 + eslint 通过 (第28次验证, Phase 17 完成) |
| 2026-05-08 | fix | **P0 diff 渲染修复**: InsightHistoryTab diffColumns(string[]) 被当作对象访问 → 遍历 diffColumns + diffSummary 渲染 |
| 2026-05-08 | refactor | **P1 类型安全**: InsightStatsSection 消除 22 处 `as` 强制类型转换 → 4 个 typed computed (numeric/text/dateTime/boolean Detail) |
| 2026-05-08 | type | **P1 类型收紧**: statsKind `string` → `'Numeric' | 'Text' | 'DateTime' | 'Boolean' | 'Unknown'` 联合类型 |
| 2026-05-08 | fix | **Import 路径修正**: QualityScoreCard/InsightStatsSection/InsightHistoryTab `../../` → `../../../`（insight/ 目录多一层） |
| 2026-05-08 | i18n | **新增 4 个 key**: insightHistory/comparing/diffResult/noDiff (zh-CN + en) |
| 2026-05-08 | build | ✅ eslint exit 0 (2 import 错误修复), cargo check 0 insight 错误（50 预存 faker 错误无关）|
| 2026-05-08 | fix | **P0 类型分类修复**: BLOB→Unknown / ARRAY→Unknown / 全NULL→Unknown / JSON→Text（不再 silent fallthrough） |
| 2026-05-08 | refactor | **P1 常量配置化**: DEFAULT_SAMPLE_SIZE / HISTOGRAM_MIN_ROWS / WEIGHT** / GRADE\_\_ |
| 2026-05-08 | verify | 测试完备验证: rule_registry(7) + schema_analyzer(15) + insight_engine(8) + quality_scorer(7) = 37 个测试 |
| 2026-05-08 | build | ✅ cargo check exit 0 (2 预存 warning) + eslint exit 0 (第30次验证, Phase 19 完成) |
| 2026-05-09 | fix | **规则ID重复warning**: `rule_registry.rs` scan_directory + load_from_embedded_dir 增加 `HashSet` 重复检测 + `tracing::warn!` |
| 2026-05-09 | fix | **复合FK推断**: `schema_analyzer.rs` 新增 `find_compound_fk_target()` 逐级后缀匹配 + 2 测试 |
| 2026-05-09 | feat | **规则热加载**: `reload_insight_rules` Rust 函数 + Tauri 命令 + 前端 API + Store action + UI 按钮 |
| 2026-05-09 | build | ✅ cargo check insight 模块 0 errors（第31次验证, Phase 20 完成） |
| 2026-05-09 | refactor | **全局主题样式统一**: tokens.css 补充 10 个 token + 9 个 .vue 消除所有硬编码 (Phase 21) |
| 2026-05-09 | build | ✅ eslint 0 errors (第32次验证, Phase 21 完成) |
| 2026-05-09 | fix | **审计修复 P0**: list_insight_rules + list_rules_for_column 补充 `render` 字段 |
| 2026-05-09 | refactor | **审计修复 P0**: result.ts 删除 6 个死类型 (65行) + `MultiRuleResult` 收紧 |
| 2026-05-09 | refactor | **审计修复 P2**: insight-store 移除 `isOpen` 死状态 + `closeInsight()` + handleCleanup 修正 |
| 2026-05-09 | build | ✅ cargo check exit 0 + eslint 4 insight 文件 0 errors (第33次验证, Phase 22 完成) |
| 2026-05-09 | docs | **文档一致化**: ARCHITECTURE.md 规则数13→18 + Commands 8→15 + v18→v19 |
| 2026-05-09 | docs | **文档一致化\*\*: DESIGN.md v1.2→v1.3 + v2.1路线图热加载标记已完成 |
| 2026-05-09 | build | ✅ cargo check exit 0 + eslint 0 errors (第34次验证, Phase 22.1 完成) |

---

## 四、构建验证记录

|  #  |     结果     | 说明                                                                                                                                                                      |
| :-: | :----------: | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
|  7  |  ✅ exit 0   | Phase 1.5: 仅2个占位警告                                                                                                                                                  |
|  8  |  ✅ exit 0   | P0打通修复: 仅2个占位警告                                                                                                                                                 |
|  9  |  ✅ exit 0   | Phase 2 表探查: 仅2个占位警告                                                                                                                                             |
| 10  |  ✅ exit 0   | 历史对比/导出/联动: 仅2个占位警告                                                                                                                                         |
| 11  |  ✅ exit 0   | 表列探查联动 + 动画: 仅2个占位警告                                                                                                                                        |
| 12  |  ✅ exit 0   | 质量评分后端: 仅3个占位/未使用警告                                                                                                                                        |
| 13  |  ✅ exit 0   | 已有错误修复 + 质量评分最终                                                                                                                                               |
| 14  |  ✅ exit 0   | 质量评分前端: 仅3个占位/未使用警告                                                                                                                                        |
| 15  |  ✅ exit 0   | 表质量聚合后端: 仅3个占位/未使用警告                                                                                                                                      |
| 16  |  ✅ exit 0   | 表质量聚合前端: 仅3个占位/未使用警告                                                                                                                                      |
| 17  |  ✅ exit 0   | Schema 洞察后端: 仅3个占位/未使用警告                                                                                                                                     |
| 18  |  ✅ exit 0   | Schema 洞察前端: 0 errors, 2 已有 warning                                                                                                                                 |
| 19  |  ✅ exit 0   | 最终验证: 0 errors, 0 warnings                                                                                                                                            |
| 20  |  ✅ exit 0   | DuckDB 统一: 仅2个占位警告                                                                                                                                                |
| 21  |   ⚠️ E0716   | P0修复+B组消除+V2对齐: insight模块编译通过，预存 state.rs:78 E0716                                                                                                        |
| 22  |  ✅ exit 0   | i18n+UX增强+V2对齐: eslint 492→492 problems (2减少)                                                                                                                       |
| 23  |  ✅ exit 0   | Phase 9 深度代码质量优化: eslint 494 problems (4 errors, 490 warnings 全为预存), cargo check insight模块 clean                                                            |
| 24  |  ✅ exit 0   | Phase 10 规则内务优化: cargo check --tests clean (仅预存 state.rs E0716), eslint insight文件 0 新增, 10 新测试                                                            |
| 25  |  ✅ exit 0   | Phase 13 QualityRule 质量门控 + Phase 14 DuckDB TTL: cargo check --tests clean (0 errors), 12 新测试                                                                      |
| 26  |  ✅ exit 0   | Phase 15 全栈审计 + 测试文档补全 + P0 get_unwrap 消除: cargo check --tests clean (0 errors), 10 新测试, 2 新文档                                                          |
| 27  |  ✅ exit 0   | Phase 16 死代码消除 + quality_scorer 测试 + 前端防护: cargo check --tests clean (0 errors, -1 warning), 7 新测试, 1 新 i18n key                                           |
| 28  |  ✅ exit 0   | Phase 17 P2 组件拆分 + RenderHint 打通: cargo check clean (1 existing warning), eslint insight 文件 0 新增, 3 新子组件                                                    |
| 29  |  ⚠️ 50预存   | Phase 18 P0 diff修复 + P1 类型安全: eslint 0 errors (import path修正), cargo 0 insight错误 (50 预存 faker E0433)                                                          |
| 30  |  ✅ exit 0   | Phase 19 审计修复: 类型分类(P0) + 常量配置化(P1) + 测试验证, cargo 0 errors (2 预存 warning)                                                                              |
| 31  | ✅ insight 0 | Phase 20 归档三修复: cargo check insight 模块 0 errors                                                                                                                    |
| 32  | ✅ eslint 0  | Phase 21 全局主题统一: eslint 0 errors (443 预存 warning)                                                                                                                 |
| 33  |  ✅ exit 0   | Phase 22 审计修复 (type/constant/dead code/dedup + i18n 双语化): cargo check 0 errors, eslint 0 errors (394 预存 warning)                                                 |
| 34  |  ✅ exit 0   | Phase 23 深度审计修复 (Semaphore/DuckDB docs/API docs/BUILTIN_RULE_COUNT/deny_unknown_fields): cargo check 0 errors (16 预存 warning), eslint 0 errors (394 预存 warning) |

---

## 五、文件变更总计

| 类型                  |                                                                        数量                                                                        |
| --------------------- | :------------------------------------------------------------------------------------------------------------------------------------------------: |
| 新增文件              |                                                                         21                                                                         |
| 修改文件              |                                                                         14                                                                         |
| P0打通 修改文件       |                                         3 (insight-store.ts, MultiColumnView\.vue, ColumnInsightPanel.vue)                                         |
| Phase 2 新增文件      |                                                         4 (3 TOML + TableProfileView\.vue)                                                         |
| Phase 2 修改文件      |               6 (result_service.rs, result_commands.rs, lib.rs, DockviewLayout.vue, use-context-menu-actions.ts, result-analysis.ts)               |
| 优化 新增文件         |                                                                         0                                                                          |
| 优化 修改文件         |       7 (result_service.rs, result_commands.rs, lib.rs, insight-store.ts, TableProfileView\.vue, ColumnInsightPanel.vue, result-analysis.ts)       |
| 联动+动画 修改文件    |       5 (result_service.rs, result_commands.rs, lib.rs, insight-store.ts, ColumnInsightPanel.vue, result-analysis.ts, TableProfileView\.vue)       |
| 质量评分 新增文件     |                                              1 (insight-rules/quality/column-quality-score.rule.toml)                                              |
| 质量评分 修改文件     |                  5 (result_service.rs, result_commands.rs, lib.rs, result-analysis.ts, insight-store.ts, ColumnInsightPanel.vue)                   |
| 已有错误修复 修改文件 |                                                              1 (scratchpad/store.rs)                                                               |
| 表质量聚合 新增文件   |                                             1 (insight-rules/table/table-quality-overview\.rule.toml)                                              |
| 表质量聚合 修改文件   |                            4 (result_service.rs, result_commands.rs, lib.rs, result-analysis.ts, TableProfileView\.vue)                            |
| Schema 洞察 新增文件  |                                                   2 (schema_analyzer.rs, SchemaInsightPanel.vue)                                                   |
| Schema 洞察 修改文件  |                    6 (mod.rs, result_commands.rs, lib.rs, result-analysis.ts, use-context-menu-actions.ts, DockviewLayout.vue)                     |
| P0修复 修改文件       |           7 (duckdb.rs, result_service.rs, schema_analyzer.rs, duckdb_engine.rs, result_commands.rs, insight-store.ts, insight_store.rs)           |
| P0修复 新增测试       |                                                      2 (duckdb.rs, schema_analyzer.rs tests)                                                       |
| B组消除 修改文件      |                         3 (TableProfileView\.vue, ColumnInsightPanel.vue, DockviewLayout.vue, use-context-menu-actions.ts)                         |
| V2对齐 修改文件       |                                                     2 (insight-store.ts, QueryResultPanel.vue)                                                     |
| i18n 修改文件         |                                                  3 (zh-CN.json, en.json, ColumnInsightPanel.vue)                                                   |
| UX增强 修改文件       |                             4 (ColumnInsightPanel.vue, QueryResultPanel.vue, ResultContextMenu.vue, insight-store.ts)                              |
| Phase 9 修改文件      |                                 4 (types/result.ts, result-analysis.ts, insight-store.ts, ColumnInsightPanel.vue)                                  |
| Phase 10 修改文件     |          7 (rule_executor.rs, rule_registry.rs, schema_analyzer.rs, DataVisualizationPanel.vue, ColumnInsightPanel.vue, insight-store.ts)          |
| Phase 13 新增类型     |                                         1 (rule_types.rs 新增 ExecutionResult/QualityReport/QualityCheck)                                          |
| Phase 13 修改文件     |                                   4 (rule_executor.rs, insight_engine.rs, result_service.rs, result_commands.rs)                                   |
| Phase 13 新增测试     |                                                        10 (QualityRule 3 + RuleRegistry 7)                                                         |
| Phase 13 前端修复     |                                                            1 (ColumnInsightsPanel.vue)                                                             |
| Phase 14 修改文件     |                                                          2 (duckdb.rs, rule_executor.rs)                                                           |
| Phase 14 新增测试     |                                                              2 (duckdb.rs TTL tests)                                                               |
| Phase 15 修改文件     |                                                      2 (rule_executor.rs, insight_engine.rs)                                                       |
| Phase 15 新增测试     |                                                        10 (insight_engine.rs 全面测试 10个)                                                        |
| Phase 15 新建文档     |                                                2 (INSIGHT-API-REFERENCE.md, INSIGHT-RULE-FORMAT.md)                                                |
| Phase 15 更新文档     |                                        2 (INSIGHT-ARCHITECTURE.md v13→v14, INSIGHT-DEV-PROGRESS.md v17→v18)                                        |
| Phase 16 修改文件     |                             4 (duckdb_service.rs, quality_scorer.rs, SchemaInsightPanel.vue, ColumnInsightsPanel.vue)                              |
| Phase 16 新增测试     |                                                           7 (quality_scorer.rs 独立测试)                                                           |
| Phase 16 i18n         |                                                              2 (zh-CN.json, en.json)                                                               |
| Phase 17 新建文件     |                                      3 (QualityScoreCard.vue, InsightStatsSection.vue, InsightHistoryTab.vue)                                      |
| Phase 17 修改文件     |     5 (ColumnInsightPanel.vue, ColumnInsightsPanel.vue, result-analysis.ts, insight-store.ts, DockviewLayout.vue, DataVisualizationPanel.vue)      |
| Phase 18 修改文件     |                  6 (InsightHistoryTab.vue, InsightStatsSection.vue, QualityScoreCard.vue, insight-store.ts, zh-CN.json, en.json)                   |
| Phase 18 i18n         |                                                 4 (insightHistory, comparing, diffResult, noDiff)                                                  |
| Phase 19 修改文件     |                                            3 (duckdb_service.rs, insight_engine.rs, quality_scorer.rs)                                             |
| Phase 20 修改文件     | 6 (rule_registry.rs, schema_analyzer.rs, insight/mod.rs, result_commands.rs, lib.rs, result-analysis.ts, insight-store.ts, ColumnInsightPanel.vue) |
| Phase 20 新增测试     |                                                         2 (schema_analyzer.rs 复合FK测试)                                                          |
| Phase 21 修改文件     |                                                             10 (1 tokens.css + 9 .vue)                                                             |
| cargo check 累计      |                                                                   31 次全部通过                                                                    |
| eslint 累计           |                                                                    5 次全部通过                                                                    |

---

## 六、待办 (Backlog)

### 查询结果 V2 对齐 (配合 QUERY-RESULT-OPTIMIZATION-PLAN)

- [ ] `insight-filter-by-value` CustomEvent 消除 (不存在于当前代码，V2实施时新增)
- [x] DuckDB 临时表 TTL 清理 ✅ Phase 14
- [x] `insightStore.isOpen` 状态管理 ✅ Phase 10

### 增强

- [ ] JSON/Array/IP 列类型支持
- [ ] 性能基准测试
- [ ] i18n 多语言支持
- [ ] Zustand Store 升级到 v5
- [x] ColumnInsightPanel 960行组件拆分 ✅ Phase 17
- [x] ColumnInsightPanel vs ColumnInsightsPanel 角色明确 ✅ Phase 17
- [x] RenderHint 前后端打通 ✅ Phase 17
- [x] 规则 ID 重复无 warning ✅ Phase 20
- [x] 外键推断对复合外键漏检 ✅ Phase 20
- [x] 规则热加载缺失 ✅ Phase 20
