# 数据库导航栏全面深度审计

> 版本：v1.0
> 日期：2026-06-09
> 审计范围：前端 15 个文件 + 后端 metadata_commands / cache / 驱动层
> 状态：审计完成，待评审

---

## 一、审计方法论

本次审计覆盖导航栏全链路（VirtualTree UI → composable dispatch → Pinia store → sub-loaders → API → Tauri IPC → Rust commands → cache → driver），识别了以下维度的问题：

| 维度     | 检查项                                                 |
| -------- | ------------------------------------------------------ |
| 错误处理 | try-catch 完整性、silent failure、错误传播链、用户反馈 |
| 性能     | 重复计算、深 watcher、无取消机制、大体积状态           |
| 内存管理 | timer/event listener 清理、组件卸载残留                |
| 代码质量 | 命名误导、未使用变量、配置硬编码、类型安全缺口         |
| 安全性   | SQL 注入、输入校验                                     |
| 架构耦合 | 职责越界、循环依赖、测试可测性                         |

---

## 二、审计发现（已验证）

### 🔴 Critical

#### Issue 1: loadChildren 异常静默吞噬，用户无感知

- **位置**：[use-database-tree-loader.ts#L1002-L1006](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/database/ui/composables/use-database-tree-loader.ts#L1002)
- **分类**：错误处理
- **现状**：

```typescript
// line 1002-1006
} catch (error) {
  console.error('加载树节点失败:', error)
}
return []
```

- **问题**：loadChildren 内部任何一步失败（loadCatalogs / loadSchemas / loadTables 等），catch 块只 `console.error`，然后返回空数组 `[]`。用户展开节点后看到"已展开但无子节点"，没有任何错误提示。
- **影响**：连接断开、缓存损坏、数据库宕机等场景下，用户完全不知道发生了什么，只能看到树节点"展开了但什么都没有"。
- **修复建议**：catch 中调用 `navigatorStore.setNodeError(node.key, ...)` 写入错误消息，让 VirtualTree 渲染错误占位节点，用户可见可重试。

#### Issue 2: debouncedPersistSave setTimeout 未在 onUnmounted 清理

- **位置**：[database-navigator.vue#L1051-L1055](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/database/ui/components/database-navigator.vue#L1051)
- **分类**：内存管理
- **现状**：

```typescript
// line 1057-1062
const debouncedPersistSave = (() => {
  let timer: ReturnType<typeof setTimeout> | null = null
  return (connId: string) => {
    if (timer) clearTimeout(timer)
    timer = setTimeout(() => saveConnectionNavigatorState(connId, {...}), 800)
  }
})()

// line 1051-1055
onUnmounted(() => {
  connectionStatusSync.cleanup()
  cleanupDragDropListeners()
  saveAllNavigatorStates()
  // ❌ debouncedPersistSave 的 timer 未清理
})
```

- **问题**：用户快速切换页面时，debouncedPersistSave 内部的 `setTimeout` 在 800ms 后触发，此时 VirtualTree 已卸载，访问已销毁的响应式数据可能出错。
- **影响**：潜在的内存泄漏 + 卸载后写 localStorage 的无效操作。
- **修复建议**：在 onUnmounted 中调用 `clearTimeout(timer)`，或改为 `let timerCleaned = false` 并在 setTimeout 回调中检查。

### 🟠 High

#### Issue 3: connection 节点的 connectionId 变量语义错误

- **位置**：[use-database-tree-loader.ts#L824](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/database/ui/composables/use-database-tree-loader.ts#L824)
- **分类**：代码质量
- **现状**：

```typescript
// line 820-825
const keyParts = NodeKeyEncoder.decode(node.key)
if (keyParts.length === 0) return []

const nodeType = keyParts[0]
const connectionId = keyParts[1] // ← 对于 'connection' 节点，此处是 scope ('global'/'project')
const dbType = node.data.driver || navigatorStore.getDbType(connectionId) || ''
```

- **问题**：对于 `nodeType === 'connection'` 的节点，`keyParts = ['connection', 'global', 'conn-123']`，所以 `connectionId = 'global'`（scope 字符串），而非真正的 connId。虽然 'connection' 分支（line 830-852）自己取 `connId = keyParts[2]` 并 return，不再使用 line 824 的 `connectionId`，但 line 825 的 `navigatorStore.getDbType('global')` 会返回错误的 dbType。
- **影响**：'connection' 节点展开时，`getNavConfig(dbType)` 可能拿到错误或不存在的配置，导致 NavigationConfig 加载失败。
- **修复建议**：将 line 824-825 移到 switch/if 分叉之后，或在 'connection' 分支中重新赋值：

```typescript
let connectionId: string
let dbType: string
if (nodeType === 'connection') {
  connectionId = keyParts[2] // 正确：'conn-123'
  dbType = node.data.driver || '' // 从 node.data 取
} else {
  connectionId = keyParts[1]
  dbType = node.data.driver || navigatorStore.getDbType(connectionId) || ''
}
```

#### Issue 4: 深 watcher 逐行 map 导致不必要的计算

- **位置**：[database-navigator.vue#L1003-L1012](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/database/ui/components/database-navigator.vue#L1003)
- **分类**：性能
- **现状**：

```typescript
watch(
  () => virtualTreeNodes.value.map(n => ({ key: n.key, isExpanded: n.isExpanded })),
  () => {
    const connId = currentConnection.value?.id
    if (connId) debouncedPersistSave(connId)
  },
  { deep: true }
)
```

- **问题**：每次 VirtualTree 任何节点的任何属性变化，都会重新 map 整个数组再 JSON 比较。100 个节点 × 高频状态更新 = 大量 GC 压力。且 `{ deep: true }` 对 `map()` 返回值无意义（map 返回新数组，每次都是新引用，deep 监控已无效）。
- **修复建议**：改为收集 `expandedKeys` Set → debounce 800ms → 写入 localStorage，避免每次变化都 map 全量：

```typescript
const dirtyKeys = ref(new Set<string>())
// VirtualTree @toggle 回调中: dirtyKeys.value.add(nodeKey)
// debounced watch 只检查 dirtyKeys.size > 0
```

### 🟡 Medium

#### Issue 5: 展开 schema 后 tables-folder 被标记 hasChildren=true 但实际需再 loadTables

- **位置**：[use-database-tree-loader.ts#L877-L895](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/database/ui/composables/use-database-tree-loader.ts#L877)
- **分类**：性能
- **问题**：当展开 schema 节点时（line 867-874），`loadTables` 已获取到该 schema 的所有表数据并存入 connectionCatalogs。但后续用户展开 `tables-folder` 时（line 877），又发起一次 `navigatorStore.loadTables(connectionId, dbName, schemaName)`。第二次调用时 L1/L2 缓存通常命中，不走 DB 查询，但仍然是多余的异步往返。
- **修复建议**：tables-folder 展开时先检查 store 中是否已有 table 数据，有则直接 `createTableNodes()` 而不调 loadTables。

#### Issue 6: NavigatorSearch.searchTables 不支持跨节点高亮跳转

- **位置**：[database-navigator.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/database/ui/components/database-navigator.vue) → `onSearchQueryChange`
- **分类**：UX
- **问题**：搜索结果以列表形式展示，但用户点击搜索结果后，树并不会自动展开到对应节点。需要用户手动一路点开 catalog → schema → tables-folder 才能看到。
- **修复建议**：点击搜索结果时调用 `navigatorStore.expandToNode(path)` 自动展开路径。

#### Issue 7: VirtualTree 缺少 ARIA 无障碍支持

- **位置**：`components/VirtualTree/*.vue`
- **分类**：UX / 可访问性
- **问题**：VirtualTree 组件未使用 `role="tree"` / `role="treeitem"` / `aria-expanded` 等 ARIA 属性，不支持屏幕阅读器。
- **建议**：添加基础 ARIA 属性，满足 WCAG 2.1 AA 标准。

#### Issue 8: 缓存预热 startCacheWarming 未暴露取消机制

- **位置**：[database-navigator-store.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/database/ui/stores/database-navigator-store.ts) → `startCacheWarming`
- **分类**：性能
- **问题**：缓存预热函数分三阶段执行，每个阶段 `Promise.allSettled` 等待所有子任务完成。如果用户在预热中途关闭连接或切换到另一个连接，预热任务继续在后台运行，浪费资源。
- **修复建议**：添加 `AbortController` 或 `isStale(connectionId)` 检查，在每个阶段开始前检查连接是否仍有效。

### 🟢 Low

#### Issue 9: navigator-persistence 未处理 localStorage quota 溢出

- **位置**：[navigator-persistence.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/database/ui/utils/navigator-persistence.ts)
- **问题**：`saveConnectionNavigatorState` 的 catch 块只 `console.warn`，不做降级处理。
- **修复建议**：捕获 QuotaExceededError，清理最旧的连接状态后再重试。

#### Issue 10: use-database-tree-loader.ts 中含 1 处 TODO 未清理

- **位置**：[use-database-tree-loader.ts#L200](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/database/ui/composables/use-database-tree-loader.ts#L200)
- **问题**：`// TODO: 未来懒加载时扩展` 注释标记了未完成的设计。

#### Issue 11: loadChildren 中 getNavConfig 无缓存

- **位置**：[use-database-tree-loader.ts#L826](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/database/ui/composables/use-database-tree-loader.ts#L826)
- **问题**：每次 `loadChildren` 都调用 `getNavConfig(dbType)`，虽然 NavConfig 文件不大，但每个节点展开都读一次 JSON 文件是不必要的。
- **修复建议**：用 `Map<string, NavigationConfig>` 缓存已加载的配置。

---

## 三、审计排除项（已验证不存在）

以下项目在原始扫描中被标记但经验证不存在或不可行：

| 声称的问题                    | 验证结果    | 原因                                                   |
| ----------------------------- | ----------- | ------------------------------------------------------ |
| MySQL driver 中有 unwrap()    | ❌ 不存在   | 当前 mysql.rs 已用 `?` 代替 unwrap                     |
| MySQL list_indexes SQL 注入   | ❌ 不可攻击 | catalog/table 来自 information_schema 查询，非用户输入 |
| Postgres Drop 未 rollback     | ❌ 不存在   | PostgresTransaction Drop 已有 rollback 逻辑            |
| Metadata Cache race condition | ❌ 不存在   | 使用 `tokio::sync::Mutex` 保护 LruCache                |
| 未使用的 computed 属性        | ❌ 不存在   | 所有 computed 均有引用                                 |

---

## 四、模块总体评价

### 得分卡

| 维度         | 评分 | 说明                                                                    |
| ------------ | ---- | ----------------------------------------------------------------------- |
| **错误处理** | 6/10 | 子 loader 层有 per-node 错误追踪（V10.6），但 loadChildren 顶层吞噬异常 |
| **性能**     | 7/10 | L1/L2 cache 分层 + lazy loading 设计优秀，深 watcher 是主要短板         |
| **内存管理** | 7/10 | 基本覆盖，debouncedPersistSave timer 遗漏                               |
| **代码结构** | 8/10 | V10.6 Store 拆分到 4 子 loader + 工具函数，架构清晰                     |
| **类型安全** | 8/10 | shared nav-types + typed specta commands，类型完整                      |
| **可访问性** | 3/10 | VirtualTree 无 ARIA 支持                                                |
| **测试覆盖** | 2/10 | 无单元测试，仅靠 typecheck + lint                                       |

### 架构理解

导航栏模块本质上是 **一个以缓存为中心的懒加载树形数据浏览器**：

```
用户交互 (click expand)
    ↓
loadChildren() — 统一调度器（switch on nodeType）
    ↓
navigatorStore — 状态聚合 + 缓存管理
    ↓
三层缓存策略：connectionCatalogs(Map) → L2 磁盘缓存 → DB 实时查询
    ↓
VirtualTree — 只负责渲染 VirtualTreeNode[]，不关心数据来源
```

关键设计洞察：

1. **配置驱动**：不同数据库（MySQL/PostgreSQL/DuckDB）的树结构通过 `form-schemas/{dbType}.json` 的 `NavigationConfig` 控制，核心代码无硬编码差异。
2. **NodeKeyEncoder** 是贯穿全模块的"身份证系统"：base64(JSON(parts)) 编码，确保节点标识唯一且可逆解。
3. **V10.6 重构**已将 1267 行巨石 Store 拆为 4 个子 loader + 1 个聚合层，职责清晰，但 loadChildren 仍是最大瓶颈（900+ 行 switch）。
4. **Loading 状态独立化**（procedures/functions/sequences/triggers 各自独立 Set）是优秀设计，但 tables 和 views 仍共用同一加载函数，可能造成不必要的重复。
