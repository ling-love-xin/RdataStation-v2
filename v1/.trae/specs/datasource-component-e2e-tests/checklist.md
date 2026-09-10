# Checklist

## Phase 1: 环境准备

- [x] `@vue/test-utils` 已安装且版本合适
- [x] `@testing-library/vue` 已安装且版本合适
- [x] `jsdom` 已安装
- [x] vitest.config 已配置 jsdom environment
- [x] naive-ui 组件在测试环境可渲染（无 import 错误）
- [x] `src/test-setup.ts` 已创建，mock `window.matchMedia`

## Phase 2: 第一层 — 纯渲染测试

- [x] FieldRenderer 所有 17 种 fieldType 均有渲染测试
- [x] FieldRenderer passwordVisible 切换测试通过
- [x] FieldRenderer dependsOn 条件显示测试通过
- [x] DynamicFormRenderer 空/单/多 section 测试通过
- [x] DynamicFormRenderer collapsible 折叠测试通过
- [x] TestResultModal 成功/失败/加载中三种状态测试通过

## Phase 3: 第二层 — 交互测试

- [x] AuthConfigManager 新建/编辑/删除 流程测试通过
- [x] AuthConfigManager scope 切换测试通过
- [x] DataSourceHeader 连接名称/状态图标测试通过

## Phase 4: 第三层 — 全组件集成测试

- [x] DataSourceSidebar 连接列表/搜索/右键菜单测试通过
- [x] AddDataSourceDialog 多步骤向导测试通过（驱动选择→配置→测试→保存）
- [x] AddDataSourceSidebar staging 同步测试通过
- [x] AddDataSourceSidebar handleApply 全链路测试通过

## Phase 5: Tauri E2E

- [x] connect_database 全局连接（SQLite）测试通过
- [x] connect_database 项目连接（SQLite）测试通过
- [x] test_connection 连接成功测试通过
- [x] test_connection 失败脱敏测试通过（错误消息不含明文密码）
- [x] create_auth_config 密码加密存储（AES: 前缀）测试通过
- [x] create_auth_config 解密还原测试通过
- [x] create_network_config SSH/SSL/Proxy 三种类型测试通过

## Phase 6: 最终验证

- [x] 前端全部测试通过（49 文件，785 用例，0 失败）
- [x] 后端 E2E 测试通过（14 用例，0 失败）
- [x] 无 ESLint/TypeScript 错误
- [x] 无 Rust 编译错误/clippy 警告
