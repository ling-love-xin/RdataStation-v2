# 导航栏卓越优化 — 验证清单

## 错误处理 (目标 10/10)

- [ ] 全部 4 个子 loader catch 块统一 setNodeError + rethrow 协议
- [ ] loadChildren catch → createErrorPlaceholderNode 渲染
- [ ] 用户可见错误节点含 "重试" 交互
- [ ] 连接断开 → 重连 → 树节点自动恢复

## 代码结构 (目标 10/10)

- [ ] loadChildren ≤ 40 行（Router 模式）
- [ ] nav-router.ts 路由表覆盖全部 VirtualTreeNodeType
- [ ] 0 个 TODO/FIXME 残留

## 性能 (目标 10/10)

- [ ] getNavConfig 第二次调用命中内存缓存（无文件 I/O）
- [ ] searchTables 在 1000+ 表场景 ≤ 50ms
- [ ] tables/views-folder 同 schema 展开无重复 loadTables 请求

## 类型安全 (目标 10/10)

- [ ] 0 个 `as` 类型断言（全部用类型守卫替代）
- [ ] node.data 访问前用 discriminated union 类型缩窄
- [ ] pnpm typecheck 0 errors（含新增测试文件）

## 内存管理 (目标 10/10)

- [ ] onUnmounted → abort 全部进行中的请求
- [ ] 全部 setTimeout/Interval 清理
- [ ] 无内存泄漏警告

## 可访问性 (目标 10/10)

- [ ] VirtualTree: role="tree" + role="treeitem" + aria-expanded + aria-level
- [ ] 键盘导航: ArrowUp/Down/Left/Right/Enter/Home/End 全部响应
- [ ] WAVE / axe DevTools 扫描 0 严重错误
- [ ] 全部交互图标有 aria-label

## 测试覆盖 (目标 10/10)

- [ ] tree-mutation.spec.ts: ≥6 用例, ≥80% 分支覆盖率
- [ ] lazy-loader.spec.ts: ≥4 用例, ≥80% 分支覆盖率
- [ ] navigator-persistence.spec.ts: ≥4 用例, ≥80% 分支覆盖率
- [ ] vitest run 全部通过
- [ ] pnpm lint 0 errors
