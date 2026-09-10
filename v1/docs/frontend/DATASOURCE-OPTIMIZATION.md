# 新增数据源模块优化说明

## 一、优化背景

本文档记录了新增数据源模块的优化方案，解决了以下核心问题：

| 优先级 | 问题                                   | 影响                                   |
| ------ | -------------------------------------- | -------------------------------------- |
| **P0** | 只选项目时仍建立全局连接               | 不符合预期行为                         |
| **P0** | 快照失败时继续使用全局 ID              | 数据不一致风险                         |
| **P0** | 缺少双路线保存支持                     | 不支持同时保存到全局和项目             |
| **P1** | StagingItem 字段不一致                 | 数据丢失风险                           |
| **P1** | 两步式保存非原子                       | 可能产生孤立连接                       |
| **P1** | 认证配置保存逻辑不完善                 | 同时保存全局和项目时只保存了项目级配置 |
| **P2** | StagingItem 未持久化                   | 页面刷新后丢失                         |
| **P2** | 缺少类型守卫和验证函数                 | 代码可维护性差                         |
| **P2** | 缺少 URL 验证                          | 可能保存无效连接                       |
| **P0** | `createProjectConnection` 字段名不匹配 | 后端收到 null 或 undefined 值          |
| **P0** | 项目连接保存后未自动建立连接           | 用户需额外操作                         |
| **P0** | 只选「项目」时未自动保存认证配置       | 认证信息丢失                           |
| **P1** | 测试连接时未传递认证配置信息           | 测试不准确                             |
| **P1** | 连接成功后缺少事件通知                 | 导航器不会刷新                         |
| **P2** | 暂存项应用后缺少状态标记               | 用户体验不佳                           |

## 二、优化方案

### 2.1 只选项目场景支持

**问题**：原实现中，即使只选择项目，也会先建立全局连接再保存到项目。

**解决方案**：

```typescript
// 判断保存范围
const shouldSaveGlobal = scope.global || item.scope === 'global'
const shouldSaveProject = scope.project || item.scope === 'project'

// 只选项目时直接保存到项目，不建立全局连接
if (shouldSaveProject && !shouldSaveGlobal) {
  await saveToProjectOnly(item, driverName, url, name, errors, invoke)
  successCount++
  continue
}
```

**修改文件**：`src/extensions/builtin/connection/ui/components/AddDataSourceDialog.vue`

### 2.2 快照失败处理优化

**问题**：快照失败时继续使用全局配置 ID，导致项目依赖全局配置。

**解决方案**：

```typescript
async function snapshotIfNeeded(configId, type, projectPath, name, errors, invoke) {
  if (!configId?.startsWith('G_') || configId.startsWith('GP_')) {
    return configId
  }

  try {
    const r = await invoke(invokeFn, { [paramName]: configId, projectPath })
    return r.snapshot_id
  } catch (e) {
    errors.push(`${name}: ${type === 'auth' ? '认证' : '网络'}配置快照失败`)
    return 'failed' // 返回失败标记，中断保存
  }
}
```

**修改文件**：`src/extensions/builtin/connection/ui/components/AddDataSourceDialog.vue`

### 2.3 双路线保存支持

**问题**：不支持同时保存到全局和项目。

**解决方案**：

```typescript
// 同时保存到全局和项目
if (shouldSaveGlobal) {
  const result = await connectDatabaseService(...)
  globalConnId = result.conn_id
}

if (shouldSaveProject && projectStore.hasProject) {
  await saveToProject(...)
}
```

**修改文件**：`src/extensions/builtin/connection/ui/components/AddDataSourceDialog.vue`

### 2.4 StagingItem 字段统一

**问题**：`saveToStaging` 和 `syncCurrentToStaging` 字段集合不一致。

**解决方案**：抽取统一的 `buildStagingItem` 函数：

```typescript
function buildStagingItem(
  name,
  driver,
  driverId,
  url,
  formData,
  authConfigId,
  authMethod,
  networkConfigId,
  driverProperties,
  advancedOptions,
  environmentId,
  description,
  schemaName,
  options,
  metadataPath,
  tags,
  useDuckdbFed
) {
  return {
    name,
    driver,
    driverId,
    url,
    formData,
    authConfigId,
    authMethod,
    networkConfigId,
    driverProperties,
    advancedOptions,
    environmentId,
    scope: scope.global ? 'global' : 'project',
    description,
    schemaName,
    options,
    metadataPath,
    tags,
    useDuckdbFed,
  }
}
```

**修改文件**：`src/extensions/builtin/connection/ui/composables/useAddDataSource.ts`

### 2.5 原子性保障

**问题**：项目连接保存分两步执行，可能产生孤立连接。

**解决方案**：使用 try-catch 补偿机制：

```typescript
let globalConnId = null
try {
  // Step 1: 建立全局连接
  const result = await connectDatabaseService(...)
  globalConnId = result.conn_id

  // Step 2: 保存到项目
  await saveToProject(...)
} catch (e) {
  // 回滚：关闭已建立的全局连接
  if (globalConnId) {
    await closeConnection(globalConnId)
  }
  throw e
}
```

**修改文件**：`src/extensions/builtin/connection/ui/components/AddDataSourceDialog.vue`

### 2.6 StagingItem 持久化

**问题**：StagingItem 仅存于内存，页面刷新后丢失。

**解决方案**：使用 localStorage 持久化：

```typescript
const STAGING_STORAGE_KEY = 'rdata-station-staging-items'

function loadStagingItems() {
  const stored = localStorage.getItem(STAGING_STORAGE_KEY)
  if (stored) {
    stagingItems.value = JSON.parse(stored)
  }
}

function saveStagingItems() {
  localStorage.setItem(STAGING_STORAGE_KEY, JSON.stringify(stagingItems.value))
}

// 监听变化自动保存
watch(stagingItems, saveStagingItems, { deep: true })
```

**修改文件**：`src/extensions/builtin/connection/ui/composables/useAddDataSource.ts`

### 2.7 认证配置保存优化

**问题**：同时保存全局和项目时，只保存了项目级认证配置。

**解决方案**：分别保存全局和项目级认证配置：

```typescript
if (shouldSaveGlobal && shouldSaveProject) {
  const globalId = await invokeTauri('create_auth_config', {
    ac: { name: `${authName} (全局)`, ... }
  })

  const projectId = await invokeTauri('project_create_auth_config', {
    name: `${authName} (项目)`, ...
  })

  authConfigId.value = projectId.id
  message.info(t('navigator.authSavedHint', { global: true, project: true }))
}
```

**修改文件**：`src/extensions/builtin/connection/ui/components/AddDataSourceDialog.vue`

### 2.8 类型守卫和验证函数

**问题**：缺少类型守卫和验证函数，代码可维护性差。

**解决方案**：在 `useAddDataSource.ts` 中添加以下辅助函数：

```typescript
// 类型守卫
function isValidStagingItem(item: unknown): item is StagingItem
function isFileDatabase(driverId: string): boolean
function needsSnapshot(configId: string | null): boolean

// 验证函数
function validateUrl(url: string): { valid: boolean; error?: string }
function validatePort(port: number): { valid: boolean; error?: string }
function validateHost(host: string): { valid: boolean; error?: string }
function validateExtended(): ExtendedValidationResult

// 连接字符串构建
function buildJdbcUrl(driverId: string, host: string, port: number, database: string): string
function buildStandardUrl(driverId: string, host: string, port: number, database: string): string
function extractDatabaseFromUrl(url: string): string | null
function extractHostAndPort(url: string): { host: string; port: number } | null
```

**修改文件**：`src/extensions/builtin/connection/ui/composables/useAddDataSource.ts`

### 2.9 认证方式判断优化

**问题**：缺少判断认证方式是否需要凭据的辅助函数。

**解决方案**：

```typescript
function isAuthRequired(authMethod: string): boolean {
  return ['password', 'ldap', 'pg_class', 'kerberos', 'oauth2'].includes(authMethod)
}
```

**修改文件**：`src/extensions/builtin/connection/ui/components/AddDataSourceDialog.vue`

### 2.10 StagingItem 管理统一

**问题**：`AddDataSourceDialog.vue` 和 `useAddDataSource.ts` 中重复实现了 StagingItem 管理功能。

**解决方案**：

- 在 `useAddDataSource.ts` 中实现完整的 StagingItem 管理功能
- 在 `AddDataSourceDialog.vue` 中使用 `useAddDataSource` 提供的功能，避免代码重复
- 添加了 `isResetting` 状态来防止重置时的循环 watch 触发
- 添加了 `applyStagingItem` 函数来统一恢复表单状态
- 添加了 `clearStagingItems` 函数来清空暂存项

**修改文件**：`src/extensions/builtin/connection/ui/components/AddDataSourceDialog.vue`, `src/extensions/builtin/connection/ui/composables/useAddDataSource.ts`

### 2.11 字段名一致性修复

**问题**：前端调用 `create_project_connection` 时使用 `camelCase` 字段，后端期望 `snake_case`，导致字段值为 null 或 undefined。

**解决方案**：统一使用 `snake_case` 字段名与后端通信：

```typescript
// 修复前（错误）
{
  projectPath: input.project_path,
  schemaName: input.schema_name,
  ...
}

// 修复后（正确）
{
  project_path: input.project_path,
  schema_name: input.schema_name,
  ...
}
```

**修改文件**：`src/extensions/builtin/connection/ui/services/project-connection.ts`

### 2.12 项目连接自动建立

**问题**：项目连接保存到数据库后，用户需要手动点击「连接」才能建立实际连接。

**解决方案**：保存项目连接后自动调用 `connectDatabaseService`：

```typescript
async function saveToProject(...) {
  // 先保存到项目数据库
  const conn = await projectConnectionStore.createConnection(...)
  // 刷新连接列表
  await projectConnectionStore.loadConnections()
  // 自动建立项目连接
  if (pp) {
    await connectDatabaseService(
      driverName, url, name, 'project', pp, buildConnectOpts(...)
    )
  }
  return conn
}
```

**修改文件**：`src/extensions/builtin/connection/ui/components/AddDataSourceDialog.vue`

### 2.13 只选项目时自动保存认证配置

**问题**：只选「项目」时，如果填写了认证信息但未选择已保存的认证配置，认证配置不会保存。

**解决方案**：在 `saveToProjectOnly` 中添加认证配置保存逻辑：

```typescript
// 如果没有使用已保存的认证配置，且用户填写了认证信息，则保存新的认证配置
let finalAuthConfigId = snapshotAuthId
let finalAuthMethod = item.authMethod ?? authMethod.value
const hasAuthData = (fd.username && fd.password) || fd.certPath || fd.principal
if (!finalAuthConfigId && hasAuthData) {
  const authName = `${name} (认证)`
  const authData = buildAuthData(finalAuthMethod, fd as Record<string, unknown>)
  const r = await invoke('project_create_auth_config', {
    name: authName,
    authType: finalAuthMethod,
    authData: JSON.stringify(authData),
    projectPath: pp,
  })
  finalAuthConfigId = r.id
}
```

**修改文件**：`src/extensions/builtin/connection/ui/components/AddDataSourceDialog.vue`

### 2.14 测试连接传递认证配置

**问题**：测试连接时没有传递 `authConfigId` 和 `authMethod`，导致测试不准确。

**解决方案**：在 `handleTest` 中添加认证配置信息：

```typescript
const params: Record<string, unknown> = {
  dbType: dbType,
  url,
}
if (networkConfigId.value) params.networkConfigId = networkConfigId.value
if (authConfigId.value) params.authConfigId = authConfigId.value
if (authMethod.value) params.authMethod = authMethod.value
```

**修改文件**：`src/extensions/builtin/connection/ui/components/AddDataSourceDialog.vue`

### 2.15 连接成功事件通知

**问题**：连接成功后没有 emit 事件通知父组件，导航器不会刷新。

**解决方案**：在 `handleApply` 成功后 emit 事件：

```typescript
if (successCount > 0) {
  emit('save')
  emit('connectionsChanged')
  resetAndClose()
}
```

**修改文件**：`src/extensions/builtin/connection/ui/components/AddDataSourceDialog.vue`

### 2.16 StagingItem 添加 id 和 applied 字段

**问题**：暂存项缺少唯一 ID，无法准确定位，且应用后没有状态标记。

**解决方案**：

```typescript
export interface StagingItem {
  id: string  // 新增唯一标识
  name: string
  ...
  applied?: boolean  // 新增应用状态标记
}

function buildStagingItem(...) {
  return {
    id: uuidv4(),
    ...,
    applied: false,
  }
}

function markStagingApplied(index: number) {
  if (stagingItems.value[index]) {
    stagingItems.value[index].applied = true
  }
}
```

**修改文件**：`src/extensions/builtin/connection/ui/composables/useAddDataSource.ts`

### 2.17 buildConnectOpts 统一连接选项构建

**问题**：连接选项构建分散在多处，代码重复。

**解决方案**：抽取统一的 `buildConnectOpts` 函数：

```typescript
function buildConnectOpts(
  item: StagingItem,
  networkConfigId: string | null,
  authConfigId: string | null
) {
  return {
    driverId: item.driverId,
    authConfigId: authConfigId ?? undefined,
    authMethod: item.authMethod,
    networkConfigId: networkConfigId ?? undefined,
    driverProperties: item.driverProperties,
    advancedOptions: item.advancedOptions,
    environmentId: item.environmentId ?? undefined,
    description: item.description,
  }
}
```

**修改文件**：`src/extensions/builtin/connection/ui/components/AddDataSourceDialog.vue`

## 三、验证方法

### 3.1 只选项目场景验证

1. 打开添加数据源对话框
2. 取消勾选"全局"
3. 勾选"项目"
4. 填写连接信息并应用
5. 预期：连接只保存到项目，不建立全局连接

### 3.2 快照失败处理验证

1. 添加一个引用全局认证配置的项目连接
2. 手动删除该全局认证配置
3. 尝试应用项目连接
4. 预期：保存失败，提示快照失败错误

### 3.3 双路线保存验证

1. 同时勾选"全局"和"项目"
2. 填写连接信息并应用
3. 检查全局连接列表和项目连接列表
4. 预期：两处都有新增的连接

### 3.4 StagingItem 字段统一验证

1. 填写完整的连接信息（包括 schemaName、tags、useDuckdbFed 等）
2. 保存到暂存
3. 切换到其他暂存项再切换回来
4. 检查所有字段是否都恢复

### 3.5 原子性验证

1. 添加一个项目连接
2. 在第二步（项目保存）时模拟失败
3. 检查全局连接池
4. 预期：没有孤立的全局连接

### 3.6 StagingItem 持久化验证

1. 添加几个暂存项
2. 刷新页面
3. 检查暂存列表
4. 预期：暂存项恢复

### 3.7 StagingItem 管理统一验证

1. 打开添加数据源对话框
2. 添加几个暂存项，填写不同的信息
3. 在暂存项之间切换，检查状态是否正确恢复
4. 关闭对话框并重新打开，检查暂存项是否持久化
5. 检查代码中是否存在重复的 StagingItem 管理逻辑

### 3.8 项目连接自动建立验证

1. 只勾选「项目」
2. 填写完整的连接信息
3. 点击「应用」
4. 验证连接列表中该连接已自动连接

### 3.9 只选项目时自动保存认证配置验证

1. 只勾选「项目」
2. 填写认证信息但不选择已保存的认证配置
3. 点击「应用」
4. 检查项目级认证配置中是否已新增一条记录

### 3.10 测试连接传递认证配置验证

1. 选择一个已保存的认证配置
2. 点击「测试连接」
3. 检查后端日志，确认 `authConfigId` 和 `authMethod` 已正确传递

### 3.11 连接成功事件通知验证

1. 在父组件中监听 `connectionsChanged` 事件
2. 添加一个连接并点击「应用」
3. 验证事件是否触发

### 3.12 StagingItem 状态标记验证

1. 添加并应用一个连接
2. 检查暂存项是否标记为「已应用」

## 四、修改文件清单

| 文件                                                                      | 修改类型 | 说明                                                                                       |
| ------------------------------------------------------------------------- | -------- | ------------------------------------------------------------------------------------------ |
| `src/extensions/builtin/connection/ui/components/AddDataSourceDialog.vue` | 修改     | 修复 handleApply 逻辑，添加认证配置保存优化，统一使用 useAddDataSource 的 StagingItem 管理 |
| `src/extensions/builtin/connection/ui/composables/useAddDataSource.ts`    | 修改     | 添加 StagingItem 管理和持久化，添加验证函数和类型守卫，添加连接字符串构建函数              |
| `src/extensions/builtin/connection/ui/services/project-connection.ts`     | 修改     | 修复 createProjectConnection 字段名从 camelCase 改为 snake_case，与后端匹配                |
| `docs/frontend/DATASOURCE-OPTIMIZATION.md`                                | 新增     | 优化文档                                                                                   |

## 五、版本记录

| 版本 | 日期       | 说明                                                                                       |
| ---- | ---------- | ------------------------------------------------------------------------------------------ |
| v1.0 | 2026-05-26 | 初始优化方案                                                                               |
| v1.1 | 2026-05-26 | 添加字段名一致性修复，确保与后端匹配                                                       |
| v1.2 | 2026-05-26 | 添加自动连接建立、认证配置自动保存、事件通知、测试连接增强、StagingItem id 和 applied 状态 |
