# 结果集融合到编辑器 Spec

## Why
当前结果集（QueryResultPanel）是独立的 dockview 面板，与编辑器分离。用户执行 SQL 后，结果出现在另一个标签页中，导致编辑区和结果区无法同时查看，需要频繁切换标签。DBeaver/DataGrip 等成熟工具的标准做法是将结果集嵌入编辑器面板下方，形成上下分屏布局，编辑区和结果区可同时可见、可拖拽调整比例。

## What Changes
- 将 QueryResultPanel 的完整功能（工具栏条、视图切换、AG Grid 表格、右侧面板、状态栏）嵌入到 EditorPanel 的 `result-area` 区域
- 结果集按编辑器文件实例隔离（每个文件有自己的结果标签页列表）
- 移除 QueryResultPanel 作为独立 dockview 面板的注册，改为 EditorPanel 内部子组件
- 右侧面板（值查看器/元数据/计算/分组/引用/洞察）保留在结果区域内，不与编辑器右侧 ActivityBar 面板冲突
- 保留编辑器现有的 `splitRatio` 拖拽分割机制
- **BREAKING**: result-store 从全局单例改为按编辑器实例隔离

## Impact
- Affected specs: `multi-mode-editor`（EditorBody 共享区域变更）
- Affected code:
  - `src/extensions/builtin/workbench/ui/components/panels/EditorPanel.vue` — result-area 区域扩展为完整结果集
  - `src/extensions/builtin/workbench/ui/components/panels/QueryResultPanel.vue` — 功能迁移到 EditorPanel，自身废弃或转为纯子组件
  - `src/extensions/builtin/workbench/ui/stores/result-store.ts` — 从全局 store 改为按 editorId 隔离
  - `src/extensions/builtin/workbench/manager/result-set-manager.ts` — 适配新的结果集生命周期
  - `src/extensions/builtin/workbench/ui/components/panels/ResultSubTab.vue` — 扩展为完整结果标签栏
  - `src/extensions/builtin/workbench/manager/EditorManager.ts` — 移除独立结果面板的 dockview 注册逻辑
  - `src/extensions/builtin/workbench/ui/composables/useSqlExecution.ts` — 结果写入改为按 editorId 路由

---

## ADDED Requirements

### Requirement: 编辑器内嵌结果集布局
编辑器面板 SHALL 在编辑区下方嵌入完整的结果集区域，支持上下分屏拖拽。

#### Scenario: 执行 SQL 后结果出现在编辑器下方
- **GIVEN** 用户在查询编辑器中编写了 `SELECT * FROM users`
- **WHEN** 用户点击执行按钮
- **THEN** 编辑区下方展开结果集区域，显示查询结果表格，分割比例为 50:50（可配置）

#### Scenario: 无结果时结果区域隐藏
- **GIVEN** 编辑器中没有执行过任何查询
- **WHEN** 编辑器渲染
- **THEN** 结果区域不显示，编辑区占满整个面板

#### Scenario: 拖拽调整分割比例
- **GIVEN** 结果区域已显示
- **WHEN** 用户拖拽编辑区和结果区之间的分割条
- **THEN** 分割比例实时调整，最小 20%:80%，最大 80%:20%

#### Scenario: 关闭结果区域
- **GIVEN** 结果区域已显示
- **WHEN** 用户双击分割条或点击结果区域关闭按钮
- **THEN** 结果区域折叠，编辑区恢复全屏，结果数据保留在内存中（再次展开可恢复）

---

### Requirement: 结果集按编辑器实例隔离
每个编辑器文件实例 SHALL 拥有独立的结果标签页列表，不同文件的结果互不干扰。

#### Scenario: 切换文件后结果隔离
- **GIVEN** 用户在 `query1.sql` 中执行了查询，结果区域有 3 个结果标签
- **WHEN** 用户切换到 `query2.sql` 编辑器标签
- **THEN** 结果区域显示 `query2.sql` 对应的结果标签（可能为空），切换回 `query1.sql` 时恢复之前的 3 个结果标签

#### Scenario: 关闭文件时清理结果
- **GIVEN** 用户在 `query1.sql` 中有 2 个结果标签
- **WHEN** 用户关闭 `query1.sql` 编辑器标签
- **THEN** 该文件对应的结果数据被清理（DuckDB 临时表释放、内存释放）

---

### Requirement: 结果集完整功能内嵌
结果区域 SHALL 包含完整的结果集功能，与原 QueryResultPanel 功能一致。

#### Scenario: 结果标签栏
- **GIVEN** 用户执行了 3 次查询
- **WHEN** 查看结果区域
- **THEN** 顶部显示 3 个结果标签（标签名包含执行时间或序号），可切换、关闭

#### Scenario: 工具栏条（过滤模式）
- **GIVEN** 结果区域已显示
- **WHEN** 查看工具栏条
- **THEN** 包含过滤模式切换（快速过滤/SQL 过滤/DuckDB 分析）、过滤预设、快速过滤输入框

#### Scenario: 视图切换（表格/文本/记录/图表）
- **GIVEN** 结果区域已显示
- **WHEN** 用户点击左侧视图切换栏的按钮
- **THEN** 在表格视图、文本视图、记录视图、图表视图之间切换

#### Scenario: 右侧面板（值查看器/元数据/计算/分组/引用/洞察）
- **GIVEN** 结果区域已显示
- **WHEN** 用户点击右侧面板标签
- **THEN** 显示对应的面板内容（值查看器、元数据、计算、分组、引用、洞察），面板可折叠

#### Scenario: 结果状态栏
- **GIVEN** 结果区域已显示
- **WHEN** 查看底部状态栏
- **THEN** 显示行数、执行时间、过滤状态、导出按钮等

---

### Requirement: 结果集右侧面板不冲突
结果集的右侧面板（值查看器/元数据/计算/分组/引用/洞察）SHALL 在结果区域内独立显示，不与编辑器右侧 ActivityBar 面板（列洞察/Mock数据/SQL历史）冲突。

#### Scenario: 结果集右侧面板与编辑器右侧面板共存
- **GIVEN** 结果区域已显示，且编辑器右侧面板（列洞察）已打开
- **WHEN** 用户在结果区域中打开"洞察"面板
- **THEN** 结果集洞察面板显示在结果区域内，编辑器右侧列洞察面板不受影响（两者服务于不同上下文）

---

### Requirement: DuckDB 临时表生命周期管理
每个结果标签对应的 DuckDB 临时表 SHALL 在结果标签关闭或文件关闭时自动释放。

#### Scenario: 关闭结果标签时释放临时表
- **GIVEN** 结果标签 `Result 1` 关联了 DuckDB 临时表 `temp_abc123`
- **WHEN** 用户关闭该结果标签
- **THEN** 后端释放 DuckDB 临时表 `temp_abc123`

#### Scenario: 关闭文件时释放所有临时表
- **GIVEN** 文件 `query1.sql` 有 3 个结果标签，关联 3 个 DuckDB 临时表
- **WHEN** 用户关闭 `query1.sql` 编辑器标签
- **THEN** 后端释放所有 3 个 DuckDB 临时表

---

## MODIFIED Requirements

### Requirement: result-store 从全局改为按实例隔离
当前 `result-store.ts` 是全局 Pinia store，所有编辑器共享同一个结果标签列表。SHALL 改为按编辑器文件路径（`editorId`）隔离，每个文件独立管理自己的结果标签。

#### Scenario: 结果 store 隔离
- **GIVEN** 用户打开了 `query1.sql` 和 `query2.sql`
- **WHEN** 在 `query1.sql` 中执行查询
- **THEN** 结果标签写入 `result-store` 中 `query1.sql` 对应的命名空间，不影响 `query2.sql` 的结果状态

### Requirement: EditorPanel result-area 扩展
当前 EditorPanel 的 `result-area` 仅包含 `ResultSubTab` 和空的 `result-panel-host`。SHALL 扩展为包含完整结果集功能（工具栏条 + 视图切换 + 表格 + 右侧面板 + 状态栏）。

### Requirement: QueryResultPanel 生命周期变更
当前 QueryResultPanel 作为独立 dockview 面板注册。SHALL 改为 EditorPanel 内部子组件，不再独立注册为 dockview 面板。保留其功能逻辑，仅改变挂载位置。

---

## REMOVED Requirements

### Requirement: QueryResultPanel 独立 dockview 面板注册
**Reason**: 结果集融合到编辑器后，不再需要独立的 dockview 面板，避免用户需要切换标签才能同时查看编辑区和结果区。  
**Migration**: QueryResultPanel 的所有功能迁移到 EditorPanel 的 result-area 区域，dockview 面板注册代码移除。外部通过 `EditorManager` 打开结果面板的调用改为直接操作编辑器实例的结果状态。