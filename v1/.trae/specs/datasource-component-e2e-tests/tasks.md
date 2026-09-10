# Tasks

## Phase 1: 环境准备

- [x] Task 1: 安装 Vue 组件测试依赖
  - [x] 安装 `@vue/test-utils`、`@testing-library/vue`、`jsdom`
  - [x] 配置 vitest 的 jsdom environment（`vitest.config.ts`）
  - [x] 验证 naive-ui 组件在 jsdom 环境下可渲染
  - [x] 创建 `src/test-setup.ts` mock `window.matchMedia`

## Phase 2: 第一层 — 纯渲染测试（3 个组件，低复杂度）

- [x] Task 2: FieldRenderer 组件渲染测试
  - [x] 每种 fieldType 渲染快照（text/number/password/select/switch/textarea/file/checkbox 等）
  - [x] passwordVisible 切换 → input type 变化
  - [x] Eye/EyeOff 图标切换
  - [x] dependsOn 条件显示/隐藏
  - [x] 必填标记（\*）渲染
  - [x] 验证：`pnpm vitest run src/extensions/builtin/connection/ui/components/__tests__/FieldRenderer.test.ts`

- [x] Task 3: DynamicFormRenderer 表单渲染测试
  - [x] 空 schema → 无渲染
  - [x] 单 section 渲染
  - [x] 多 section 分组渲染
  - [x] collapsible 折叠/展开
  - [x] passwordVisibility 状态传递
  - [x] 验证：`pnpm vitest run src/extensions/builtin/connection/ui/components/__tests__/DynamicFormRenderer.test.ts`

- [x] Task 4: TestResultModal 结果展示测试
  - [x] 成功状态渲染（绿色图标 + 耗时）
  - [x] 失败状态渲染（红色图标 + 错误消息）
  - [x] 加载中状态渲染
  - [x] 验证：`pnpm vitest run src/extensions/builtin/connection/ui/components/__tests__/TestResultModal.test.ts`

## Phase 3: 第二层 — 交互测试（2 个组件，中复杂度）

- [x] Task 5: AuthConfigManager 认证配置管理测试
  - [x] 新建认证配置表单填写
  - [x] 编辑已有配置回填
  - [x] 删除确认
  - [x] scope 切换（global/project）
  - [x] 验证：`pnpm vitest run src/extensions/builtin/connection/ui/components/__tests__/AuthConfigManager.test.ts`

- [x] Task 6: DataSourceHeader 连接状态测试
  - [x] 连接名称展示
  - [x] 连接状态图标切换
  - [x] 验证：`pnpm vitest run src/extensions/builtin/connection/ui/components/__tests__/DataSourceHeader.test.ts`

## Phase 4: 第三层 — 全组件集成测试（3 个组件，高复杂度）

- [x] Task 7: DataSourceSidebar 侧边栏测试
  - [x] 连接列表渲染
  - [x] 搜索过滤
  - [x] 右键菜单操作
  - [x] 验证：`pnpm vitest run src/extensions/builtin/connection/ui/components/__tests__/DataSourceSidebar.test.ts`

- [x] Task 8: AddDataSourceDialog 多步骤向导测试
  - [x] 驱动选择步骤
  - [x] 连接配置填写步骤
  - [x] 测试连接 + 结果展示
  - [x] 保存并关闭
  - [x] 验证：`pnpm vitest run src/extensions/builtin/connection/ui/components/__tests__/AddDataSourceDialog.test.ts`

- [x] Task 9: AddDataSourceSidebar 侧边栏全流程测试
  - [x] staging 状态同步
  - [x] handleApply 全链路（mock connect_database）
  - [x] scope 切换（全局/项目）
  - [x] 验证：`pnpm vitest run src/extensions/builtin/connection/ui/components/__tests__/AddDataSourceSidebar.test.ts`

## Phase 5: Tauri E2E 全链路测试

- [x] Task 10: connect_database 全链路
  - [x] 全局连接创建（SQLite，无需网络）
  - [x] 项目连接创建（SQLite）
  - [x] 验证：`cargo test --test e2e_add_datasource_tests`

- [x] Task 11: test_connection 全链路
  - [x] 连接成功（SQLite 有效路径）
  - [x] 连接失败脱敏（无效路径，错误消息不含明文密码）
  - [x] 验证：`cargo test --test e2e_add_datasource_tests`

- [x] Task 12: create_auth_config 全链路
  - [x] 创建密码认证配置
  - [x] 验证 AES-256-GCM 加密（AES: 前缀）
  - [x] 验证解密后可还原
  - [x] 验证：`cargo test --test e2e_add_datasource_tests`

- [x] Task 13: create_network_config 全链路
  - [x] 创建 SSH 配置
  - [x] 创建 SSL 配置
  - [x] 创建 Proxy 配置
  - [x] 验证：`cargo test --test e2e_add_datasource_tests`

## Phase 6: 最终验证

- [x] Task 14: 运行全部测试并生成报告
  - [x] 前端：`pnpm vitest run`（49 文件，785 用例，0 失败）
  - [x] 后端：`cargo test --test e2e_add_datasource_tests`（14 用例，0 失败）
  - [x] 零失败

# Task Dependencies

- Task 1 是所有前端组件测试的前置依赖
- Task 2、3、4 可并行执行（无相互依赖）
- Task 5、6 可并行执行（无相互依赖）
- Task 7、8、9 依赖 Task 1（需要 mock composables）
- Task 10、11、12、13 可并行执行（无相互依赖）
- Task 14 依赖所有前置任务完成
