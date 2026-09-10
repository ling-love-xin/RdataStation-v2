# Tasks

## Phase 1: 数据层改造（result-store 隔离）

- [x] Task 1: result-store 从全局改为按 editorId 隔离
  - 将 `result-store.ts` 中的 `tabs`、`activeTabId` 等状态改为 `Map<editorId, ResultTab[]>` 结构
  - 新增 `currentEditorId` 状态，绑定当前激活的编辑器文件路径
  - 新增 `getTabs(editorId)` / `getActiveTab(editorId)` 等 getter
  - 所有 mutation action 接受 `editorId` 参数
  - 保留向后兼容的快捷访问（当 `currentEditorId` 已设置时，省略 `editorId` 参数）
  - 新增 `clearEditorResults(editorId)` 方法，清理指定编辑器的所有结果

## Phase 2: 结果集 UI 嵌入

- [x] Task 2: 将 QueryResultPanel 核心功能抽取为可嵌入子组件
  - 创建 `ResultArea.vue` 作为编辑器内嵌结果集的主容器组件
  - 将 QueryResultPanel 的 template 内容（工具栏条 + 视图切换 + 表格 + 右侧面板 + 状态栏）迁移到 `ResultArea.vue`
  - 保持 QueryResultPanel.vue 可用（暂不删除，作为过渡），使其内部委托给 `ResultArea.vue`
  - 所有子组件（ResultGridView、ResultTextView、ResultRecordView、ResultChartView 等）保持独立复用

- [x] Task 3: 扩展 EditorPanel 的 result-area 区域
  - 在 EditorPanel.vue 的 `result-area` div 中引入 `ResultArea` 组件
  - 传入当前文件的 `editorId`（文件路径）作为 props
  - 调整 `hasResults` computed，改为依赖当前 editorId 的结果状态（`resultStore.hasResults`）
  - 确保 `splitRatio` 拖拽逻辑与新的结果区域兼容
  - 在 `onUnmounted` 中调用 `resultStore.clearEditorResults(fp)` 清理结果

- [x] Task 4: 扩展 ResultSubTab 为完整结果标签栏
  - 合并到 Task 3：ResultArea.vue 已内置完整的 result-tabs 标签栏
  - ResultSubTab.vue 从 EditorPanel 中移除引用，保留文件作为向后兼容
  - 支持多标签切换、关闭、右键菜单（由 ResultArea 内置）

## Phase 3: 执行链路适配

- [x] Task 5: 适配 useSqlExecution 结果写入路由
  - 修改 `useSqlExecution.ts` 中结果写入逻辑，传入当前 `editorId`
  - SQL 执行完成后，结果写入 `result-store` 对应 editorId 的命名空间
  - 确保 `executeSingleStatement`、`executeBatch`、DuckDB 加速执行等路径均适配

- [x] Task 6: 适配 EditorManager 结果集管理
  - 在 `sql-execution-service.ts` 的 `executeCurrentSQL` 中增加 resultStore 写入
  - 成功/失败/异常三条路径均写入 resultStore，确保 editorId 隔离
  - 保留 `result-set-manager.ts` 的 dockview 面板创建逻辑作为过渡

## Phase 4: 清理与收尾

- [x] Task 7: 移除 QueryResultPanel 独立 dockview 面板注册
  - 从 MainContentArea.vue 中移除 QueryResultPanel 底部面板区域
  - 移除相关的 resize 逻辑和 resultStore.showPanel 依赖
  - 保留 QueryResultPanel.vue 文件（MultiTabResults 等内部引用保留）
  - workbench-store.ts 的 `openQueryResult` 函数保留（未被调用，向后兼容）

- [x] Task 8: DuckDB 临时表生命周期管理
  - 在 result-store 的 `closeTab(editorId, tabId)` 中增加 DuckDB 临时表引用清理
  - 在 result-store 的 `clearEditorResults(editorId)` 中批量清理临时表引用
  - 在 EditorPanel 的 `onUnmounted` 中触发 `clearEditorResults`

# Task Dependencies

- Task 2 依赖 Task 1（结果集 UI 需要隔离后的 store）
- Task 3 依赖 Task 2（EditorPanel 引用 ResultArea 组件）
- Task 5 依赖 Task 1（执行链路需要隔离后的 store）
- Task 4 可与 Task 2 并行
- Task 7 依赖 Task 3（移除独立面板注册前确保编辑器内嵌已工作）
- Task 8 依赖 Task 1（临时表管理依赖 store 隔离后的生命周期）