# Checklist: 洞察模块混合布局

- [x] BottomInsightPanel.vue 创建，包含 4 个 NTabs TabPane
- [x] 底部面板 4 个 Tab 分别渲染正确组件（MultiColumnView / InsightHistoryTab / SchemaInsightPanel / TableProfileView）
- [x] 底部面板 Tab 切换后 `layoutStore.bottomInsightTab` 值正确更新
- [x] insight-store 的 `pendingSchemaInsightRequest` 触发时底部面板自动切换到 Schema 洞察 Tab
- [x] insight-store 的 `pendingTableProfileRequest` 触发时底部面板自动切换到表探查 Tab
- [x] ColumnInsightsPanel 保持为单一列洞察视图（本身无 NTabs）
- [x] 右侧边栏列洞察功能正常（ColumnInsightsPanel 快速统计）
- [x] layout-store 新增 `bottomInsightTab` 状态，默认 `'multi'`
- [x] layout-store 序列化/反序列化包含 `bottomInsightTab` 持久化
- [x] Workbench 底部面板注册 BottomInsightPanel 组件（query extension.ts）
- [x] zh-CN.json + en.json 新增 4 个 i18n key
- [x] `pnpm run lint` 0 新增错误（6 errors 全部已有）
- [x] `pnpm run typecheck` 0 新增错误（BottomInsightPanel/layout-store 零错误）
