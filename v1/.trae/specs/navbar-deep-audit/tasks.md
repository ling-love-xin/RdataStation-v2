# 导航栏深度审计 — 优化任务

## 🔴 本轮必做（已完成 V10.8）

- [x] Task 1: 修复 loadChildren 异常静默吞噬
  - [x] 在 catch 块中调用 navigatorStore.setNodeError(node.key, msg) 写入错误
  - [x] createErrorPlaceholderNode() 返回错误占位节点（用户可见 `⚠ 加载失败：{msg}`）
  - [x] 添加 `placeholder` 到 VirtualTreeNodeType 联合类型
  - [x] 验证: pnpm typecheck OK + ESLint OK

- [x] Task 2: 清理 debouncedPersistSave timer
  - [x] 在 onUnmounted 中添加 `if (persistTimer) clearTimeout(persistTimer)`
  - [x] 保留 saveAllNavigatorStates() 确保已展开节点数据不丢失
  - [x] 验证: pnpm typecheck OK + ESLint OK

- [x] Task 3: 修复 connection 节点 connectionId 语义错误
  - [x] connection 节点从 keyParts[2] 取 connId，从 node.data.driver 取 dbType
  - [x] 非 connection 节点保持 keyParts[1] + navigatorStore.getDbType(connectionId)
  - [x] 验证: pnpm typecheck OK + ESLint OK

## 🟡 可选优化（已完成 V10.8）

- [x] Task 4: 深 watcher 评估
  - [x] 结论: debouncedPersistSave 800ms 已充分聚合展开/折叠操作，实际性能影响可忽略
  - [x] 决策: 暂不修改，保留现有 watch({ deep: true })

- [x] Task 5: tables-folder + views-folder 展开跳过重复 loadTables
  - [x] tables-folder: 先检查 catalog.schemas[i].tables/views 是否有数据
  - [x] views-folder: 同上，有数据直接 createViewNodes()
  - [x] 验证: pnpm typecheck OK + ESLint OK

## 🟢 远期规划（排入 backlog）

- [ ] Task 6: 搜索结果点击 → 自动展开树到对应节点
- [ ] Task 7: VirtualTree ARIA 无障碍属性
- [ ] Task 8: startCacheWarming 添加 AbortController 取消
- [ ] Task 9: localStorage quota 溢出降级
- [ ] Task 10: getNavConfig 缓存
