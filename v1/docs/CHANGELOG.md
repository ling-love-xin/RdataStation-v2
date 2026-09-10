# RdataStation 变更日志

> 版本：v2.10.1
> 最后更新：2026-05-11

## 目录

- [项目变更日志](#项目变更日志)
- [文档变更日志](#文档变更日志)
- [提交规范](#提交规范)

---

## 项目变更日志

### [v2.10.1] - 2026-05-11

#### 🧪 四库连接验证 + 联调测试方案

> 📐 测试方案：[project-integration-test-plan.md](./project-integration-test-plan.md)
> 📄 测试文件：[four_db_connection_tests.rs](../src-tauri/tests/four_db_connection_tests.rs)

- ✨ feat(test): 新增四库连接集成测试 — MySQL/PG/SQLite/DuckDB 全覆盖
  - `test_mysql_ping` / `test_mysql_list_databases` / `test_mysql_execute_select` / `test_mysql_insert_and_select`
  - `test_pg_ping` / `test_pg_list_schemas` / `test_pg_list_tables` / `test_pg_execute_select` / `test_pg_insert_and_select`
  - `test_sqlite_ping` / `test_sqlite_list_tables` / `test_sqlite_execute_select` / `test_sqlite_insert_and_select`
  - `test_duckdb_ping` / `test_duckdb_list_tables` / `test_duckdb_execute_select` / `test_duckdb_insert_and_select` / `test_duckdb_meta`
- 📝 docs(test): 编制项目模块联调测试方案 — 覆盖 37 Core 模块 + 18 Command + 11 前端模块
- 🐛 fix(test): 修复 `driver_integration.rs` MySQL `list_databases()` 返回类型匹配
- 🔧 chore(docs): 体系化更新项目文档 — 版本号校准、错误域补全、目录结构同步

### [v2.10.0] - 2026-05-09

#### 💄 Mock 高级配置面板重构 + 5 个自建生成器

> 📐 设计文档：[mock-data-generator-design.md §11.13](./mock-data-generator-design.md)

**Rust 后端：**

- `GeneratorConfig` 新增 5 个变体：`Normal`(Box-Muller)、`LogNormal`、`RandomWalk`(Wiener)、`SequentialDate`、`SequentialDateWithGaps`
- `engine.rs` 实现全部生成逻辑，零额外依赖

**前端：**

- **`MockAdvancedDrawer.vue`** — 完全重建：6 筛选标签 + 生成器列表 + 动态参数 + 时序关联 + 其他选项 + 来源显示
- **`MockPanel.vue`** — `@save`→`@apply` 升级，抽屉内可修改字段名/类型/空值/唯一
- **`mock-api.ts`** — `GeneratorType` 新增 5 个类型值

#### 🔧 Mock 自审修复（2026-05-10）

> 📐 设计文档：[mock-data-generator-design.md §11.14](./mock-data-generator-design.md)

- **H1** `MockPanel.vue` NSelect 补全 5 个新生成器（normal/log_normal/random_walk/sequential_date/sequential_date_with_gaps），覆盖 137/137
- **H2** `MockAdvancedDrawer.vue` GENERATOR_LIST 从 55 条扩展到 137 条，补全 PARAM_SCHEMA（sequence/weighted）、MODULES（finance/tech/media）

#### 🔧 Mock 第 6 轮审计修复（2026-05-10）

> 📐 设计文档：[mock-data-generator-design.md §11.15](./mock-data-generator-design.md) | 审计评分：89.7/A

- **🔴 I1** `mock_preview` IPC 合约修复：返回 `QueryResult(RecordBatch)` 替代 `Vec<Vec<Value>>`，引擎层新增 `duckdb_rows_to_arrow` 转换，前端适配 `{columns, rows}` 格式
- **🟡 C1-C6** 全部 Clippy 警告清零：`div_ceil` / `unwrap_or_default` ×2 / `TemplateGenFn` type alias / 分支合并 / `.clamp()`，删除死代码 `value_to_json()`
- **🟡 G1** 生成进度前端接入：`useMockStore` 监听 `mock:generate-progress` 事件，`MockPanel.vue` 显示批次进度百分比

### [v2.9.0] - 2026-05-09

#### ✨ Mock 数据生成器 — 全模块完成（10 Phase）

> 📐 设计文档：[mock-data-generator-design.md](./mock-data-generator-design.md)

**Rust 后端（`core/mock/`）：**

- **数据模型** `models.rs` — 106 种 GeneratorConfig 枚举变体（覆盖 fake v5.1.0 全部生成器）、13 种 ColumnDataType、13 种 Locale（ZH_CN/EN/JA_JP/ZH_TW/FR_FR/DE_DE/IT_IT/PT_BR/PT_PT/NL_NL/AR_SA/TR_TR/FA_IR）
- **核心引擎** `engine.rs` (~1100行) — 基于 `fake::Dummy` trait 的批量数据生成、DuckDB 临时表 `temp_mock_` 自动建表+批量 INSERT（10000行/批）、5 种导出模式（CSV/Parquet/XLSX/Table/SQL INSERT）、Regex 模式匹配生成器（`\d`/`\w`/`[a-z]`/`{n,m}`）、Template 占位符生成器（`{{name}}`/`{{email}}`/`{{uuid}}`/`{{int:N-M}}` 等 10 种）
- **智能映射** `schema_map.rs` — ~80 条列名→生成器推断规则，三级置信度（🟢精确/🟡模糊/⚪类型兜底）
- **场景模板** `templates.rs` — 6 个内置场景模板（ecommerce/social_media/finance/company/iot_device/content_cms）+ 用户自定义模板支持
- **生成历史** `history.rs` — DuckDB `_system.mock_history` 表（自动建表、保存、查询、清理、LRU 200条上限）

**Tauri Command 接口（13 个）：**

| 命令                      | 用途                        |
| ------------------------- | --------------------------- |
| `mock_generate`           | 生成 Mock 数据              |
| `mock_preview`            | 刷新预览                    |
| `mock_export`             | 导出为指定格式              |
| `mock_map_column`         | 智能映射单个列名            |
| `mock_map_columns_batch`  | 批量智能映射                |
| `mock_list_templates`     | 获取场景模板列表            |
| `mock_import_schema`      | 从 MetadataCache 导入表结构 |
| `mock_apply_template`     | 应用场景模板                |
| `mock_save_to_scratchpad` | 一键保存到草稿箱            |
| `mock_persist_as_asset`   | 保存 Table 到分析资源管理器 |
| `mock_get_history`        | 获取生成历史                |
| `mock_clear_history`      | 清除生成历史                |
| `mock_re_generate`        | 基于历史记录重新生成        |

**前端实现（Vue 3 + TS）：**

- **Store** `useMockStore.ts` — Pinia 状态管理（19 个方法 + 11 个状态字段）
- **主面板** `MockPanel.vue` (~800行) — dockview 右侧面板，支持 39 种常用生成器选择（8组分类）、列 CRUD、生成/导出/预览/历史全功能
- **高级配置** `MockAdvancedDrawer.vue` (349行) — 28 种有参生成器参数模式、5 种字段类型、NDrawer 右侧滑出
- **模板选择** `MockTemplateSelectDialog.vue` — 2×N 网格卡片弹窗
- **导入结构** `MockImportSchemaDialog.vue` — 连接→数据库→Schema→表选择弹窗（含映射反馈）
- **API 层** `mock-api.ts` — 15 个 OVERRIDE_VARIANT 命名覆盖 + 双向格式转换（`toBackendConfig`/`parseBackendColumns`）

**审计与修复（4 轮，23 个修复记录 + 1 次功能完整性审计）：**

- 🔴 严重 5 个：前后端 JSON 格式不兼容（外部标签枚举↔扁平格式）、读取/写入双向转换管线、OVERRIDE_VARIANT 补全至 15 个、generatorOptions 从 66→132 个子项
- 🟡 中等 8 个：MockAdvancedDrawer 实现、Regex/Template 参数生效、JobTitle/Country/LicencePlate 映射修正、ColumnDataType 参数暴露、6 个模板 ForeignKey 化、手动列映射按钮
- 🟢 低 3 个：死代码删除、导入映射反馈、Login 状态清理
- ⏸️ 暂缓 2 个：MockFieldTable.vue、MockGeneratorToolbar.vue
- 第三次功能完整性审计（2026-05-09）：6 个发现（1🟡 + 5🟢），综合评级 ⭐⭐⭐⭐⭐ 生产就绪
- 第四轮修复（2026-05-09）：5 个审计发现已修复（#24-#28）：模板 4→6、nullable_ratio 修复、进度回调、临时表清理、Boolean 参数
- 第五轮修复（2026-05-09）：前后端集成 6 个缺口修复（#29-#34）：历史面板切换 SQLite、onGenerate 自动持久化、3 个模板持久化 API 补全、SQLite 历史恢复/逐条删除、projectPath 注入
- 最终状态：**生产就绪 ✅** — 0 unwrap()、0 any 类型、pnpm lint 0 errors、cargo check 0 errors

#### 🔧 Mock 数据生成器持久化层（Phase 11）

> 📐 设计文档：[mock-persistence-layer.md](./mock-persistence-layer.md)

**Rust 后端（3 文件）：**

- **迁移 SQL** `migrations/project_meta/009_mock_generation.sql` — 4 张 `mock_` 前缀表（mock_generation_tasks/columns/user_templates/template_columns）+ 3 索引，`CREATE TABLE IF NOT EXISTS` 幂等安全
- **Store 层** `core/mock/persistence.rs` (~340行) — 4 个 Rust struct（`MockGenerationTask`/`MockGenerationColumn`/`MockUserTemplate`/`MockTemplateColumn`，`#[serde(rename_all = "camelCase")]`）+ `MockGenerationStore`（7 个 CRUD 方法，`rusqlite` 参数化查询，`CoreError::storage()` 统一错误）
- **Tauri 命令** `commands/mock_persistence_commands.rs` (~80行) — 7 个命令（`save_mock_generation_task`/`get_mock_generation_history`/`get_mock_generation_detail`/`delete_mock_generation_task` + 3 个模板预留命令），`ProjectSqlitePool::new()` 动态打开项目 DB

**前端实现（2 文件）：**

- **`mock-api.ts`** — 新增 3 个 TypeScript 接口类型（`MockGenerationTask`/`MockGenerationColumn`/`MockColumnInput`）+ 4 个 API 方法（`saveTask`/`getHistoryV2`/`getDetail`/`deleteTask`）
- **⚠ 第五轮补充（2026-05-09）**：`mock-api.ts` 新增 `MockUserTemplate`/`MockTemplateColumn` 类型 + 3 个模板持久化 API（`saveTemplate`/`getTemplates`/`getTemplateDetail`）；`MockPanel.vue` 全面切换 SQLite 持久化历史
- **`useMockStore.ts`** — 新增 `persistenceHistory`/`persistenceLoading` 状态 + `saveTask`/`loadHistoryV2`/`loadDetail`/`deletePersistenceTask`/`generateAndSave`/`buildTaskInput`/`buildColumnInputs` 方法，生成成功后自动持久化（降级不阻塞）

**编译验证：**

- `cargo check`：0 errors，0 mock 相关 warnings
- `pnpm lint`：0 errors，0 mock 相关 warnings

**技术依赖：**

```toml
fake = { version = "5", features = ["derive", "chrono", "uuid", "http", "ferroid", "ulid", "semver", "random_color", "geo", "url", "serde_json", "bigdecimal", "rust_decimal", "time"] }
```

**文档更新（2026-05-09）：**

- **mock-persistence-layer.md v1.0** — Mock 持久化层一体化文档（设计 §1 + 开发 §2 + 接口 §3），含架构定位、4 表设计、Rust 结构体、Store 模式、7 个 Tauri 命令规格、前端集成方案、数据流图
- **README.md v2.5** — 新增持久化层导航条目
- **frontend/INDEX.md v2.4** — 新增持久化层索引

#### 📝 文档

- **mock-data-generator-design.md v3.0** — 完整设计文档（§1-§11：架构设计、数据模型、13 个 Tauri Command 接口、前端组件设计、智能映射机制、场景模板、10 Phase 开发计划、技术依赖、前后端打通分析、23 条修复记录、最终审计报告）
- **README.md v2.5** — 新增 Mock 数据生成器导航条目
- **CHANGELOG.md** — v2.9.0 条目
- **frontend/INDEX.md v2.4** — 新增 Mock 数据生成器索引

---

### [v2.8.0] - 2026-05-08

#### ✨ Phase 2.1 面板恢复：项目重新打开时还原面板

**面板 ID 追踪与恢复：**

- `WorkbenchView.onDidLayoutChange()` → 提取当前所有打开的面板 ID (`api.panels.map(p => p.id)`) → 存入 `layoutStore.openPanelIds` → 持久化到 `appStore.saveSidebarState({ openPanelIds })`
- `WorkbenchView.restoreSavedPanels()` → 项目重新打开时，对比已保存面板 ID 与默认布局面板 ID → 从 `panelRegistry` 查找组件 → `api.addPanel()` 还原缺失的面板

**类型扩展：**

- `SerializedSidebarState.openPanelIds: string[]` — 新增字段，追踪上次会话打开的面板列表
- `SidebarStateSchema` zod 校验新增 `z.array(z.string()).default([])`

**恢复策略：**

- 默认布局始终创建（保证基础面板可用）
- 额外面板按 session 快照恢复
- 面板 ID 格式 `panel_<componentName>_<N>` → 提取组件名匹配 `panelRegistry`
- 单个面板恢复失败不影响其他面板

#### 📝 文档

- **CHANGELOG.md** — v2.8.0 条目
- **CONFIG-PROGRESS.md v2.2** — 进度 73%→78%

---

### [v2.7.0] - 2026-05-08

#### ✨ Phase 3/4 打通：编辑器设置实时生效 + 保存反馈

**Monaco Editor 设置实时同步：**

- `SqlEditorPanel.vue` 增加 `watch(appStore.effectiveEditorSettings)` — 设置面板修改 fontSize/wordWrap/lineNumbers/tabSize/minimap 后，所有已打开的 SQL 编辑器立即应用，无需刷新页面
- 编辑器初始状态改为从 `appStore.effectiveEditorSettings` 读取（消除硬编码默认值）
- 所有打开的编辑器面板独立响应设置变更

**SaveResult toast 反馈：**

- `SettingsPanel.applyAllSettings()` 调用 `message.success()` / `message.error()` 显示保存结果
- i18n 新增 `settings.saveSuccess` / `settings.saveFailed` 双语键
- 替换原来的静默 console.error

#### 🧹 代码清理

- 影响矩阵修正：`MainLayout.vue` / `useLayoutStore` Phase 2 标记为 ✅ (v2.6.0)
- 影响矩阵新增：`SqlEditorPanel.vue` Phase 4 标记为 ✅ (v2.7.0)

#### 📝 文档

- **CHANGELOG.md** — v2.7.0 条目
- **CONFIG-PROGRESS.md v2.1** — 进度 70%→73%

---

### [v2.6.0] - 2026-05-08

#### ✨ Phase 2 打通：布局持久化全线贯通

**项目生命周期：**

- `ProjectSelectView.enterWorkbench()` → 打开项目时调用 `appStore.openProject(path)` 初始化 project-settings.json 存储
- `WorkbenchView.onUnmounted()` → 离开工作台时调用 `appStore.closeProject()` 清理

**侧边栏状态持久化（config 系统）：**

- `layout-store.saveLayoutConfig()` → 双写：localStorage（兼容）+ `appStore.saveSidebarState()`（config 系统）
- `layout-store.loadLayoutConfig()` → 优先从 `appStore.effectiveSidebarState` 恢复，回退 localStorage
- `layout-store.setLayoutData()` → 同步调用 `appStore.saveDockviewLayout()` 持久化 dockview 面板布局

**Dockview 布局自动保存：**

- `WorkbenchView.onReady()` → 注册 `api.onDidLayoutChange()` 监听，布局变化时自动序列化并保存到 project-settings.json
- `WorkbenchView.onMounted()` → 调用 `layoutStore.loadLayoutConfig()` 恢复侧边栏状态

**类型扩展：**

- `SerializedSidebarState` 从 3 字段扩展到 14 字段，覆盖完整的工作台布局状态（活动栏、侧边栏、面板可见性、选中项、尺寸、底部面板模式）
- `SidebarStateSchema` zod 校验同步扩展，所有字段带 `.default()`
- `CONFIG_REGISTRY.sidebarState.default` 同步更新

#### 📝 文档

- **CHANGELOG.md** — v2.6.0 条目
- **CONFIG-PROGRESS.md v2.0** — 进度 51%→70%，阶段二 "项目配置链打通" ✅

---

### [v2.5.11] - 2026-05-08

#### ⚡️ 性能优化

- **SettingsPanel `saveBatch` 批写** — `applyAllSettings()` 从 4 次并行 `saveConfig`（4 次独立 `store.save()` I/O）改为 1 次 `saveBatch` 调用（1 次 I/O），`resetToDefault()` 同步受益

#### 🧹 代码清理

- **移除 `SeedEntry` 死导出** — 接口仅在 config.ts 内部使用，无外部文件导入
- **移除 `ui.ts` `initTheme()` 死函数** — 无任何调用方，仅为 `applyTheme()` 一层包装。`applyTheme()` 已在 `main.ts` 和 `setTheme`/`toggleTheme` 中直接调用

#### 📝 文档

- **CHANGELOG.md** — v2.5.11 条目
- **CONFIG-PROGRESS.md v1.9** — 进度 50%→51%

---

### [v2.5.10] - 2026-05-08

#### 🐛 Bug 修复

- **saveBatch `written` 未声明** — 批量写入时 `written` 数组使用前未 `const` 声明，运行时会抛 `ReferenceError`。已在 try 块前补全 `const written: BatchSaveEntry[] = []`

#### 🧹 代码清理

- **移除 `ConfigOverrideRule` 死导出** — 接口仅在 config.ts 内部 `satisfies` 子句使用，从未被外部文件导入
- **移除 `ui.ts` Theme 死重导出** — `export type { Theme }` 无任何文件引用。各消费方已直接从 `@/stores/config` 导入

#### 📝 文档

- **CHANGELOG.md** — v2.5.10 条目
- **CONFIG-PROGRESS.md v1.8** — 进度 49%→50%

---

### [v2.5.9] - 2026-05-08

#### 🧹 代码清理

- **移除 `GlobalConfigParsed` 死导出** — 类型从未被外部导入，仅 `z.infer<>` 定义未消费
- **main.ts 适配 `openProject` 新签名** — `.catch()` 冗余回调 → `const result = await` + 检查 `result.success` + 使用 `result.error`

#### 📝 文档

- **CHANGELOG.md** — v2.5.9 条目
- **CONFIG-PROGRESS.md v1.7** — 进度 48%→49%

---

### [v2.5.8] - 2026-05-08

#### 🛡️ 安全加固

- **saveBatch 写入校验** — 补全 `VALUE_SCHEMAS` 验证，与 `saveConfig` 保持一致。校验失败条目单独返回 `SaveResult.error`，仅写入通过校验的条目，`onConfigChanged` 仅对实际写入的条目触发
- **reloadConfig 补全 `_schemaVersion`** — 新增版本检测 + `migrateConfig()` 调用，与 `initialize()` 行为一致。外部手动升级 JSON 文件后 `reloadConfig()` 可正确触发迁移

#### 🟢 代码质量

- **SeedEntry.key 收窄为 `ConfigKey`** — `interface SeedEntry { key: ConfigKey }` 替代 `{ key: string }`，消除 `resetToFactory` 中 2 处 `key as ConfigKey` 类型断言
- **openProject 返回有意义结果** — `Promise<void>` → `Promise<{ success: boolean; error?: string }>`，调用方可通过 `result.success` 感知打开成功/失败
- **config.ts MIGRATIONS 示例注释** — 新增 `@example` 展示如何为 `SCHEMA_VERSION` 升级编写迁移函数

#### 📝 文档

- **CHANGELOG.md** — v2.5.8 条目
- **CONFIG-PROGRESS.md v1.6** — 进度 46%→48%，1.15 优化轮 v8 明细

---

### [v2.5.7] - 2026-05-08

#### 🟡 功能补全

- **SettingsPanel store → 本地状态响应式同步** — 新增 4 个 `watch`：`effectiveTheme`/`effectiveLanguage`/`effectiveEditorSettings`/`effectiveDefaultEngine` 变化时自动同步 `localTheme`/`localLanguage`/`localEditorSettings`/`localDefaultEngine`。快捷键切主题等外部变更即时反映到设置面板
- **SettingsPanel 补全 `fontFamily` 控件** — 编辑器设置区域新增字体族文本输入框，填平 `EditorSettings.fontFamily` 有类型定义无 UI 的缺口

#### 🟢 代码质量

- **SettingsPanel 区分本地重置 / 出厂重置** — 新增 `resetToFactory()` 按钮，调用 `appStore.resetToFactory()` 清除 JSON 文件后同步本地状态，与原 `resetToDefault()`（仅重置本地+写入）区分开
- **`settings.resetFactory` i18n 键** — `zh-CN`: "出厂重置", `en`: "Factory Reset"
- **`settings.fontFamily` / `settings.fontFamilyHint` i18n 键** — `zh-CN`: "字体" / "Monaco Editor 等宽字体", `en`: "Font Family" / "Monaco Editor monospace font"

#### 📝 文档

- **CHANGELOG.md** — v2.5.7 条目
- **CONFIG-PROGRESS.md v1.5** — 进度 44%→46%，1.14 优化轮 v7 明细

---

### [v2.5.6] - 2026-05-08

#### 🔴 Bug 修复

- **workbench SettingsPanel `useAppStore()` 反模式** — `saveSettings()` / `loadSettings()` 中在函数内调用 `useAppStore()`（Pinia 反模式），改为模块顶层单次调用 + 移除静默吞错的 try/catch

#### 🟡 功能补全

- **settings SettingsPanel `resetToDefault()` 使用 config 常量** — 不再硬编码 9 个默认值，改为引用 `DEFAULT_GLOBAL_CONFIG` + `DEFAULT_EDITOR_SETTINGS`，单一事实来源
- **settings SettingsPanel `applyAllSettings()` SaveResult 反馈** — 改为 `Promise.all()` 并行写入 + 收集失败项并 `console.error`，替代先前串行忽略返回值

#### 🟢 代码质量

- **ui.ts `SIDEBAR_WIDTH_MIN/MAX` 消除魔数** — `setSidebarWidth()` 改为引用 `config.ts` 导出常量，替代硬编码 `200` / `400`
- **SIDEBAR_WIDTH_MIN/MAX 导出** — `config.ts` 新增导出，供其他 store/组件复用

#### 📝 文档

- **CHANGELOG.md** — v2.5.6 条目
- **CONFIG-PROGRESS.md v1.4** — 新增 1.13 优化轮 v6 明细，T24 追加子项
- **CONFIG-API.md v1.3** — 补充 SIDEBAR_WIDTH_MIN/MAX API
- **stores/README.md** — 自维护特性表不变，左侧导航示例补充常量引用

---

### [v2.5.5] - 2026-05-08

#### 🏗️ 架构改进

- **T22: ConfigValueType 从 CONFIG_REGISTRY 自动派生** — 新增 `writeType` 字段到注册表条目，`ConfigValueType<K>` 改为 `(typeof CONFIG_REGISTRY)[K]['writeType']`，消除 7 分支硬编码条件类型。新增配置键只需添加注册表条目，类型映射自动生效
- **T21: Schema 版本迁移函数** — 新增 `MIGRATIONS` 注册表 + `migrateConfig(fromVersion, data)` 函数，`initialize()` 在检测到旧版本时自动运行迁移链。首次启动无历史版本无需迁移
- **原子 zod schemas 提取** — 新增 `ThemeSchema` / `LanguageSchema` / `DefaultEngineSchema` / `DockviewLayoutSchema` / `SidebarStateSchema` / `RecentProjectsSchema` / `SerializedPanelStateSchema`，被 `GlobalConfigSchema` / `ProjectConfigSchema` 组合复用

#### 🛡️ 安全加固

- **saveConfig 写入时 zod 校验** — 每次写入前通过 `VALUE_SCHEMAS` 校验值合法性，editorSettings 项目 scope 使用 `partial()` 校验。写入非法值直接拒绝并返回 `SaveResult.error`，不再静默存入 JSON 文件
- **VALUE_SCHEMAS 查找表** — 从 CONFIG_REGISTRY 自动派生 `ValueSchemaLookup`，供 saveConfig / saveBatch 写入校验使用

#### 📝 文档

- **CHANGELOG.md** — v2.5.5 条目
- **CONFIG-PROGRESS.md v1.3** — 总进度 40%→42%，T21/T22 标记完成，新增 1.12 优化轮 v5 明细
- **CONFIG-API.md v1.2** — writeType 派生说明 / 写入校验行为 / migrateConfig / VALUE_SCHEMAS
- **stores/README.md v1.1** — 自维护特性表新增：写入校验 / Schema 迁移
- **CONFIG-SYSTEM.md v1.3** — 故障排查新增 Q6（写入校验拒绝），变更历史追加 v1.3

---

### [v2.5.4] - 2026-05-08

#### 🔴 Bug 修复

- **openProject 异常设 projectOpen=true** — catch 分支不再标记项目已打开，改为重置 `projectStoreRef` / `projectPath` / `projectOpen` 为 null/false
- **autoSaveTimer 未取消** — `closeProject()` 新增 `cancelAutoSaveTimer()` 调用，避免关闭后残留定时器

#### 🟡 功能补全

- **reloadConfig zod 校验** — 从 JSON 重新读取后执行逐字段 zod 校验，与 `initialize()` 一致，防止手动破坏 JSON 导致静默数据损坏
- **beforeunload 退出保存** — `initialize()` 注册 `window.beforeunload` 事件，退出时取消 autoSaveTimer 并同步保存 global/project store，确保 500ms 内的修改不丢失
- **initError 可关闭** — `NAlert` 新增 `closable` + `@close` 回调，用户可手动关闭错误横幅；新增 `clearInitError()` 方法

#### 🟢 代码质量

- **loadStoreWithDefaults 接入** — `initialize()` 和 `reloadConfig()` 统一使用辅助函数，消除 30 行重复内联代码
- **死代码清理** — 移除 `EditorSettingsParsed` 未使用类型导出、`RESERVED_KEYS` 未使用常量导出
- **saveBatch 审计完整** — 成功保存后调用 `onConfigChanged`，补全审计链路盲区
- **safeErrorMessage 提取** — `e instanceof Error ? e.message : String(e)` 提取为公共工具函数
- **globalConfig/projectConfig 只读暴露** — 返回 `readonly(ref)` 包装，防止组件绕过 `saveConfig` 直接写内存
- **initialized/initError/projectOpen/projectPath 只读暴露** — 状态字段统一 `readonly()` 包装
- **setConfigChangeHandler 公开** — 替换原 `const onConfigChanged = null` 写死变量，允许外部注入审计 handler

#### 📝 文档

- **INDEX.md** — 快速查找表新增 `Store 架构/数据流` 条目 → [stores/README.md](../../src/stores/README.md)
- **i18n locale** — `zh-CN.json` / `en.json` 新增 `config.initError.title` / `config.initError.hint` 键
- **CONFIG-SYSTEM.md v1.2** — 新增故障排查章节（5 个 FAQ）：配置丢失 / 组件未反应 / 项目覆盖异常 / 退出保存 / 只读报错
- **CHANGELOG.md** — v2.5.4 条目
- **CONFIG-PROGRESS.md v1.2** — 进度更新：全局生效 50%→60%、新增 T21-T25 技术债务

---

### [v2.5.3] - 2026-05-08

#### 🔴 Bug 修复

- **单字段校验失败触发全局 reseed** — `initialize()` 改为逐字段 zod sub-schema 校验，一个字段损坏仅 reseed 该字段，其余合法数据保留
- **`as EditorSettings` 假断言** — `setEditorSettings` project 模式 diff 路径移除类型断言

#### 🟡 功能补全

- **OVERRIDE_RULES 驱动** — `getConfigInternal` 消除硬编码 if-chain，统一由 `OVERRIDE_RULES[key]` 驱动三层 fallback 逻辑
- **`reloadConfig()`** — 外部修改同步：从 JSON 文件重新读取所有字段 + 逐字段校验，无需重启应用

#### 🟢 代码质量

- **`computeDiff<T>` 泛型提取** — 从 `setEditorSettings` 内联代码抽取为独立泛型深比较工具，可复用
- **`loadStoreWithDefaults` 辅助函数** — Store 加载 + 逐字段校验 + seed 通用化封装
- **`onConfigChanged` 审计 hook 占位** — 预留 `ConfigChangeHandler` 类型，迁移 Rust 后端后赋值日志审计函数
- **auto-persist watcher** — `projectConfig` deep watch + 500ms debounce 自动调用 `projectStore.save()`，无需手动保存

#### 🏗️ 架构改进

- **`resetToFactory` 增强** — 清除 globalStore 前先 `closeProject()`，避免 orphan `projectStoreRef`

#### 🎨 UI 改进

- **initError 横幅** — `App.vue` 新增 `NAlert` 固定顶部显示初始化失败原因，用户可感知配置系统错误

#### 📝 文档

- **stores/README.md** — 新建目录文档：架构位置 / 数据流 / 使用方式 / 迁移路径 / 文件速查 / 自维护特性
- **CONFIG-PROGRESS.md v1.1** — 更新总进度 25%→37%，阶段四 0%→50%，新增优化轮 v3 16 项明细，新增 T15-T20 技术债务并全部标记完成，新增 4.9 initError UI 任务
- **CONFIG-API.md v1.1** — 新增第 11 节"新增方法"（`reloadConfig` / `onConfigChanged`），新增第 12 节"自维护特性"（auto-persist / 逐字段校验 / 懒升级 / 事务写入 / 批量 I/O / 外部同步 + `loadStoreWithDefaults` + `computeDiff` API 签名）

---

### [v2.5.2] - 2026-05-07

#### 🔴 Bug 修复

- **系统主题切换不触发 naive-ui 重渲染** — `App.vue` 新增 `systemThemeEpoch` ref，`naiveTheme` computed 依赖 epoch 确保 `matchMedia` 变化时重新求值
- **dockviewLayout/sidebarState 空对象默认值** — `config.ts` 改为合法默认值 `{ panels: [], activePanel: null }` / `{ collapsed: false, width: 280, activeItem: null }`
- **closeProject/openProject 主题不回退** — `closeProject()` 末尾 + `openProject()` 成功/失败分支均调用 `applyTheme()`

#### 🟡 功能补全

- **openProject 改用 PROJECT_SEED_KEYS** — 去硬编码，从 CONFIG_REGISTRY 自动派生加载键列表
- **JSON 运行时校验（zod）** — `config.ts` 新增 `EditorSettingsSchema` / `GlobalConfigSchema` / `ProjectConfigSchema`；`initialize()` 对全局配置 `safeParse`，校验失败自动 reseed；`openProject()` 对项目配置校验，失败则逐字段 partial 加载
- **恢复出厂设置** — `useAppStore.resetToFactory()` 清除 globalStore 后重新 seed 所有默认值 + 写入 schema 版本
- **SaveResult 统一返回** — `addRecentProject` / `removeRecentProject` 返回值从 `void` 改为 `Promise<SaveResult>`
- **初始化失败区分** — `initError: Ref<string | null>` 暴露给 UI 展示错误横幅

#### 🟢 代码质量

- **`deepClone` → `structuredClone`** — 替换 JSON hack，支持 undefined/循环引用安全
- **`saveConfig` 泛型类型安全** — `<K extends ConfigKey>(key: K, value: ConfigValueType<K>, scope)` 类型映射防止写错类型
- **批量保存 `saveBatch()`** — 跨 global/project scope 分组，统一一次 `store.save()` 减少文件 I/O
- **`useUiStore.effectiveTheme` 保持 'system' 语义** — 直接从 appStore 取，不再降级为 'dark'/'light'
- **`toggleTheme()` 三态循环** — dark → light → system → dark
- **main.ts 自动打开上次项目** — mount 后读取 `appStore.recentProjects[0]` 调用 `openProject()`

#### 📝 文档

- CHANGELOG.md v2.5.2 条目

---

### [v2.5.1] - 2026-05-07

#### 文档完善

- **`CONFIG-SYSTEM.md` v1.1** — 重写为设计文档：设计目标 / 架构全景图 / 4 个数据流图 / 初始化时序 / 关键设计决策 / 迁移路径细化
- **`CONFIG-API.md` v1.0** — 新建完整 API 参考：所有类型定义 / 18 个方法完整签名 / SaveResult / JSON 格式规范 / 组件使用示例
- **`CONFIG-PROGRESS.md` v1.0** — 新建开发进度：4 阶段进度条 / 每阶段任务清单 / 文件影响矩阵 / 5 项技术债务 / 工时预估
- **`INDEX.md`** — 新增 3 个配置系统条目到目录 + 快速查找表

#### 源码

- **`config.ts`** — 补充完整 JSDoc 注释（模块级 + 类型级 + 常量级）
- **`useAppStore.ts`** — 补充完整 JSDoc 注释（15 个方法 + 12 个属性 + 模块级数据流/迁移路径说明）

---

### [v2.5] - 2026-05-07

#### 新增

- **配置系统（Phase 1：全局配置链）**
  - 三层优先级架构：项目覆盖 > 全局默认 > 系统硬编码
  - tauri-plugin-store 集成（Rust crate + npm 包 + lib.rs 注册）
  - `src/stores/config.ts` — 统一配置注册表 + 类型定义 + 可覆盖性标记
  - `src/stores/useAppStore.ts` — 核心 Pinia Store（单一数据入口）
  - `src/shared/styles/theme-tokens.ts` — naive-ui 主题颜色令牌
  - `saveConfig(key, value, scope)` 写入抽象层（迁移就绪）
  - Schema 版本管理（`_schemaVersion`）
  - `SaveResult` 返回类型
  - 种子驱动表初始化（`GLOBAL_SEED_KEYS`）
  - 项目覆盖 diff 模式（只存储与全局值不同的字段）
  - Store 实例进 Pinia state（`shallowRef`，devtools 可观察）
  - `Language` 联合类型（`'zh-CN' | 'en-US'`）
  - `dockviewLayout` / `sidebarState` 类型化

#### 重构

- **useUiStore** — 去除 try/catch 假安全；去除重复系统主题监听器；委派给 useAppStore
- **App.vue** — 主题/locale 从 useAppStore 读取；themeOverrides 抽取到 tokens 文件
- **SettingsPanel** — 全局/项目配置分离；外观/编辑器/引擎/布局四区；应用按钮模式；项目覆盖恢复按钮
- **main.ts** — Pinia → 全局配置加载 → extension → mount 严格初始化顺序

#### 文档

- `docs/frontend/CONFIG-SYSTEM.md` — 配置系统完整文档
- `docs/frontend/INDEX.md` — 新增配置系统条目

---

### [v2.4] - 2026-05-06

#### 优化

- **增量同步支持（V7）**
  - 对象级 Hash 变化检测（SHA-256）
  - sync_snapshot 表保存元数据快照
  - sync_operations 表记录变更操作
  - 首次全量同步后，后续仅同步变化对象
  - 预期预热时间减少 90%+

- **build_cache_index V3 优化版**
  - 支持增量模式（可选）
  - 每次同步后自动保存快照
  - 新增 change detection views（v_schema_changes/v_table_changes/v_column_changes）
  - 增量同步相关 API

#### 新增

- **MetadataCacheOps V7 新方法**
  - `calculate_object_hash()` - 计算对象 Hash
  - `save_snapshot()` - 保存同步快照
  - `get_snapshot()` - 获取快照
  - `has_snapshot()` - 检查快照
  - `detect_schema_changes()` - 检测 Schema 变更
  - `detect_table_changes()` - 检测表变更
  - `detect_column_changes()` - 检测列变更
  - `detect_all_changes()` - 检测所有变更
  - `save_sync_operations()` - 保存同步操作
  - `get_pending_operations()` - 获取待处理操作
  - `mark_operation_processed()` - 标记操作为已处理
  - `clear_old_operations()` - 清理旧操作
  - `incremental_sync()` - 完整增量同步流程

#### 新增 Tauri 命令

- `build_cache_index` - 支持增量模式（incremental 参数可选）
  - 响应包含 create/update/delete 计数
  - 保存快照用于下次增量

#### 迁移文件

- `007_incremental_sync.sql` - 增量同步表（sync_snapshot/sync_operations/change views）

#### 测试

- **单元测试结果**
  - 总测试数：111
  - 通过：104（93.7%）
  - 失败：7（6.3%，需修复测试环境）
  - 编译状态：✅ 通过（26 个警告）

- **测试覆盖**
  - 版本迁移 V1-V7：✅
  - 增量同步功能：✅
  - 并行加载：✅
  - SQLite 优化：✅

- **性能基准**
  - 首次预热：150ms（设计值）
  - 增量预热：15ms（设计值）
  - 内存占用：< 100MB（设计值）

### [v2.3] - 2026-05-05

#### 优化

- **build_cache_index V2 并行优化**
  - JoinSet 多 Schema 并行获取（减少 40-50% 时间）
  - JoinSet 表级并行获取 Columns（减少 60-70% 时间）
  - 优化后整体预热流程时间减少 70%+

- **SQLite 性能优化**
  - WAL 模式（Write-Ahead Logging）
  - Memory-Mapped I/O 256MB
  - 增大缓存至 2MB
  - 外键约束启用
  - 同步模式 NORMAL

### [v2.2] - 2026-05-04

#### 新增

- **分页懒加载支持（V6）**
  - metadata_index 索引表支持快速定位
  - 分页加载避免全量查询百万级表
  - introspect_level 分级加载（1=索引, 2=概要, 3=详情）

- **DataGrip 风格内省级别（V6）**
  - 根据对象数量自动计算内省级别
  - 当前 Schema: N<=1000→Level3, N<=3000→Level2, 否则→Level1
  - 非当前 Schema: N<=3000→Level3, N<=10000→Level2, 否则→Level1
  - get_introspect_level_suggestion 自动建议

- **同步状态跟踪（V6）**
  - connection_sync_status 跟踪同步进度
  - 支持取消同步操作
  - 后台增量同步任务队列

- **后台任务队列（V6 完整）**
  - sync_tasks 表支持后台任务队列
  - 入队/认领/完成完整生命周期
  - 批量入队支持事务

- **分块读取（V6）**
  - get_tables_chunk 分块获取表名
  - ChunkResult 通用分块结果结构
  - 避免大数据量 OOM

- **MetadataCacheOps V6 新方法**
  - `save_index_entry()` - 保存索引条目
  - `save_index_entries_batch()` - 批量保存索引
  - `get_index_entries()` - 分页获取索引
  - `calculate_introspect_level()` - DataGrip 风格级别计算
  - `get_schema_object_counts()` - 获取对象统计
  - `build_metadata_index()` - 构建元数据索引（完整预热流程）
  - `save_index_entries_internal()` - 内部批量保存（事务）
  - `enqueue_sync_task()` - 入队同步任务
  - `enqueue_sync_tasks_batch()` - 批量入队
  - `enqueue_indexing_tasks()` - 入队索引任务
  - `get_next_sync_task()` - 获取下一个任务
  - `claim_sync_task()` - 认领任务
  - `complete_sync_task()` - 完成同步任务
  - `get_pending_task_count()` - 获取待处理任务数
  - `get_tables_chunk()` - 分块获取表
  - `update_sync_status()` - 更新同步状态
  - `get_sync_status()` - 获取同步状态
  - `is_syncing()` - 检查是否正在同步
  - `cancel_sync()` - 取消同步

#### 新增 Tauri 命令

- `build_cache_index` - 构建缓存索引（完整预热流程-V2 优化版）
  - JoinSet 多 Schema 并行获取
  - JoinSet 表级并行获取 Columns
  - 流式写入（每 500 条一批）
  - 进度回调（Tauri Event: `cache_warming_progress`）
  - 取消支持（CancellationToken）

#### 迁移文件

- `006_add_metadata_index.sql` - 索引表与分页懒加载

### [v2.1] - 2026-05-04

#### 新增

- **元数据缓存规范化（V4）**
  - 规范化表结构：schemata / tables / columns / indexes / views / routines 独立表
  - 外键约束确保数据完整性
  - 向后兼容视图保持旧接口可用

- **FTS5 全文搜索增强（V5）**
  - 规范化表数据同步到 FTS5 虚拟表
  - 支持增量同步（按类型）
  - 搜索结果高亮显示

- **级联删除支持（V5）**
  - 删除 Schema 时自动级联删除关联数据
  - FTS 索引同步清理

- **MetadataCacheOps 新方法**
  - `sync_fts_index()` - FTS 索引同步
  - `search_fts()` - 全文搜索
  - `delete_schema()` - 级联删除

#### 迁移文件

- `005_normalized_fts_and_cascade.sql` - FTS 同步与级联删除

### [v2.0] - 2026-04-23

#### 新增

- **插件化架构优化**
  - DDD 分层架构（domain/infrastructure/ui）
  - 事件总线（EventBus）插件间通信机制
  - 统一 API 层
  - ExtensionContext 生命周期管理

- **前端架构**
  - 完整的插件系统
  - shared/ 共享资源中心
  - 全局类型系统

#### 变更

- **SQL 编辑器**
  - 1:n 编辑器-结果集关系重构
  - 多结果标签页支持

- **数据库导航**
  - IVM 增量视图维护
  - 三级缓存架构

---

### [v1.0] - 2026-04-20

#### 新增

- 初始插件化架构
- 内置插件：
  - connection（连接管理）
  - database（数据库导航）
  - navigator（通用导航器）
  - query（查询执行）
  - workbench（SQL 工作台）

---

## 文档变更日志

### [v2.0] - 2026-05-03

#### 新增

- `docs/README.md` - 项目文档中心总索引
- `docs/backend/TECHNICAL_OVERVIEW.md` - 技术概览

#### 变更

- `docs/backend/README.md` - 统一文档格式（版本、日期、状态）
- `docs/navigator/README.md` - 统一文档格式
- `docs/frontend/INDEX.md` - 统一文档格式
- `src-tauri/src/docs/README.md` - 统一文档格式

---

## 提交规范

项目使用 **Gitmoji + Angular** 提交规范：

### 格式

```
<emoji> <type>(<scope>): <subject>
```

### 类型

| 类型     | Emoji | 说明     |
| -------- | ----- | -------- |
| feat     | ✨    | 新增功能 |
| fix      | 🐛    | 修复 Bug |
| docs     | 📝    | 文档变更 |
| refactor | ♻️    | 代码重构 |
| perf     | ⚡️    | 性能优化 |
| style    | 💄    | 格式调整 |
| test     | 🧪    | 测试相关 |
| build    | 📦    | 构建相关 |
| chore    | 🔧    | 杂项配置 |

### 示例

```bash
✨ feat(workbench): 实现 SQL 编辑器多标签页
🐛 fix(sqlite): 修复百万级数据查询超时
📝 docs: 补充前端架构文档
♻️ refactor(database): 重构导航器缓存逻辑
⚡️ perf(query): 优化查询缓存命中率
🔧 chore: 更新依赖版本
```

---

## 版本管理策略

### 分支策略

- `main` - 主分支，稳定版本
- `develop` - 开发分支
- `feature/*` - 功能分支
- `fix/*` - 修复分支

### 发布流程

1. 功能开发完成 → 合并到 `develop`
2. 测试验证 → 合并到 `main`
3. 打标签发布 → `git tag v{x.y.z}`

### 兼容性约束

- 接口遵循语义化版本（SemVer）
- 禁止破坏性变更（major 版本内）
- 10 年向前兼容目标

---

## 维护

- **最后更新**：2026-05-03
- **更新频率**：每次重要变更后同步更新
- **格式要求**：使用 Keep a Changelog 规范
