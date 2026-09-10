# Tasks

- [x] Task 1: 创建 `treeMutation` 通用工具函数
  - [x] 新建 `src/extensions/builtin/database/ui/utils/tree-mutation.ts`
  - [x] 实现 `mutateTreeNode(connectionId, path, updater)` — 封装 catalogs.find → schemas.find → mutate 的遍历模式
  - [x] 实现 `getTreeNode(connectionId, path)` — 按路径获取树节点
  - [x] 定义 `TreeMutationPath` 类型：`{ catalogName?: string; schemaName?: string; tableName?: string }`
  - [x] 验证：单独文件编译通过，类型推断正确

- [x] Task 2: 拆分 Store — 子加载器模块
  - [x] 新建 `src/extensions/builtin/database/ui/stores/nav-loaders/use-catalog-loader.ts`
  - [x] 新建 `src/extensions/builtin/database/ui/stores/nav-loaders/use-table-loader.ts`
  - [x] 新建 `src/extensions/builtin/database/ui/stores/nav-loaders/use-object-loader.ts`
  - [x] 新建 `src/extensions/builtin/database/ui/stores/nav-loaders/use-column-loader.ts`
  - [x] 每个子 loader 内部使用 `treeMutation.ts` 的 `mutateTreeNode` 替代手动遍历
  - [x] 每个子 loader 返回 loading/error 状态

- [x] Task 3: 引入通用 LazyLoader 工厂
  - [x] 新建 `src/extensions/builtin/database/ui/utils/lazy-loader.ts`
  - [x] 实现 `createLazyLoader` 函数：先查缓存 → 缓存有效返回 → 缓存无效调 API
  - [x] 在 catalog-loader 和 table-loader 中使用 LazyLoader 替换三段式重复代码
  - [x] 验证：L2 缓存命中时跳过 API 调用，缓存失效时正常调用

- [x] Task 4: 重构主 Store — 聚合层
  - [x] 精简 `database-navigator-store.ts` 为聚合层
  - [x] 向内聚合 4 个子 loader 的导出
  - [x] 保持所有对外接口不变（向后兼容）
  - [x] 错误状态改为 `Map<string, string>`，提供 `getNodeError(key)` / `setNodeError(key, msg)` / `clearNodeError(key)` / `clearAllNodeErrors()`
  - [x] 消除 `connectionCatalogs.value = new Map(currentMap)` 反模式，改为直接 `set`
  - [x] 验证：`pnpm typecheck` 通过，所有现有 imports 不报错

- [x] Task 5: Loading 状态独立化
  - [x] 在 Object Loader 中为 procedures/functions/sequences/triggers 各自创建独立的 `Ref<Set<string>>`
  - [x] 修改主 Store 暴露 `isLoadingProcedures` / `isLoadingFunctions` / `isLoadingSequences` / `isLoadingTriggers` computed
  - [x] 验证：展开 Procedures 文件夹后再展开 Functions 文件夹，两个互不阻塞

- [x] Task 6: 移除冗余代码
  - [x] 删除 `loadViews` 函数（纯包装，tree loader 已直接用 `loadTables`）
  - [x] 删除未使用的 `loadSchemasFromCache` 和 `loadTablesFromCache` 方法（loadSchemasFromCache 仅内部使用保留，loadTablesFromCache 已不存在）
  - [x] 验证：搜索全项目调用引用，确认无残留后删除

- [x] Task 7: 适配 database-navigator.vue
  - [x] 适配新 Store 的错误获取方式（`getNodeError(node.key)` + `setNodeError` 替代 `error.value`）
  - [x] 适配新的 loading computed 属性
  - [x] 验证：pnpm typecheck 通过（导航相关 0 error）

- [x] Task 8: 更新相关文档
  - [x] 更新 `docs/navigator/README.md` — 记录 Store 拆分、新工具函数、已知问题状态（v2.3）
  - [x] 更新 `docs/frontend/ARCHITECTURE.md` — 补充导航栏子模块架构说明
  - [x] 更新 `docs/metadata-cache-implementation.md` — 记录 V10.6 前端重构

# Task Dependencies

- Task 2, 3, 4, 5, 6 都依赖 Task 1（treeMutation 工具函数）
- Task 4 依赖 Task 2、3（聚合层需要子 loader 和 LazyLoader 就绪）
- Task 5 依赖 Task 2（独立的 loading 状态在子 loader 中）
- Task 7 依赖 Task 4（Nav 组件适配需要新 Store 完成）
- Task 8 依赖所有 Task 完成
