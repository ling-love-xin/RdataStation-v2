# Tasks: 洞察模块混合布局

- [x] Task 1: 新增 BottomInsightPanel 容器组件
  - 创建 `src/extensions/builtin/workbench/ui/components/panels/BottomInsightPanel.vue`
  - 使用 NTabs 组件，包含 4 个 TabPane：多列分析、历史版本、Schema 洞察、表探查
  - 每个 TabPane 分别渲染 MultiColumnView、InsightHistoryTab、SchemaInsightPanel、TableProfileView
  - 从 insight-store 读取 `bottomInsightTab` 控制当前激活 Tab
  - 切换 Tab 时更新 `layoutStore.bottomInsightTab`
  - 监听 insight-store 的 `pendingSchemaInsightRequest` / `pendingTableProfileRequest` 自动切换 Tab

- [x] Task 2: 简化 ColumnInsightsPanel 为单一列洞察视图
  - 修改 `src/extensions/builtin/workbench/ui/components/panels/ColumnInsightsPanel.vue`
  - 移除 NTabs 组件（移除多列分析 Tab 和历史 Tab）
  - 直接渲染 ColumnInsightPanel 组件（不再包裹 Tab 栏）
  - 保持与 insight-store 的数据绑定不变

- [x] Task 3: 扩展 layout-store 状态管理
  - 在 `src/extensions/builtin/workbench/ui/stores/layout-store.ts` 中新增 `bottomInsightTab` ref
  - 类型: `'multi' | 'history' | 'schema' | 'table'`，默认 `'multi'`
  - 新增 `setBottomInsightTab(tab)` 方法
  - 在布局序列化/反序列化中加入 `bottomInsightTab` 持久化

- [x] Task 4: 注册底部面板组件到 Workbench
  - 修改 `src/extensions/builtin/workbench/ui/views/WorkbenchView.vue`（或对应布局文件）
  - 在底部面板区域注册 BottomInsightPanel 组件
  - 确保底部面板高度可拖拽调整（已有 resize 机制）

- [x] Task 5: 新增 i18n 国际化键
  - 在 `src/shared/locales/zh-CN.json` 中新增 4 个 Tab 标签 key:
    - `navigator.multiColumnAnalysis`: "多列分析"
    - `navigator.historyVersions`: "历史版本"
    - `navigator.schemaInsight`: "Schema 洞察"
    - `navigator.tableProfile`: "表探查"

- [x] Task 6: 验证编译与 lint
  - 运行 `pnpm run lint` 确保无新增错误
  - 运行 `pnpm run typecheck` 确保类型检查通过

# Task Dependencies

- Task 3 依赖 Task 1（BottomInsightPanel 需要 layoutStore 的 bottomInsightTab 状态）
- Task 4 依赖 Task 1（Workbench 需要 BottomInsightPanel 组件）
- Task 2 可并行于 Task 1
- Task 5 可并行于 Task 1-4
- Task 6 依赖 Task 1-5 全部完成
