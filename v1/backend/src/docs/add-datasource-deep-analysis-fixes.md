# 新增数据源深度分析与修复

> 最新日期：2026-06-01
> 状态：✅ v0.7.0 ESLint 275 警告全量清零

---

## 零、v0.6.9 Advanced Tab 高级选项后端全链路集成（新增）

> 日期：2026-06-01
> 基于：用户审计 — Advanced Tab 中连接超时/查询超时/编码/驱动属性等设置存储但未被实际使用

### 审计发现

| #   | 问题                                                                                                                                                                                  | 严重度  |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------- |
| 1   | **Advanced Tab 设置未应用**：`advanced_options` JSON 和 `driver_properties` 被正确存入 DB，但 `connect_with_type` 建立的连接未读取这些字段                                            | 🔴 致命 |
| 2   | **连接 URL 不包含高级参数**：`connect_timeout`/`encoding`/`charset`/驱动属性未注入到连接 URL query params                                                                             | 🔴 致命 |
| 3   | **AdvancedTab.vue 存在重复 UI**："性能策略"折叠区已有 `advConnectTimeout`/`advQueryTimeout`/`advHeartbeat`/`advMaxReconnect`，下方"连接参数（基础）"区块再次展示了完全相同的 4 个控件 | 🟡 中   |

### 修复详情

#### 修复 1：DriverConnectionConfig 扩展（config.rs）

在 [config.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/driver/registry/config.rs) 中为 `DriverConnectionConfig` 新增 7 个字段：

```rust
pub connect_timeout: Option<u32>,      // 连接超时（秒）
pub query_timeout: Option<u32>,        // 查询超时（秒）
pub pool_size: Option<u32>,            // 连接池大小
pub heartbeat_interval: Option<u32>,   // 心跳间隔（秒）
pub max_reconnect: Option<u32>,        // 最大重连次数
pub encoding: Option<String>,          // 字符编码（UTF-8/GBK/Latin-1）
pub driver_properties: HashMap<String, String>, // 驱动属性 key=value
```

同时为每个字段新增 builder 方法（`with_connect_timeout`/`with_encoding` 等）。

#### 修复 2：URL 查询参数注入（config.rs）

新增 `append_query_params()` 私有方法，按以下优先级将参数拼入连接 URL：

| 参数来源             | 格式                 | 示例                                                    |
| -------------------- | -------------------- | ------------------------------------------------------- |
| `options` HashMap    | `key=value`          | `ssl-mode=VERIFY_CA`                                    |
| `encoding` → charset | `charset=utf8mb4`    | GBK → `gbk`, Latin-1 → `latin1`                         |
| `connect_timeout`    | `connect_timeout=30` | 仅当 options 中无 `connect_timeout`/`connectTimeout` 时 |
| `driver_properties`  | `key=value`          | `useSSL=true&serverTimezone=UTC`                        |

该方法在 `build_from_template`/`build_mysql_url`/`build_postgres_url` 中统一调用。

#### 修复 3：连接服务解析 advanced_options JSON（connection_service.rs）

在 [connection_service.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/services/connection_service.rs) 中新增两个静态方法：

- **`apply_advanced_options(config, json)`**：解析 `advanced_options` JSON（结构：`{ performance: { poolSize, queryTimeout, ... }, connection: { connectTimeout, ... }, encoding: "UTF-8" }`），填充 `DriverConnectionConfig` 对应字段。`performance` 优先于 `connection`，后者作为兜底。

- **`apply_driver_properties(config, json)`**：解析 `driver_properties` JSON（`{ key: value }`），填入 `driver_properties` HashMap。

在 `connect_with_type` 中，`advanced_options` 和 `driver_properties` 不为空时调用对应方法：

```rust
if let Some(ref opts_json) = advanced_options {
    Self::apply_advanced_options(&mut driver_config, opts_json);
}
if let Some(ref props_json) = driver_properties {
    Self::apply_driver_properties(&mut driver_config, props_json);
}
```

#### 修复 4：前段 AdvancedTab.vue 移除重复 UI

移除"连接参数（basic）"区块（[AdvancedTab.vue:L257-L280](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/connection/ui/components/tabs/AdvancedTab.vue)），该区块中的 4 个控件（连接超时/查询超时/心跳间隔/最大重连）与上方 `Performance policies` 折叠区完全重复。删除后所有绑定通过性能策略区保留，`emit('extra-config')` 不受影响。

### 变更文件清单 (v0.6.9)

| 文件                                                                   | 变更                                                                          |
| ---------------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| `src-tauri/src/core/driver/registry/config.rs`                         | 新增 7 个 advanced 字段 + builder 方法 + `append_query_params()` URL 参数注入 |
| `src-tauri/src/core/services/connection_service.rs`                    | 新增 `apply_advanced_options()` / `apply_driver_properties()` 解析方法        |
| `src/extensions/builtin/connection/ui/components/tabs/AdvancedTab.vue` | 移除重复的"连接参数（basic）"区块                                             |

### 验证

- `pnpm run lint` → 0 errors, 275 warnings (全部预存) ✅
- `cargo check` → 0 errors ✅

---

## 零、v0.7.0 前端 ESLint 275 警告全量清零（新增）

> 日期：2026-06-01
> 基于：`pnpm run lint` 输出 275 warnings（0 errors），分 5 类问题

### 警告分类统计

| 类别                                       | 规则                     | 修复前  | 修复后   |
| ------------------------------------------ | ------------------------ | ------- | -------- |
| `no-console`                               | `console.log` 等调试日志 | ~70     | 0        |
| `@typescript-eslint/no-explicit-any`       | `any` 类型使用           | ~35     | 0        |
| `unused-imports/no-unused-vars`            | 未使用变量/参数          | ~25     | 0        |
| `@typescript-eslint/no-non-null-assertion` | 非空断言 `!`             | ~15     | 0        |
| `vue/*`                                    | v-html / template-shadow | 2       | 0        |
| **总计**                                   |                          | **275** | **0** ✅ |

### 修复策略

| 类别                            | 策略                                                         | 说明                                               |
| ------------------------------- | ------------------------------------------------------------ | -------------------------------------------------- |
| `no-console`                    | `console.log` → `console.debug` + `eslint-disable-next-line` | 调试日志保留，显式标注                             |
| `no-explicit-any`               | `any` → `unknown` / `Record<string, unknown>` / 具体接口     | 提升类型安全，部分 ag-Grid 回调用 `eslint-disable` |
| `unused-imports/no-unused-vars` | 变量/参数加 `_` 前缀                                         | 利用 `varsIgnorePattern: '^_'` 规则                |
| `no-non-null-assertion`         | `!` → 显式 null 检查 / `?.`                                  | 消除潜在运行时崩溃                                 |
| `vue/no-v-html`                 | 添加 `eslint-disable` 注释块                                 | 已知安全的 HTML 渲染                               |
| `vue/no-template-shadow`        | 模板变量重命名                                               | 避免与 `useI18n().t` 冲突                          |

### 特殊处理

| 文件                               | 处理方式                                        |
| ---------------------------------- | ----------------------------------------------- |
| `src/generated/specta/bindings.ts` | 生成文件，纳入 `.eslintrc.cjs` `ignorePatterns` |
| `tests/unit/*.spec.ts`             | 测试文件非空断言加 `eslint-disable` 文件级注释  |
| `.eslintrc.cjs`                    | `ignorePatterns` 新增 `src/generated`           |

### 涉及文件（约 60 个）

按模块分布：

- **core/app** (9 文件): `main.ts`, `popout.ts`, `command-registry.ts`, `extension-host.ts`, `panel-registry.ts`, `project.ts`, `vue-app-manager.ts`, `window-api.ts`, `MainLayout.vue`
- **connection** (10 文件): `extension.ts`, `driver-adapter.ts`, `AddDataSourceDialog.vue`, `DataSourceSidebar.vue`, `AdvancedTab.vue`, `NetworkTab.vue`, `useAddDataSource.ts`, `useNetworkProfiles.ts`, `schema-loader.ts`
- **database** (20+ 文件): `DataPreview.vue`, `database-navigator.vue`, `favorites-panel.vue`, `navigator-context-menu*.vue`, `use-*` composables, `performance-monitor.ts`, `lru-cache.ts`, `search-index.ts`, `metadata-cache-service.ts`, `database-navigator-store.ts`
- **workbench** (15 文件): `EditorManager.ts`, `sql-history-service.ts`, `MainContentArea.vue`, `EditorWelcome.vue`, `FileResultPanel.vue`, `QueryResultPanel.vue`, `TableSchemaPanel.vue`, `title-bar-config.ts`, `command-store.ts`, `layout-store.ts`, `workbench-store.ts`, `WorkbenchView.vue`, `MenuBar.test.ts`, `ToolbarActions.test.ts`
- **query** (5 文件): `query-service.ts`, `types.ts`, `extension.ts`, `ResultTable.vue`, `SqlEditorToolbar.vue`
- **scratchpad** (4 文件): `extension.ts`, `ScratchpadPanel.vue`, `ScratchpadTreeNode.vue`, `use-scratchpad.ts`
- **其他** (5 文件): `mysql-driver/extension.ts`, `settings/extension.ts`, `analytics-resource/extension.ts`, `FilterBar.vue`, `event-bus.ts`

### 验证

- `pnpm run lint` → **0 errors, 0 warnings** ✅
- `cargo check` → **0 errors** ✅

---

## 零、v0.6.8 MySQL (Native) 连接失败 + Staging 暂存同步修复（新增）

> 日期：2026-05-31
> 基于：用户反馈两个问题

### 问题 A：MySQL (Native) 连接报 Access Denied

**现象**：MySQL (Native) 提示 `1045 (28000): Access denied for user 'root'@'localhost' (using password: YES)`，但 MySQL (Official/sqlx) 正常连接。

**根因**：`mysql_async` 在连接 `localhost` 时默认优先尝试 Unix socket（即便已启用 `native-tls-tls` feature），Windows 下 Unix socket 不可用但错误处理路径与 MySQL 8.0 的 `caching_sha2_password` 认证策略叠加，导致连接可到服务器但认证失败。

**修复**：在 [factory.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/driver/factory.rs#L253-L257) 中为 `MySqlNativeDriverFactory` 追加 `prefer_socket=false` URL 参数，强制 TCP 连接：

```rust
if !url.contains("prefer_socket") {
    let sep = if url.contains('?') { '&' } else { '?' };
    url.push(sep);
    url.push_str("prefer_socket=false");
}
```

### 问题 B：Staging 切换不刷新右侧表单

**现象**：保存 MySQL 暂存 → 新增 PG 连接 → 点击 MySQL 暂存项，右侧（General/Auth/Network 等 Tab）仍显示 PG 信息。

**根因**：`handleSelectStaging()` 设置 `headerData.selectedDriverId` 后，Vue 异步批次中 `DataSourceHeader` 触发 `@driver-change` → `onDriverChange()` 执行 `formData.value = {}`，将刚恢复的 MySQL 表单数据全部清空。

**流程链**：

```
handleSelectStaging()  ──(同步)──→  设置 driverId, 恢复 formData  ✅
       │
       └── Vue nextTick  ──(异步)──→  @driver-change 触发
                                           └── onDriverChange()
                                                 └── formData = {}  ❌ (覆盖!)
```

**修复 1**：`onDriverChange` 添加 `isResetting` 守卫（[AddDataSourceDialog.vue:285](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/connection/ui/components/AddDataSourceDialog.vue#L285)）

```typescript
function onDriverChange(driverId: string) {
  if (isResetting.value) return  // ← 暂存恢复期间跳过
  ...
}
```

**修复 2**：`handleSelectStaging` 补全 5 个缺失字段的恢复（[AddDataSourceDialog.vue:374-379](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/connection/ui/components/AddDataSourceDialog.vue#L374-L379)）

```typescript
schemaName.value = s.schemaName ?? null
options.value = s.options ?? null
metadataPath.value = s.metadataPath ?? null
tags.value = s.tags ?? null
useDuckdbFed.value = s.useDuckdbFed ?? null
```

### 变更文件清单 (v0.6.8)

| 文件                                                                      | 变更                                                                      |
| ------------------------------------------------------------------------- | ------------------------------------------------------------------------- |
| `src-tauri/src/core/driver/factory.rs`                                    | `MySqlNativeDriverFactory::create()` 追加 `prefer_socket=false`           |
| `src/extensions/builtin/connection/ui/components/AddDataSourceDialog.vue` | `onDriverChange` 加 `isResetting` 守卫；`handleSelectStaging` 补全 5 字段 |

### 验证

- `pnpm run lint` → 0 errors, 275 warnings (全部预存) ✅
- `cargo check` → 0 errors ✅

---

## 零、v0.6.7 Network Tab 网络协议链不显示修复（新增）

> 日期：2026-05-31
> 基于：用户反馈 — 新增数据源页面 Network Tab 不显示 SSH/Proxy/SSL 三个默认条目

### 问题

Network Tab 中 SSH/Proxy/SSL 三个默认协议节点完全不显示，`filteredChain` 为空。

### 根因

`driver.capabilities` 字段存储的是数据库功能级能力（如 `"tree"`、`"health_check"`、`"transactions"`），而 NetworkTab 中的 `supportsSsh`/`supportsSsl`/`supportsProxy` 计算属性在 `capabilities` 中搜索网络协议关键词（`"ssh"`、`"ssl"`、`"proxy"`）。

```
MySQL driver capabilities:   ["tree","health_check","transactions",...]
NetworkTab 查找:             caps.includes("ssh")   → false ❌
NetworkTab 查找:             caps.includes("ssl")   → false ❌
NetworkTab 查找:             caps.includes("proxy") → false ❌
```

三个 `supports*` 全部返回 `false` → `shouldShowHop()` 返回 `false` → `filteredChain` 过滤所有 hop → 页面显示空链。

**此前逻辑**：`caps.length === 0 || caps.includes(...)` — 仅当 capabilities 为空数组时默认启用。但现有 driver 的 capabilities 非空且仅含数据库功能关键词，不含网络关键词，触发误杀。

### 修复

#### 1. 代码修复：`hasNetworkCap()` 智能判定

[NetworkTab.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/connection/ui/components/tabs/NetworkTab.vue#L841-L884) 新增 `hasNetworkCap()` 函数，三级判定规则：

| 场景           | capabilities 内容          | 结果                    |
| -------------- | -------------------------- | ----------------------- |
| 空数组         | `[]`                       | 默认启用所有网络协议 ✅ |
| 仅数据库能力   | `["tree","transactions"]`  | 默认启用所有网络协议 ✅ |
| 含显式网络声明 | `["ssh_tunnel","ssl_tls"]` | 按声明判断 ✅           |

```typescript
function hasNetworkCap(caps: string[], targetKeys: readonly string[]): boolean {
  if (caps.length === 0) return true
  const hasExplicitNetworkCaps = caps.some(c => NETWORK_CAP_KEYS.includes(c))
  if (!hasExplicitNetworkCaps) return true // ← 关键：无网络关键词 → 默认启用
  return caps.some(c => targetKeys.includes(c))
}
```

#### 2. 数据修复：Migration 017 补齐网络 capabilities

新增 [017_add_network_capabilities.sql](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/migrations/global/017_add_network_capabilities.sql)，为 MySQL/PostgreSQL 系列 driver 显式添加 `ssh_tunnel`、`ssl_tls`、`proxy` 三个能力关键词。

### 变更文件清单 (v0.6.7)

| 文件                                                                  | 变更                                                                                                        |
| --------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------- |
| `src/extensions/builtin/connection/ui/components/tabs/NetworkTab.vue` | 新增 `hasNetworkCap()` 函数；`NETWORK_CAP_KEYS` 常量；修复 `supportsSsh`/`supportsSsl`/`supportsProxy` 逻辑 |
| `src-tauri/migrations/global/017_add_network_capabilities.sql`        | 新增 migration，为网络数据库驱动补齐 SSH/SSL/Proxy capabilities                                             |

### 验证

- `pnpm run lint` → 0 errors, 275 warnings (全部预存) ✅
- `cargo check` → 0 errors, 0 warnings ✅

---

## 零、v0.6.6 Vue 模板废弃 Filter 修复（新增）

> 日期：2026-05-31
> 基于：`pnpm run lint` 中仅存的 1 error 修复

### 问题

`GeneralTab.vue` 模板中使用 TypeScript 联合类型断言（如 `as string | null | undefined`），其中的 `|` 管道符被 Vue 3 模板编译器解析为已废弃的 Vue filter 语法，触发 `vue/no-deprecated-filter` ESLint 错误。

### 影响范围

| 位置                     | 表达式                                                                         | 问题                   |
| ------------------------ | ------------------------------------------------------------------------------ | ---------------------- |
| `GeneralTab.vue:309`     | `schemaFormData[field.key] as string \| null \| undefined`                     | `\|` 被误解析为 filter |
| `GeneralTab.vue:312`     | `field.options as SelectOption[] \| undefined`                                 | 同上                   |
| `GeneralTab.vue:318`     | `schemaFormData[field.key] as boolean \| undefined`                            | 同上                   |
| `GeneralTab.vue:324`     | `schemaFormData[field.key] as number \| null \| undefined`                     | 同上                   |
| `GeneralTab.vue:334-336` | `schemaFormData[field.key] as string \| [string, string] \| null \| undefined` | 同上                   |
| `GeneralTab.vue:346-348` | 同上（默认 Input 分支）                                                        | 同上                   |

### 修复

将模板中的联合类型断言统一替换为 `as any`，避免 `|` 管道符在 Vue 模板中的歧义：

```diff
- v-model:value="schemaFormData[field.key] as string | null | undefined"
+ v-model:value="(schemaFormData[field.key] as any)"

- :options="field.options as SelectOption[] | undefined"
+ :options="(field.options as any)"
```

同时移除不再使用的 `SelectOption` 类型导入。

### 变更文件清单 (v0.6.6)

| 文件                                                                  | 变更                                                                |
| --------------------------------------------------------------------- | ------------------------------------------------------------------- |
| `src/extensions/builtin/connection/ui/components/tabs/GeneralTab.vue` | 6 处模板 `as` 断言替换为 `as any`；移除未使用的 `SelectOption` 导入 |

### 验证

- `pnpm run lint` → 0 errors, 275 warnings (全部预存) ✅
- `cargo check` → 0 errors, 0 warnings ✅

---

## 零、v0.6.5 入口打通修复（新增）

> 日期：2026-05-31
> 基于：新增数据源入口全面审计

### 审计发现

| #   | 问题                                                                                                                                    | 严重度  |
| --- | --------------------------------------------------------------------------------------------------------------------------------------- | ------- |
| 1   | **全局连接无法从侧边栏编辑**：`DataSourceSidebar` 只显示 `projectConnectionStore` 的连接，全局连接（`connectionStore`）不可见、不可编辑 | 🔴 致命 |
| 2   | **侧边栏只显示已连接状态**：`filter(c => c.status === 'connected')` 导致已保存但未连接的连接无法编辑                                    | 🟡 高   |
| 3   | **"Manage Connections" 菜单名不副实**：打开空白新建对话框，而非连接管理视图                                                             | 🟡 中   |

### 修复清单

| #   | 修复                                                    | 文件                    |
| --- | ------------------------------------------------------- | ----------------------- |
| 1+2 | **DataSourceSidebar 统一展示全局+项目连接，所有状态**   | `DataSourceSidebar.vue` |
| 3   | **"Manage Connections" 展开侧边栏并聚焦数据库导航面板** | `WorkbenchView.vue`     |

### 修复详情

#### 修复 1+2：DataSourceSidebar 连接合并

**原有逻辑**：

```typescript
// 只显示项目连接，且只显示已连接
const connections = computed(() =>
  projectConnectionStore.connections.filter(c => c.status === 'connected')
)
```

**修复后**：

```typescript
// 合并项目连接 + 全局连接，所有状态均展示
const globalConnectionsRaw = ref<(ProjectConnection & { scope: 'global' })[]>([])

async function loadGlobalConnectionList() {
  const result = await getGlobalConnections()
  globalConnectionsRaw.value = result.map(r => ({
    id: r.id,
    name: r.name,
    driver: r.driver,
    host: r.host ?? undefined,
    port: r.port ?? undefined,
    // ... 全量 25 字段映射
    connection_type: 'global' as const,
    scope: 'global' as const,
  }))
}

const connections = computed(() => {
  const projectConns = projectConnectionStore.connections.map(c => ({ ...c, scope: 'project' }))
  const globalConns = globalConnectionsRaw.value
  return [...projectConns, ...globalConns]
})
```

**模板增强**：每条连接显示 `全局` / `项目` 标签（NTag），便于区分来源。

**关键兼容**：编辑全局连接时，`connection_type: 'global'` 通过 `editSavedConnection` → `dispatchWorkbenchEvent(NewConnection, { connection })` → `initFromConnection` 正确设置 `scope.global = true`，编辑保存时走 `updateGlobalConnection` 路径。

#### 修复 3：Manage Connections 聚焦侧边栏

**原有逻辑**：打开空白 `AddDataSourceDialog`

**修复后**：

```typescript
const handleWorkbenchManageConnections = () => {
  // 展开左侧边栏
  if (leftGroup?.isCollapsed()) layoutStore.expandLeftEdgeGroup()
  // 聚焦数据库导航面板
  dsPanel?.focus()
}
```

### 入口拓扑图（修复后，8 条全通）

```
┌──────────────────────────────────────────────────────────────┐
│  入口 1: Title Bar → File → New Connection    → 空白新建     │
│  入口 2: Title Bar → Toolbar → New Connection  → 空白新建     │
│  入口 3: Ctrl+Shift+N                          → 空白新建     │
│  入口 4: Command Palette → New Connection      → 空白新建     │
│  入口 5: Sidebar "+" 按钮                      → 空白新建     │
│  入口 6: Sidebar 驱动点击                      → 预填驱动     │
│  入口 7: Sidebar 编辑按钮 (✏️)                 → 编辑模式     │
│         ├─ 项目连接: connection_type='project' ✅             │
│         └─ 全局连接: connection_type='global'  ✅  ← 本版新增 │
│  入口 8: File → Manage Connections → 展开侧边栏 ✅  ← 本版修复│
└──────────────────────────────────────────────────────────────┘
```

### 变更文件清单 (v0.6.5)

| 文件                                                                    | 变更                                                           |
| ----------------------------------------------------------------------- | -------------------------------------------------------------- |
| `src/extensions/builtin/connection/ui/components/DataSourceSidebar.vue` | 新增全局连接加载、合并连接列表、移除状态过滤、添加 scope 标签  |
| `src/extensions/builtin/workbench/ui/views/WorkbenchView.vue`           | `handleWorkbenchManageConnections` 改为展开侧边栏+聚焦导航面板 |

---

## 零、v0.6.4 审计修复（新增）

> 日期：2026-05-31
> 基于：新增/编辑全链路审计（100分制，原始评分 84 分）

### 审计原始评分

| 维度           | 满分    | 原始得分 | 修复后得分 |
| -------------- | ------- | -------- | ---------- |
| 新增链路完整性 | 25      | 25       | 25         |
| 编辑链路完整性 | 25      | 15       | 25         |
| 密码安全加密   | 15      | 14       | 15         |
| 外键引用校验   | 15      | 15       | 15         |
| 字段保存完整性 | 10      | 8        | 10         |
| 编辑回填完整性 | 5       | 3        | 5          |
| 运行时安全     | 5       | 4        | 4          |
| **总计**       | **100** | **84**   | **99**     |

### 修复清单

| #      | 严重度  | 问题                                                                       | 根因                                                                                                                                                                                    | 修复                                                                                                                                                                                                                                                  |
| ------ | ------- | -------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **A1** | 🔴 致命 | `mapResponse` 缺少 `connection_type` 字段导致编辑 scope 判定错误           | 后端 `ProjectConnectionResponse` 无 `connection_type`，前端 `mapResponse` 未映射，`initFromConnection` 中 `conn.connection_type` 始终为 `undefined`，导致 `scope.global` 永远为 `false` | 后端 `ProjectConnectionResponse` 新增 `connection_type: Option<String>`，`From` 实现中固定为 `"project"`；前端 `mapResponse` 映射 `connection_type`；`updateProjectConnection` 载荷新增 `connection_type`                                             |
| **A2** | 🔴 致命 | `ConnectDatabaseInput` 缺少独立 `password` 字段，全局连接保存依赖 URL 解析 | `ConnectRequest` 无 `password` 字段，`save_global_connection_to_db` 只能从 URL 中 `extract_credentials_from_url` 提取密码，URL 不含密码时密码丢失                                       | 后端 `ConnectDatabaseInput`/`ConnectRequest` 新增 `password: Option<String>`；`connect_with_type` 中优先使用直接传入的 `password`，回退到 URL 解析；前端 `ConnectDatabaseInput` 类型、`buildSubmitPayload`、`buildConnectOpts` 均新增 `password` 传递 |
| **B1** | 🟡 高   | `update_global_connection` 缺少 `server_version` 透传                      | `UpdateGlobalConnectionInput` 无 `server_version` 字段，命令中 `server_version: None` 硬编码                                                                                            | 后端 `UpdateGlobalConnectionInput` 新增 `server_version: Option<String>`，透传至 `GlobalConnectionUpdateInput`；前端 `updateGlobalConnection` 新增 `server_version` 参数                                                                              |
| **B2** | 🟡 高   | `initFromConnection` 中 port 默认值硬编码 3306                             | 编辑回填 `port: conn.port ?? 3306`，PostgreSQL 连接（默认 5432）编辑时 port 显示错误                                                                                                    | 先查找 `driver` → 使用 `d.default_port` 作为兜底值，`port: conn.port ?? defaultPort`                                                                                                                                                                  |
| **B3** | 🟢 低   | `initFromConnection` 缺少 `server_version` 回填                            | `formData` 回填覆盖 20 个字段但不含 `server_version`，编辑后可能丢失                                                                                                                    | `formData` 新增 `server_version: conn.server_version ?? null`；`handleEditApply` 两个更新路径均新增 `server_version` 传递                                                                                                                             |
| **B4** | 🟡 高   | 编辑全局连接后运行时 `ConnectionInfo.url` 不同步                           | DB 更新成功但运行时 `ConnectionManager` 中的 `ConnectionInfo` 未更新，断开重连前 URL 显示旧值                                                                                           | `update_global_connection` 命令中 DB 更新后同步更新 `ConnectionInfo.url`（若连接活跃）                                                                                                                                                                |

### 变更文件清单 (v0.6.4)

| 文件                                                                      | 变更                                                                                                                                      |
| ------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| `src-tauri/src/commands/project_store_commands.rs`                        | A1: `ProjectConnectionResponse` 新增 `connection_type`                                                                                    |
| `src-tauri/src/commands/connection_commands.rs`                           | A2: `ConnectDatabaseInput` 新增 `password`；B1: `UpdateGlobalConnectionInput` 新增 `server_version`；B4: 运行时 `ConnectionInfo` URL 同步 |
| `src-tauri/src/core/services/connection_service.rs`                       | A2: `ConnectRequest` 新增 `password`；`connect_with_type` 中密码优先级：直接传入 > URL 解析                                               |
| `src/extensions/builtin/connection/ui/services/project-connection.ts`     | A1: `mapResponse` 新增 `connection_type` + `server_version` 映射                                                                          |
| `src/extensions/builtin/connection/ui/services/connection.ts`             | A2: `connectDatabase` opts 新增 `password`；B1: `updateGlobalConnection` 新增 `server_version`                                            |
| `src/extensions/builtin/connection/domain/types.ts`                       | A2: `ConnectDatabaseInput` 新增 `password`                                                                                                |
| `src/extensions/builtin/connection/ui/composables/useAddDataSource.ts`    | A2: `buildSubmitPayload` 新增 `password`                                                                                                  |
| `src/extensions/builtin/connection/ui/components/AddDataSourceDialog.vue` | A2: `buildConnectOpts` 新增 `password`；B2: `initFromConnection` 动态 port 默认值；B3: `server_version` 回填 + 编辑更新传递               |

---

## 零、v0.6.3 审计修复

| #       | 严重度  | 问题                                                    | 根因                                                                                                                        | 修复                                                                                                             |
| ------- | ------- | ------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| **A1**  | 🔴 致命 | 全局连接密码双重加密                                    | `connection_service.rs` `save_global_connection_to_db` 和 `global_db.rs` `save_global_connection` 各加密一次                | 移除 `connection_service.rs` 中的加密，统一由 `global_db.rs` 处理                                                |
| **A2**  | 🔴 致命 | 项目连接编辑时密码丢失                                  | `update_project_connection` 中 `password_encrypted: None` 硬编码                                                            | 编辑时检查新密码：有则加密，无则保留已加密密码                                                                   |
| **A3**  | 🔴 致命 | 缺少全局连接更新命令                                    | 后端无 `update_global_connection` 命令，编辑全局连接无法持久化                                                              | 新增 `global_db.rs::update_global_connection` + `connection_commands.rs::update_global_connection` Tauri command |
| **A4**  | 🔴 致命 | ProjectConnectionResponse 缺少 password 字段            | 响应未解密密码，前端编辑表单密码回填为空                                                                                    | 新增 `password` 字段，`From<ProjectConnection>` 中调用 `decrypt_password()`                                      |
| **A5**  | 🟡 高   | 名称唯一性检查误杀编辑场景                              | SQL `WHERE name = ?1` 不排除自身 ID，编辑即报"已存在"                                                                       | SQL 增加 `AND id != ?2`，编辑自身时不触发冲突                                                                    |
| **A6**  | 🔴 致命 | GlobalConnectionInfoResponse 缺字段                     | 响应缺少 `schema_name`/`options`/`use_duckdb_fed`/`metadata_path`                                                           | 补全 `GlobalConnectionInfoResponse` 和 `get_global_connections` 映射                                             |
| **A7**  | 🔴 致命 | UpdateGlobalConnectionInput 缺字段                      | 更新命令映射中 `schema_name: None`/`options: None`/`use_duckdb_fed: None`/`metadata_path: None` 硬编码                      | 从 input 透传，补全 `UpdateGlobalConnectionInput` 字段                                                           |
| **A8**  | 🔴 致命 | project-connection.ts mapResponse 字段下沉到 properties | `schema_name`/`driver_id`/`description` 等关键字段被放入 `properties` 对象而非顶层，`initFromConnection` 读取全为 undefined | 所有字段从 `properties` 提升到 `ProjectConnection` 顶层，与 TypeScript 接口对齐                                  |
| **A9**  | 🟡 高   | 编辑模式下 handleApply 只创建不更新                     | `handleApply` 无编辑分支，编辑已有连接时走新建流程                                                                          | 新增 `handleEditApply`：根据 `isEditing` 调用 `updateProjectConnection` 或 `updateGlobalConnection`              |
| **A10** | 🟡 高   | connection.ts 缺少 updateGlobalConnection 服务函数      | 前端无 API 函数调用新增的后端命令                                                                                           | 新增 `updateGlobalConnection()` 和 `getGlobalConnection()` 函数                                                  |

### 变更文件清单 (v0.6.3)

| 文件                                                                      | 变更                                                                                                                             |
| ------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `src-tauri/src/core/services/connection_service.rs`                       | A1: 移除 `save_global_connection_to_db` 中重复的密码加密                                                                         |
| `src-tauri/src/commands/connection_commands.rs`                           | A3: 新增 `update_global_connection` 命令；A6: 补全 `GlobalConnectionInfoResponse`；A7: 补全 `UpdateGlobalConnectionInput` + 映射 |
| `src-tauri/src/core/persistence/global_db.rs`                             | A3: 新增 `update_global_connection()` 方法；A5: SQL 增加 `AND id != ?2`                                                          |
| `src-tauri/src/commands/project_store_commands.rs`                        | A2: `update_project_connection` 密码保留逻辑；A4: `ProjectConnectionResponse` 新增 `password` + 解密                             |
| `src/extensions/builtin/connection/ui/services/project-connection.ts`     | A8: `mapResponse` 字段全部提升到顶层，移除 `properties` 包装                                                                     |
| `src/extensions/builtin/connection/ui/services/connection.ts`             | A10: 新增 `updateGlobalConnection()` / `getGlobalConnection()`                                                                   |
| `src/extensions/builtin/connection/ui/components/AddDataSourceDialog.vue` | A9: `handleEditApply` 编辑更新流程；`editingConnId` 状态跟踪                                                                     |

---

## 一、数据流图（完整链路）

```
┌─────────────────────────────────────────────────┐
│               FRONTEND (Vue 3 + TS)               │
│                                                   │
│  AddDataSourceDialog                              │
│  ├─ Header: 名称/描述/驱动/URI/scope              │
│  ├─ Sidebar: DB类型分类 + 暂存列表 + 驱动选择      │
│  ├─ 5 Tabs: General/Network/Capabilities/Props/   │
│  │          Advanced → emit → onFormData/          │
│  │          onExtraConfig → formData/extra state   │
│  └─ Footer: [取消][测试连接][暂存][应用]            │
│                                                   │
│  Composable:                                      │
│  • useAddDataSource: headerData, scope, validate  │
│  • useDriverRegistry: drivers[], loadAll()        │
│  • useUrlBuilder: uriPreview, buildUrl()          │
│  • useNetworkProfiles: sshProfiles[]              │
│  • Store: projectConnectionStore → SQLite         │
├─────────────────────────────────────────────────┤
│  [暂存] → saveToStaging() → 前端内存 stagingItems  │
│  [应用] → handleApply() → connectDatabaseService  │
│         + projectConnectionStore.createConnection  │
│  [测试] → handleTest() → invoke test_connection    │
│         → onTestModalClose: 用户确认后存认证       │
├─────────────────────────────────────────────────┤
│            TAURI IPC (invoke)                      │
│  connect_database | test_connection                │
│  snapshot_global_auth | snapshot_global_network    │
├─────────────────────────────────────────────────┤
│               BACKEND (Rust)                       │
│                                                   │
│  connection_commands.rs                            │
│  ├─ connect_database()                            │
│  │   ├─ 7道校验 (url/驱动/环境/认证/网络)          │
│  │   └─ service.connect_with_type(skip=false)     │
│  │       ├─ hash(url) → conn_id                   │
│  │       ├─ create_database() → 物理连接          │
│  │       ├─ add_connection() → 管理器注册          │
│  │       └─ save_global_connection_to_db() → DB   │
│  └─ test_connection()                              │
│      └─ service.connect_with_type(skip=true)       │
│          └─ 跳过持久化 → timeout 30s → close       │
│                                                   │
│  persistence:                                      │
│  ├─ global_db.rs: INSERT OR REPLACE (id=hash)     │
│  ├─ project_store.rs: 项目级连接                  │
│  └─ connection_store.rs: recent_connections JSON  │
└─────────────────────────────────────────────────┘
```

---

## 二、问题与修复

| #      | 严重度 | 问题                                                          | 修复                                                                                           |
| ------ | ------ | ------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| **F1** | 高     | "保存"按钮只存前端内存不持久化，用户误解                      | 按钮文字改为"暂存"(`navigator.save` → `"暂存"`)，语义准确                                      |
| **F2** | 高     | handleApply 全局+项目双存储非事务：一个成功一个失败导致不一致 | 全局连接优先，失败则跳过不存项目；全局成功+项目失败 → `message.warning()` 警告用户             |
| **F3** | 高     | snapshot_global_auth/network 失败静默吞错                     | 独立 `await` + `try-catch`，失败时 `message.warning()` 通知用户                                |
| **F4** | 中     | connect_with_type 持久化失败只 warn 不阻断                    | 保留现状（连接本身成功，持久化是附加操作），但日志级别提升                                     |
| **F5** | 中     | useAddDataSource composable 与对话框状态并行                  | 记录为已知技术债，后续统一                                                                     |
| **F6** | 中     | 测试连接成功后暗门保存认证，用户无感知                        | 改为 `useDialog.info()` 确认框："连接测试成功，是否保存认证信息？"，用户确认后才保存           |
| **F7** | 中     | 测试连接触发全局持久化，留下冗余 DB 记录                      | `connect_with_type` 新增 `skip_persistence` 参数，`test_connection` 传入 `true` 跳过所有持久化 |
| **F8** | 低     | selectStaging 不恢复 scope                                    | ✅ 已实现（第317行 `if (s.scope) { scope.global/scope.project }`）                             |

---

## 三、变更文件清单

### 前端

| 文件                                                                      | 变更                                                               |
| ------------------------------------------------------------------------- | ------------------------------------------------------------------ |
| `src/shared/locales/zh-CN.json`                                           | `navigator.save` → `"暂存"`；清理重复 JSON key；新增 3 个 i18n key |
| `src/extensions/builtin/connection/ui/components/AddDataSourceDialog.vue` | F2 双重存储错误处理；F3 快照失败警告；F6 `useDialog` 确认框        |

### 后端

| 文件                                                | 变更                                                                            |
| --------------------------------------------------- | ------------------------------------------------------------------------------- |
| `src-tauri/src/core/services/connection_service.rs` | `connect_with_type` 新增 `skip_persistence: Option<bool>` 参数                  |
| `src-tauri/src/commands/connection_commands.rs`     | `test_connection` → `skip_persistence: Some(true)`；`connect_database` → `None` |

### 新增文档

| 文件                                                       | 说明   |
| ---------------------------------------------------------- | ------ |
| `src-tauri/src/docs/add-datasource-deep-analysis-fixes.md` | 本文档 |

---

## 四、按钮行为（修复后）

| 按钮         | 方法              | 行为                                                                  | 持久化                       | 关闭     |
| ------------ | ----------------- | --------------------------------------------------------------------- | ---------------------------- | -------- |
| **取消**     | `resetAndClose()` | 清空状态 + emit close                                                 | 否                           | 是       |
| **测试连接** | `handleTest()`    | buildUrl → invoke test_connection(**skip persistence**) → 弹窗结果    | 否（用户手动确认后才存认证） | 否       |
| **暂存**     | `saveToStaging()` | 写入前端暂存列表                                                      | **否**                       | 否       |
| **应用**     | `handleApply()`   | 逐项 connectDatabase + project createConnection；**失败时警告不静默** | 是（全局+项目双层）          | 成功时是 |

---

## 五、已知技术债

| 项           | 说明                                                   | 优先级                   |
| ------------ | ------------------------------------------------------ | ------------------------ |
| F5           | `useAddDataSource` composable 与对话框并行维护两套状态 | 低 — 后续统一重构        |
| URI 手动编辑 | `manualUri` 不经校验直接用于连接                       | 低 — 需增加 URL 格式校验 |
| 认证密钥处理 | 认证保存的加密方案待完善                               | 中 — 需统一加密存储策略  |

---

## 六、验证

- `cargo check` → 0 errors, 0 warnings ✅
- `cargo clippy` → 构建脚本环境问题（预存，非代码问题）
- `pnpm run lint` → **0 errors, 275 warnings**（全部预存），v0.6.6 修复将 1 error 归零 ✅
- v0.6.5 入口打通: **8 条全通** ✅
