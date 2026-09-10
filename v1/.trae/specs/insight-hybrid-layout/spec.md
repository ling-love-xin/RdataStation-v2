# 洞察模块混合布局 Spec

## Why

当前洞察模块所有功能（列洞察/多列分析/历史/Schema洞察/表探查）集中在 340px 右侧边栏内，多列分析结果表格和历史版本对比面板在窄空间中极度拥挤。方案B将右侧边栏保留为列洞察快速预览，底部面板（全宽）承载多列分析、历史版本、Schema洞察、表探查四类分析型内容，充分利用全宽空间。

## What Changes

- **布局重构**: 右侧边栏只保留 ColumnInsightPanel（列洞察快速预览），多列分析/历史版本/Schema洞察/表探查移至底部面板
- **底部面板新增 4 个 Tab**: 多列分析、历史版本、Schema 洞察、表探查
- **RightSidebarContent 简化**: 注册表从 4 个组件减少到 1 个（ColumnInsightsPanel）
- **新增 BottomInsightPanel 容器组件**: 统一管理底部面板的 4 个洞察 Tab
- **layout-store 扩展**: 新增 `bottomInsightTab` 状态管理
- **i18n**: 新增底部面板 4 个 Tab 标签的国际化 key

## Impact

- Affected specs: 无
- Affected code:
  - `src/extensions/builtin/workbench/ui/components/RightSidebarContent.vue` — 简化组件注册
  - `src/extensions/builtin/workbench/ui/components/panels/ColumnInsightsPanel.vue` — 移除内部 Tab（多列/历史）
  - `src/extensions/builtin/workbench/ui/stores/layout-store.ts` — 新增 bottomInsightTab 状态
  - `src/extensions/builtin/workbench/ui/views/WorkbenchView.vue` — 底部面板注册新组件
  - `src/extensions/builtin/workbench/ui/components/panels/BottomInsightPanel.vue` — **新增** 底部洞察容器
  - `src/shared/locales/zh-CN.json` — 新增 i18n keys

## ADDED Requirements

### Requirement: 底部洞察面板容器

系统 SHALL 在底部面板区域提供一个新的 Tab 容器组件 `BottomInsightPanel`，包含 4 个 Tab：多列分析、历史版本、Schema 洞察、表探查。

#### Scenario: 底部面板默认显示多列分析

- **WHEN** 用户打开工作台
- **THEN** 底部面板显示 "多列分析" Tab 激活状态

#### Scenario: 切换到历史版本 Tab

- **WHEN** 用户点击底部面板 "历史版本" Tab
- **THEN** 底部面板显示 InsightHistoryTab 组件内容

#### Scenario: 切换到 Schema 洞察 Tab

- **WHEN** 用户点击底部面板 "Schema 洞察" Tab
- **THEN** 底部面板显示 SchemaInsightPanel 组件内容

#### Scenario: 切换到表探查 Tab

- **WHEN** 用户点击底部面板 "表探查" Tab
- **THEN** 底部面板显示 TableProfileView 组件内容

### Requirement: 右侧边栏简化

系统 SHALL 将右侧边栏的 ColumnInsightsPanel 简化为仅包含列洞察 Tab（移除多列分析和历史 Tab）。

#### Scenario: 右侧边栏只显示列洞察

- **WHEN** 用户打开右侧边栏的列洞察
- **THEN** 只显示单个列洞察视图（QualityScoreCard + NCollapse 统计区），不再显示 Tab 栏

### Requirement: 跨组件通信保持

系统 SHALL 保持 insight-store 的 `pendingXxxRequest` 机制不变，确保底部面板和右侧边栏之间的数据联动正常。

#### Scenario: 右键数据库打开 Schema 洞察

- **WHEN** 用户在导航树右键数据库选择 "Schema 洞察"
- **THEN** 底部面板自动切换到 "Schema 洞察" Tab 并加载数据

#### Scenario: 右键表打开表探查

- **WHEN** 用户在导航树右键表选择 "表探查"
- **THEN** 底部面板自动切换到 "表探查" Tab 并加载数据

## MODIFIED Requirements

### Requirement: layout-store 底部面板 Tab 状态

系统 SHALL 在 `layout-store` 中新增 `bottomInsightTab` 状态字段，类型为 `'multi' | 'history' | 'schema' | 'table'`，默认值为 `'multi'`。

#### Scenario: 状态持久化

- **WHEN** 用户切换底部面板 Tab 后关闭应用
- **THEN** 下次打开应用时恢复上次选择的 Tab

## REMOVED Requirements

无
