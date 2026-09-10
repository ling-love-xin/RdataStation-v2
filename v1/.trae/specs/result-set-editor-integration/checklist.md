# Checklist

## 数据层
- [x] result-store 的 `tabs` 状态从数组改为 `Map<editorId, ResultTab[]>`
- [x] result-store 的 `activeTabId` 按 editorId 隔离
- [x] result-store 新增 `currentEditorId` 状态，与编辑器激活文件同步
- [x] result-store 所有 mutation action 接受 `editorId` 参数
- [x] result-store 新增 `clearEditorResults(editorId)` 方法

## UI 层
- [x] `ResultArea.vue` 组件创建，包含完整结果集功能（工具栏条 + 视图切换 + 表格 + 右侧面板 + 状态栏）
- [x] EditorPanel.vue 的 `result-area` 区域引入 `ResultArea` 组件
- [x] `hasResults` computed 基于当前 editorId 的结果状态计算（`resultStore.hasResults`）
- [x] 分割条拖拽与新的结果区域兼容，比例范围 10%-90%
- [x] 结果区域支持折叠（双击分割条）和展开（执行 SQL 后自动展开）
- [x] 结果标签栏支持多标签切换、关闭、右键菜单（由 ResultArea 内置）
- [x] 右侧面板（值查看器/元数据/计算/分组/引用/洞察）在结果区域内正常工作
- [x] 结果状态栏显示行数、执行时间、过滤状态

## 执行链路
- [x] `useSqlExecution` 结果写入时传入正确的 `editorId`
- [x] `sql-execution-service.ts` 的 `executeCurrentSQL` 同时写入 resultStore
- [x] SQL 执行完成后结果出现在编辑器下方而非独立面板
- [x] `executeSingleStatement`、`executeBatch`、DuckDB 加速执行路径均适配
- [x] 不同文件执行 SQL 后结果互不干扰（editorId 隔离）

## 清理
- [x] QueryResultPanel 独立 dockview 底部面板区域从 MainContentArea.vue 移除
- [x] 保留 QueryResultPanel.vue 文件（MultiTabResults 内部引用）
- [x] 关闭结果标签时 DuckDB 临时表引用清理
- [x] 关闭编辑器文件时所有关联临时表引用清理（`clearEditorResults`）
- [x] EditorPanel `onUnmounted` 触发 `resultStore.clearEditorResults(fp)`

## 回归验证
- [ ] 现有 SQL 执行功能正常（普通模式/分析模式/智能模式）
- [ ] 结果集过滤功能正常（快速过滤/SQL 过滤/DuckDB 分析）
- [ ] 结果集导出功能正常（CSV/JSON/Excel/Parquet/INSERT）
- [ ] 单元格编辑功能正常
- [ ] 列洞察功能正常（查看统计/分布/质量/历史）
- [ ] 编辑器模式切换（SQL/分析/代码）不影响结果区域
- [ ] 编辑器标签切换时结果区域正确切换
- [ ] 编辑器关闭时结果正确清理，无内存泄漏