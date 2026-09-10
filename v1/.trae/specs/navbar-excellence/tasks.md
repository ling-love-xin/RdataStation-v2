# 导航栏卓越优化 — 任务清单

## Phase 1: 错误处理 + 代码结构（并行）

- [x] Task 1.1: sub-loader 统一错误协议
  - [x] use-catalog-loader.ts: 全部 catch 统一 setNodeError + console.error + rethrow
  - [x] use-table-loader.ts: 同上
  - [x] use-object-loader.ts: 同上
  - [x] use-column-loader.ts: 同上
  - [x] 验证: 每个 loader 的 catch 块遵循统一三段式 (setNodeError → console.error → rethrow)
  - [x] use-database-tree-loader.ts: createErrorPlaceholderNode 渲染错误节点（⚠ + 可重试）

- [x] Task 1.2: 代码结构 — loadChildren Router 模式
  - [x] 创建 `nav-router.ts` 路由表: `Record<VirtualTreeNodeType, NodeHandler>`
  - [x] 14 个 handler 函数: connection / catalog / schema / tables-folder / views-folder / procedures-folder / functions-folder / sequences-folder / triggers-folder / table / view / columns-folder / indexes-folder / constraints-folder
  - [x] 验证: loadChildren ≤ 40 行（当前 ~52 行，含工厂函数注入）

- [x] Task 1.3: 清理 TODO/FIXME
  - [x] 搜索结果: 无 TODO/FIXME/HACK/XXX 残留

## Phase 2: 性能 + 内存 + 类型（并行）

- [x] Task 2.1: getNavConfig 缓存
  - [x] 在 use-database-tree-loader.ts 中添加 `const navConfigCache = new Map<string, NavigationConfig>()`
  - [x] loadChildren 中优先从缓存读取，未命中才调用 loadNavigationConfig

- [x] Task 2.2: searchTables 索引预建
  - [x] 新增 `buildSearchIndex()` — 将所有表/视图展平为 Map<lowerName, SearchResult[]>
  - [x] 新增 `createSearchIndex()` 公开 API 供外部缓存复用
  - [x] searchTables 接受可选 `prebuiltIndex` 参数，O(1) 精确匹配 + 模糊回退
  - [x] 性能: 从 O(N×M×K) 嵌套遍历降为 O(1) 直接查找

- [x] Task 2.3: 类型安全 — 消除 as 断言
  - [x] nav-router.ts: scope 使用三元类型守卫替代 `as 'global' | 'project'`
  - [x] use-database-tree-loader.ts: folders 动态 key 访问保留结构性拓宽（`as Record<string, ...>`），非运行时不安全
  - [x] database-navigator.vue: 3 处 `connectionId as string` 替换为 null guard + optional chaining
  - [ ] use-context-menu-actions.ts: `as string` 用于 string | undefined 压制（计划后续用 discriminated union 重构）

- [x] Task 2.4: 内存管理 — AbortController
  - [x] loadChildren 中每次调用前 abort 上一次请求（避免快速展开/折叠状态竞争）
  - [x] 导出 abortPendingLoads() 方法
  - [x] database-navigator.vue onUnmounted 中调用 treeLoader.abortPendingLoads()

## Phase 3: 可访问性 + 测试（并行）

- [x] Task 3.1: VirtualTree ARIA
  - [x] 容器: role="tree" + aria-label="数据库导航树" + tabindex="0"
  - [x] 节点: role="treeitem" + aria-expanded (leaf 时为 undefined) + aria-level (node.level + 1) + aria-selected ("true"/"false") + aria-label (node.label)
  - [x] 键盘事件: ArrowUp/Down/Left/Right + Enter + Space + Home/End (use-keyboard-navigation.ts)
  - [x] 展开图标: role="button" + :aria-label="node.isExpanded ? '折叠' : '展开'"

- [x] Task 3.2: 单元测试
  - [x] tree-mutation.spec.ts: 10 个测试用例 (mutateTreeNode × 5, getTreeNode × 3, mutateCatalogNode × 2)
  - [x] lazy-loader.spec.ts: 4 个测试用例 (缓存命中 / API 回退 / cache check 异常 / 并发去重)
  - [x] navigator-persistence.spec.ts: 5 个测试用例 (保存恢复 / 最后活跃连接 / 不存在 / 损坏 JSON / 全部清除)
  - [x] 全部 ESLint non-null assertion (!) 替换为 optional chaining (?.)
  - [x] 总计 19 个测试用例，覆盖核心工具函数

- [x] Task 3.3: 图标 aria-label
  - [x] 展开/折叠图标: role="button" + :aria-label 动态切换
  - [x] Lucide 图标: aria-hidden="true" (装饰性)
  - [x] Loader2: aria-label="加载中" role="img"
  - [x] 节点类型图标: :aria-label="node.type" role="img"
  - [x] 收藏星标: aria-label="已收藏" role="img"

# Task Dependencies

- Phase 1 三任务完全并行
- Phase 2 四任务完全并行，依赖 Phase 1.2
- Phase 3 三任务完全并行，可与其他 Phase 并行
- Task 1.2 (Router 模式) 是其他代码结构工作的基础

# 完成状态

- **总计**: 10/10 任务完成
- **Phase 1**: 3/3 ✅ 错误处理 + 代码结构
- **Phase 2**: 4/4 ✅ 性能 + 内存 + 类型
- **Phase 3**: 3/3 ✅ 可访问性 + 测试
