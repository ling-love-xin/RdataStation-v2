# 导航栏深度分析 — 改进任务（已完成）

## 已完成（V10.7）

- [x] Task 1: 修复 schema 节点下 loadTables 重复调用
  - 文件: `use-database-tree-loader.ts` 第 872-875 行
  - 修复: 删除 Promise.all 双调用，改为单次 await navigatorStore.loadTables(...)
  - 验证: pnpm typecheck OK + ESLint OK

- [x] Task 2: 树展开状态跨会话持久化改进
  - 新增: `saveLastActiveConnection()` / `getLastActiveConnection()` 到 navigator-persistence.ts
  - 新增: 连接节点选中时自动保存 lastActive
  - 新增: `onMounted` 自动恢复上次活跃连接（expand + select）
  - 验证: pnpm typecheck OK + ESLint OK

- [x] Task 3: 搜索性能优化（300ms debounce）
  - 修复: `onSearchQueryChange` 用 `debounceAsync(fn, 300)` 包装
  - 导入: `debounceAsync` from `../utils/debounce`
  - 验证: pnpm typecheck OK + ESLint OK
