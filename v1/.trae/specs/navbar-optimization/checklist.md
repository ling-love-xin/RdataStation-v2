# 数据库导航栏优化 — 验证清单

## 编译与类型检查

- [x] `pnpm typecheck` 导航相关 0 errors（预存错误不在本次变更范围内）
- [x] 所有子 loader 模块的 TypeScript imports 正确（统一引用 nav-types.ts）
- [x] 主 Store `database-navigator-store.ts` 的对外接口向后兼容（所有调用方无需修改签名）

## 功能正确性

> 以下需运行时验证，代码层面已全部就绪

- [ ] 展开连接节点 → Catalog 列表正常加载（在线 + L2 缓存）
- [ ] 展开 Catalog 节点 → Schema 列表正常加载（含 `hasSchemas` 和 `hasCatalogs` 两种模式）
- [ ] 展开 Schema 节点 → 对象文件夹正常显示（tables/views/functions/procedures/sequences/triggers）
- [ ] 展开 Tables 文件夹 → 表列表正常加载
- [ ] 展开表节点 → 子文件夹正常显示（columns/indexes/constraints）
- [ ] 展开 Columns 文件夹 → 列列表正常加载
- [ ] 展开 Procedures 文件夹 → 存储过程列表正常加载
- [ ] 展开 Functions 文件夹 → 函数列表正常加载
- [ ] 展开 Sequences 文件夹 → 序列列表正常加载
- [ ] 展开 Triggers 文件夹 → 触发器列表正常加载

## 状态管理

- [x] 同时展开 Procedures 和 Functions 文件夹，加载互不阻塞（4 个独立 loading ref，代码已验证）
- [ ] 展开 Schema A 时 Schema B 错误不影响 Schema A 的显示（nodeErrors 按 key 存储，需运行时验证）
- [x] 同一节点重复展开不发送重复请求（loading 集合的 `has()` 守卫逻辑不变）
- [ ] 离线模式（无 runtime connection）下从 L2 缓存正常加载 catalogs（需运行时验证）
- [x] `connectionCatalogs` 使用直接 `set` 而非创建新 Map（grep 验证无反模式残留）

## 代码质量

- [x] `loadViews` 函数已删除，无残留引用（grep 验证 0 references）
- [x] `loadSchemasFromCache` 仅内部使用（catalog-loader 内部调用，合法保留）
- [x] `loadTablesFromCache` 已不存在（grep 验证 0 references）
- [x] 子 loader 各自代码量合理（use-catalog-loader ~350行，use-table-loader ~300行，use-object-loader ~250行，use-column-loader ~320行）
- [x] 8 处 treeMutation 调用替代手动 catalogs.find → schemas.find 遍历（object-loader 4处 + column-loader 3处 + table-loader 1处）
- [x] `tree-mutation.ts` 有类型安全的 TypeScript 签名

## 错误处理

- [x] 子 loader 按节点 key 写入 `nodeErrors` 而非全局 error 字符串
- [x] API 调用失败时 console.error 包含完整错误信息
- [x] Store 暴露 `getNodeError(key)` / `setNodeError(key, msg)` / `clearNodeError(key)` / `clearAllNodeErrors()` 完整 API

## 文档

- [x] `docs/navigator/README.md` 标注 Store 拆分结构（v2.3）
- [x] `docs/frontend/ARCHITECTURE.md` 补充导航栏子模块架构说明
- [x] `docs/metadata-cache-implementation.md` 记录 V10.6 前端重构
