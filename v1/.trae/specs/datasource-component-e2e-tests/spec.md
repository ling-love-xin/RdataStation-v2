# 新增数据源模块 — Vue 组件测试 + E2E 全链路测试 Spec

> 状态：✅ **已完成**（2026-06-20）
> 测试报告：[TEST_REPORT.md](../../../src-tauri/tests/TEST_REPORT.md)
> 最佳实践：[BEST_PRACTICES.md](../../../src-tauri/tests/BEST_PRACTICES.md)

## Why

"新增数据源"模块的 composables/adapters/services/stores 层已实现 100% 单元测试覆盖（684 用例），但 8 个 Vue 组件和 Tauri Command 全链路仍是测试真空区。需要在组件渲染层和 IPC 全链路两个维度补全测试覆盖，确保 UI 交互和前后端集成正确性。

## What Changes（已完成）

- ✅ 新增 Vue 组件测试：FieldRenderer、DynamicFormRenderer、TestResultModal、DataSourceHeader、AuthConfigManager、DataSourceSidebar、AddDataSourceDialog、AddDataSourceSidebar（共 8 个，24 用例）
- ✅ 新增 Tauri 全链路集成测试：connect_database、test_connection、create_auth_config、create_network_config（共 4 条链路，15 用例）
- ✅ 新增依赖：`@vue/test-utils`（组件测试）、`@testing-library/vue`（交互测试）、`jsdom`
- ✅ 安全审计：发现 8 个安全问题，修复 6 个，2 个已知限制已记录
- ✅ 后端测试修复：16 项编译错误修复 + 5 项安全缺陷修复
- ✅ 文档更新：TEST_REPORT.md v3.0、BEST_PRACTICES.md v2.0、.trae/rules/README.md v1.6

## Impact

- Affected specs: 无
- Affected code: `src/extensions/builtin/connection/ui/components/__tests__/`（新增）、`src-tauri/tests/`（新增 e2e）

## 实施结果

### Vue 组件测试（8 组件 / 24 用例 / 100% 通过）

| 组件                 | 分层     | 用例数 | 状态 |
| -------------------- | -------- | ------ | ---- |
| FieldRenderer        | L1+L2    | 3      | ✅   |
| DynamicFormRenderer  | L1+L2    | 3      | ✅   |
| TestResultModal      | L1+L2    | 3      | ✅   |
| DataSourceHeader     | L1+L2    | 3      | ✅   |
| AuthConfigManager    | L1+L2    | 3      | ✅   |
| DataSourceSidebar    | L1+L2    | 3      | ✅   |
| AddDataSourceDialog  | L1+L2+L3 | 3      | ✅   |
| AddDataSourceSidebar | L1+L2+L3 | 3      | ✅   |

### E2E Tauri 集成测试（4 链路 / 15 用例 / 100% 通过）

| 链路                  | 用例数 | 状态 |
| --------------------- | ------ | ---- |
| connect_database      | 3      | ✅   |
| test_connection       | 3      | ✅   |
| create_auth_config    | 3      | ✅   |
| create_network_config | 3      | ✅   |
| 数据源 CRUD           | 3      | ✅   |

### 安全审计（8 项发现 / 6 项修复）

| 编号    | 风险    | 状态        |
| ------- | ------- | ----------- |
| SEC-001 | 🔴 高危 | ✅ 已修复   |
| SEC-002 | 🔴 高危 | ✅ 已修复   |
| SEC-003 | 🔴 高危 | ✅ 已修复   |
| SEC-004 | 🟡 中危 | ✅ 已修复   |
| SEC-005 | 🟡 中危 | ✅ 已修复   |
| SEC-006 | 🟡 中危 | ⚠️ 已知限制 |
| SEC-007 | 🟢 低危 | ✅ 已修复   |
| SEC-008 | 🟢 低危 | ⚠️ 已知限制 |

---

## ADDED Requirements（全部完成）

### Requirement: FieldRenderer 组件渲染测试 ✅

FieldRenderer 组件 SHALL 为每种 fieldType 渲染正确的输入控件，并正确处理 passwordVisible 切换和 dependsOn 条件显示。

#### Scenario: 17 种 fieldType 渲染 ✅

- **WHEN** 传入不同 fieldType 的 DriverField
- **THEN** 对应渲染 n-input(text)、n-input-number、n-input(type=password)、n-select、n-switch、n-input(type=textarea)、n-input(file) 等

#### Scenario: 密码可见性切换 ✅

- **WHEN** 点击密码字段右侧的 Eye/EyeOff 图标
- **THEN** input type 在 password/text 之间切换，emit toggle-password 事件

#### Scenario: dependsOn 条件显示 ✅

- **WHEN** dependsOn 字段值不满足条件
- **THEN** 字段不渲染（v-if 为 false）

### Requirement: DynamicFormRenderer 表单渲染测试 ✅

DynamicFormRenderer 组件 SHALL 按 section 分组渲染字段，支持 collapsible 折叠，并管理 passwordVisibility 状态。

#### Scenario: 空 schema → 空渲染 ✅

- **WHEN** schema.sections 为空
- **THEN** 不渲染任何 FieldRenderer

#### Scenario: 多 section 分组 ✅

- **WHEN** schema 包含 connection 和 options 两个 section
- **THEN** 渲染两个分组标题 + 对应字段

#### Scenario: collapsible 折叠 ✅

- **WHEN** section.collapsible 为 true
- **THEN** 渲染折叠箭头，点击可展开/收起

### Requirement: TestResultModal 结果展示测试 ✅

TestResultModal SHALL 根据 test_connection 返回结果展示成功/失败/加载中三种状态。

#### Scenario: 成功状态 ✅

- **WHEN** result.success 为 true
- **THEN** 显示绿色成功图标 + 耗时信息

#### Scenario: 失败状态 ✅

- **WHEN** result.success 为 false
- **THEN** 显示红色失败图标 + 错误消息

### Requirement: AuthConfigManager 认证配置管理测试 ✅

AuthConfigManager SHALL 支持创建/编辑/删除认证配置，并正确传递 scope 和 authType。

#### Scenario: 新建认证配置 ✅

- **WHEN** 填写认证配置表单并保存
- **THEN** 调用 create_auth_config 并更新列表

#### Scenario: 编辑已有配置 ✅

- **WHEN** 选择已有配置修改
- **THEN** 回填表单并调用 update 接口

### Requirement: AddDataSourceDialog 多步骤向导测试 ✅

AddDataSourceDialog SHALL 支持驱动选择 → 连接配置 → 测试连接 → 保存 的多步骤流程。

#### Scenario: 完整向导流程 ✅

- **WHEN** 依次完成驱动选择、填写连接参数、测试连接成功、保存
- **THEN** 每一步状态正确切换，最终 emit apply 事件

### Requirement: AddDataSourceSidebar 侧边栏全流程测试 ✅

AddDataSourceSidebar SHALL 管理 staging 状态同步，并在 apply 时调用 connect_database。

#### Scenario: staging 同步 ✅

- **WHEN** 修改表单字段
- **THEN** staging 状态实时更新

#### Scenario: handleApply 全链路 ✅

- **WHEN** 点击应用按钮
- **THEN** 调用 connect_database 并返回结果

### Requirement: Tauri E2E 全链路 — connect_database ✅

系统 SHALL 支持通过 Tauri invoke 调用 connect_database，完成全局连接和项目连接创建。

#### Scenario: 全局连接创建（SQLite） ✅

- **WHEN** 调用 connect_database 传入 SQLite 参数
- **THEN** 全局 DB 中写入连接记录，返回 ConnectionResponse

#### Scenario: 项目连接创建（SQLite） ✅

- **WHEN** 调用 connect_database 传入 project_id 和 SQLite 参数
- **THEN** 项目 DB 中写入连接记录

### Requirement: Tauri E2E 全链路 — test_connection ✅

系统 SHALL 通过 Tauri invoke 调用 test_connection 测试数据库连通性。

#### Scenario: 连接成功 ✅

- **WHEN** 调用 test_connection 传入有效的 SQLite 文件路径
- **THEN** 返回 { success: true, response_time_ms > 0 }

#### Scenario: 连接失败（脱敏） ✅

- **WHEN** 调用 test_connection 传入无效路径
- **THEN** 返回 { success: false }，错误消息中不包含明文密码

### Requirement: Tauri E2E 全链路 — create_auth_config ✅

系统 SHALL 通过 Tauri invoke 调用 create_auth_config，加密存储密码并返回 AuthConfig。

#### Scenario: 密码认证创建 ✅

- **WHEN** 调用 create_auth_config 传入 password 类型的 auth_data
- **THEN** 密码被 AES-256-GCM 加密（AES: 前缀），返回含 id 的 AuthConfig

### Requirement: Tauri E2E 全链路 — create_network_config ✅

系统 SHALL 通过 Tauri invoke 调用 create_network_config，存储 SSH/SSL/Proxy 配置。

#### Scenario: SSH 配置创建 ✅

- **WHEN** 调用 create_network_config 传入 SSH 配置
- **THEN** 存储完整 config JSON，返回含 id 的 NetworkConfig
