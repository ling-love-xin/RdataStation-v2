# 新增数据源功能全面审计 规约

> 审计范围：驱动架构 · 认证/网络/环境三大解耦 · 多层级协议链拓扑 · 全局/项目双域架构 · DuckDB 联邦查询引擎 · 驱动能力/属性

---

## 模块深度理解

### 一、6 层解耦架构

新增数据源是整个 RdataStation 中最核心的入口功能，承载了 6 层解耦设计：

```
AddDataSourceDialog.vue
  │
  ├─ Layer 1: 驱动注册表 (Driver Registry)
  │   SQLite global.db → DataSourceType + Driver → 前端动态表单
  │   关键词: config_schema · url_template · supported_auth_types · capabilities
  │
  ├─ Layer 2: 认证配置 (Auth Config) — 凭据与连接属性分离
  │   auth_configs 表 · AES-256-GCM 加密 · 7 大认证抽象类别
  │   关键词: auth_data 只存凭据 · "AES:" 前缀 · scope: global/project
  │
  ├─ Layer 3: 网络拓扑 (Network Topology) — 多层级可拖拽协议链
  │   network_configs 表 · ChainHopItem[] · SSH→代理→SSL 叠加
  │   关键词: 3 跳上限 · SSL 末尾强制 · select/new/custom 三种模式
  │
  ├─ Layer 4: 环境策略 (Environment) — 5 维策略模板
  │   environments 表 · environment_policies 表 · G_ → GP_ 快照
  │   关键词: Security · Schema · Performance · Audit · UI
  │
  ├─ Layer 5: 全局/项目双域 (Global / Project) — 互补共存
  │   global_connections (25 列) ≡ connections (25 列) · 同字段对齐
  │   关键词: 非二选一 · 同时写入 · 认证/网络/环境快照联动
  │
  └─ Layer 6: DuckDB 联邦查询引擎 (DuckDB Federation)
      use_duckdb_fed 标志 · DuckDB ATTACH 远程数据库
      关键词: 直接查询 · 无需数据拷贝 · mysql_scanner / postgres_scanner
```

### 二、Layer 1: 驱动架构

**设计理念**: 新增数据库类型**不需要发版**，只需往 SQLite `global.db` 的 `drivers` 表 INSERT 一行。

```
SQLite global.db
  ├── data_source_types 表     ← 数据库大类 (relational / file-based / nosql / analytics / cloud / mq / http)
  │     id, name, category, icon, enabled
  │
  └── drivers 表               ← 具体驱动实例
        id, type_id, name, driver_kind, is_file,
        default_port, url_template,              ← jdbc:mysql://{host}:{port}/{database}
        config_schema,                           ← JSON Schema → 驱动前端动态生成表单字段
        supported_auth_types,                    ← JSON ["password","kerberos"] → 过滤认证下拉
        capabilities,                            ← JSON {"supports_transaction":true,…} → CapabilitiesTab
        driver_properties,                       ← JSON KV → DriverPropsTab 默认值
        download_url, version, enabled
```

**前端加载链路**:

```
useDriverRegistry.loadAll(projectPath?)
  └─ Promise.all([
       invoke('get_data_source_types') → DataSourceType[],
       invoke('get_available_drivers') → DriverListResponse { drivers, missing[] }
     ])
       ├─ AddDataSourceSidebar: getGroupedTypes() → 按 category 分组渲染
       └─ DataSourceHeader: driverOptions → 过滤当前 type_id 的驱动 → NSelect 下拉
              │
              onDriverSelect(driverId)
                → headerData.selectedDriverId = driverId
                → onDriverChange(driver)
                   → parseConfigSchema(driver.config_schema)  ← GeneralTab 应动态渲染
                   → parseSupportedAuthTypes(driver.supported_auth_types)  ← 过滤认证下拉
```

**关键设计点**:

| 字段                   | 含义                                                                             | 消费方                            |
| ---------------------- | -------------------------------------------------------------------------------- | --------------------------------- | ---- | ---- | ---- | ---- | ------ | --- | ---------------- |
| `config_schema`        | JSON Schema 字符串，定义连接字段的 type/label/required/default/placeholder/order | GeneralTab                        |
| `url_template`         | 占位符模板 `{host}:{port}/{database}`                                            | DataSourceHeader 实时 URI 预览    |
| `supported_auth_types` | 驱动支持的认证方式列表                                                           | useAuthConfig.authMethodOpts 过滤 |
| `driver_kind`          | `native                                                                          | jdbc                              | odbc | wasm | adbc | http | python | js` | 后端选择连接策略 |
| `capabilities`         | 驱动能力元数据                                                                   | CapabilitiesTab 展示              |

### 三、Layer 2: 认证配置 — 凭据与连接属性分离

**设计理念**: `auth_data` 列只存认证凭据（username / password / certPath / principal / clientSecret），不混入 host/port/database 等连接属性。密码/敏感字段用 AES-256-GCM 加密，以 `AES:` 前缀标记。

```
auth_configs 表:
  id         TEXT  PRIMARY KEY
  name       TEXT
  auth_type  TEXT   ← password | ldap | pg_class | kerberos | oauth2 | os_auth | trust
                     | ssh_password | proxy_password
  auth_data  TEXT   ← JSON { username, password, certPath, principal, clientSecret, … }
                     加密前: JSON 字符串
                     加密后: "AES:" + base64(iv + ciphertext)
  origin     TEXT   ← global | project
  created_at / updated_at

7 大认证抽象类别:
  数据库认证: password · ldap · pg_class · kerberos · oauth2 · os_auth · trust
  网络认证:   ssh_password · proxy_password
```

**前端认证流程** (useAuthConfig):

```
loadAuthConfigs(projectPath?)
  ├─ invoke('list_auth_configs')           → 全局认证配置 (origin='global')
  └─ invoke('project_list_auth_configs')    → 项目认证配置 (origin='project')
      → 合并: [...globalConfigs, ...projectConfigs]
      → authConfigs.value 供 filteredAuthConfigOpts 使用

authMethod
  ├─ 初始值 'password'
  └─ 驱动切换时 updateSupportedAuthTypes(types) → types 为空则显示全部 7 种

onAuthConfigSelect(configId)
  ├─ configId 为空 → 清空表单 → "手动填写"模式
  └─ configId 有效 → 选中已保存配置 → 自动预填 username/password/certPath/…
      → onAuthConfigChange(configId, authType) 通知父组件
```

### 四、Layer 3: 网络拓扑 — 多层级可拖拽协议链

**设计理念**: 不是单一的 SSH/SSL 开关，而是可组合、可排序的多跳协议链。每跳支持 3 种配置模式（选择已有 / 新建写入 DB / 临时填写）。**同协议可重复**（SSH→代理→SSH 是合法的内网穿透拓扑），**SSL/TLS 必须在末尾**（TLS 是连接级加密而非隧道）。

```
协议链模型 ProtocolNode[]:
  { id, protocol: 'ssh'|'proxy'|'ssl', enabled: boolean, mode: 'select'|'new'|'custom', profileId }

约束规则:
  ✅ 同协议可重复       → SSH → 代理 → SSH 合法
  ✅ SSL 始终在末尾     → ensureSslAtEnd() + 拖拽拦截
  ✅ 非 SSL 跳上限 = 3  → MAX_NETWORK_HOPS
  ✅ 每种协议至少保留 1 跳 → deleteHop 保护
  ✅ SSL 始终可添加     → addHop 不受上限限制（appends to end）

每跳 3 种配置模式:
  select ── 从 useNetworkProfiles 选择已有配置
  new    ── 新建配置 → 调用后端 create_network_config → 写入 DB + 同步到 useNetworkProfiles
  custom ── 临时填写（不持久化到 DB）

network_configs 表:
  id · name · network_type (ssh|ssl|proxy|chain)
  config: TEXT  ← 完整冗余 (host/port/username/auth/config…)
  auth_config_id: TEXT  ← 可选引用 auth_configs.id
  origin: global|project
```

**拓扑拖拽规则** (useNetworkChain):

```
onDragStart(id) ── 记录拖拽源
onDrop(targetId) ── 从 chain 中提取 src 节点，插入 target 之前
  ├─ 如果 src 是 SSL 且 target 不是末尾 → 拦截（返回 false，调用方弹提示）
  └─ ensureSslAtEnd() ── 强制 SSL 回末尾
```

### 五、Layer 4: 环境策略 — 5 维策略模板

**设计理念**: 环境是预定义的策略模板，选择后自动填充所有策略字段。用户可选择性覆盖（`isPolicyOverridden` 标记）。全局环境被项目引用时触发快照 `G_xxx → GP_xxx`，快照后可自由修改不污染全局模板。

```
environments 表:
  id    (G_env_dev / G_env_prod / GP_env_prod / …)
  name · description · color · sort_order
  origin · source_id · snapshot_at  ← 快照溯源

environment_policies 表:
  environment_id · policy_type · policy_config · enabled

5 维策略:
  Security  ── readonly · writeConfirm · ddlConfirm · dropPolicy · autocommit · rowLimit · sizeLimit
  Schema    ── autoLoad · loadDepth · showSystem · refreshInterval
  Perf      ── poolSize · queryTimeout · connectTimeout · heartbeat · maxReconnect
  Audit     ── sqlLog · retentionDays
  UI        ── topBarColor · numberFormat · nullDisplay · maxColumnWidth
```

### 六、Layer 5: 全局/项目双域

**设计理念**: 全局和项目**不是二选一切换**，而是**互补共存**。用户可以同时勾选 `scope.global=true` + `scope.project=true`，Apply 时连接同时写入两边的数据库。

```
global_connections 表 (25 列):
  id · name · driver · url · host · port · database · username · password_encrypted
  auth_config_id · auth_method · network_config_id · environment_id
  driver_properties · advanced_options · options · tags · metadata_path · schema_name
  use_duckdb_fed · connection_type · created_at · updated_at · status · last_connected_at · origin

connections 表 (25 列):
  ↑ 字段完全对齐 (25/25)，差异仅在全局表多 origin/source_id 列

Apply 流程:
  StagingItem[] → 遍历每个 item:
    1. 认证快照:  if scope.project & authConfigId → create_auth_config() → 返回新 ID
    2. 网络快照:  if scope.project & networkConfigId → create_network_config() → 返回新 ID
    3. 环境快照:  if scope.project & environmentId → snapshot_global_env() → 返回 GP_xxx
    4. 项目连接:  if scope.project → create_project_connection(input)  → 写入 connections
    5. 全局连接:  if scope.global → create_global_connection(input)     → 写入 global_connections
    6. 自动连接:  if scope.global → connect_to_datasource(connId)
```

### 七、Layer 6: DuckDB 联邦查询引擎

**核心理念**: DuckDB 通过其多数据库扩展（`mysql_scanner`、`postgres_scanner`、`sqlite_scanner` 等）直接 ATTACH 外部数据库，在 DuckDB 内部执行分析查询。**数据不经过拷贝步骤**——DuckDB 直接查询远程数据库。

> ⚠️ 区别于"数据同步/物化"模块：另一个独立模块会实现将远程数据拉取到本地 DuckDB 表做物化缓存，该功能不属于新增数据源模块。

```
高级配置 → DuckDBAccelSection:
  useDuckdbFed:    boolean    ← 标记此连接可被 DuckDB 引擎直接查询
  sync_strategy:   manual | interval | trigger  ← 动态刷新元数据的策略
  sync_interval:   number     ← 定时刷新表结构/索引的间隔(秒)
  memory_limit:    2048 MB    ← DuckDB 实例为此连接预留的内存
  threads:         4          ← DuckDB 执行查询时的并行线程数

存储:
  StagingItem.options = { duckdb_accel: { enabled, syncStrategy, syncInterval, memory, threads } }
  ConnectDatabaseInput.use_duckdb_fed = true/false
  connections.use_duckdb_fed = 1/0

适用条件:
  DuckDB 通过扩展支持 ATTACH 的数据库类型:
    ✅ MySQL (mysql_scanner)    ✅ PostgreSQL (postgres_scanner)
    ✅ SQLite (sqlite_scanner)   ❌ Oracle · SQL Server (无 DuckDB 扩展)

查询工作流 (实际查询时):
  DuckDB 引擎自动执行:
    ATTACH 'mysql://user:pass@host:3306/db' AS remote_db (TYPE mysql);
    SELECT t1.*, t2.* FROM remote_db.orders AS t1 JOIN duckdb_local.analytics AS t2 USING (key);
    DETACH remote_db;
  → 零数据拷贝，DuckDB 直接对远程 DB 发起查询并执行分析
```

**配置项含义**:

| 配置            | 不涉及数据拷贝 | 实际作用                                                        |
| --------------- | :------------: | --------------------------------------------------------------- |
| `enabled`       |       ✅       | 是否标记为联邦查询候选源，查询编辑器可选中此源做 ATTACH         |
| `sync_strategy` |       ✅       | 刷新 remote schema 元数据的策略（表列表、列定义等），非数据同步 |
| `sync_interval` |       ✅       | 定时拉取元数据（schema 结构）的间隔                             |
| `memory_limit`  |       ✅       | DuckDB 进程内存预算                                             |
| `threads`       |       ✅       | DuckDB 并行线程数                                               |

---

## Why

新增数据源是 RdataStation 的核心入口功能，涉及 6 层架构（驱动→认证→网络→环境→DuckDB→存储）协同工作。当前实现有 **3 个 P0 设计缺陷**和 **5 个 P1 架构不一致**需要修复。

## What Changes

### 🔴 P0 — 设计缺陷

- **BREAKING**: `useNetworkChain` 与 `useNetworkProfiles` 双重状态存储合并为单一 `useNetworkProfiles` 共享状态
- **BREAKING**: `StagingItem.formData` 移除明文 `password` 键，统一走 `authConfigId` 引用
- 消除 `isResetting` 全局标志，改用 `nextTick` 守卫

### 🟡 P1 — 架构不一致

- `GeneralTab` 表单字段从 `Driver.config_schema` JSON Schema 动态生成，替换旧 `DriverField[]` 硬编码
- 环境选择器触发时自动加载策略，无需手动调用 `fetchPolicies`
- `useNetworkChain.loadProfilesFromDb()` 与 `useNetworkProfiles.loadByType()` 统一查询接口
- 协议链创建新配置后实时同步到 `useNetworkProfiles` 共享列表
- `AdvancedTab` 拆分为 4 个子组件（环境/DuckDB/策略/元数据），1162 行 → ≤400 行

### 🟢 P2 — 体验完善

- `CapabilitiesTab` 的能力数据传递到后端（非仅展示）
- `uriEditing` 模式添加 url_template 占位符校验提示
- 测试连接反馈弹窗中 `os_auth`/`trust` 不弹保存对话框

## Impact

- Affected specs: `multi-mode-editor` (联邦查询依赖 Connection)
- Affected code:
  - `src/extensions/builtin/connection/ui/composables/useNetworkChain.ts` — **合并配置文件状态**
  - `src/extensions/builtin/connection/ui/composables/useNetworkProfiles.ts` — 暴露共享 profiles
  - `src/extensions/builtin/connection/ui/composables/useAddDataSource.ts` — 密码移除 + isResetting 消除
  - `src/extensions/builtin/connection/ui/components/AddDataSourceDialog.vue` — 拆分逻辑
  - `src/extensions/builtin/connection/ui/components/tabs/GeneralTab.vue` — config_schema 动态表单
  - `src/extensions/builtin/connection/ui/components/tabs/AdvancedTab.vue` — 拆分
  - `src/extensions/builtin/connection/ui/stores/environmentStore.ts` — 选环境时自动加载策略

---

## ADDED Requirements

### Requirement: useNetworkChain / useNetworkProfiles 共享配置文件状态

`useNetworkChain` SHALL NOT 维护独立的 `sshProfiles`/`sslProfiles`/`proxyProfiles` ref。所有网络配置文件的 CRUD 操作 MUST 通过 `useNetworkProfiles` 完成，`useNetworkChain` MAY 读取但不 SHALL NOT 写入。

#### Scenario: 协议链中新建 SSH 配置

- **WHEN** 用户在 `NetworkTab` 的协议链中新建 SSH 配置
- **THEN** `useNetworkChain.saveNewHop()` 调用 `useNetworkProfiles.saveProjectProfile()` 写入后端
- **AND** 写入成功后 `useNetworkProfiles.sshProfiles` 更新，所有引用者自动获得新数据
- **AND** `NetworkTab` 的下拉选项实时包含新创建的配置

### Requirement: GeneralTab 表单字段由 Driver.config_schema 动态驱动

`GeneralTab.vue` SHALL 解析 `Driver.config_schema`（JSON Schema 字符串）动态渲染表单字段。不再使用旧的 `DriverField[]` 类型和硬编码字段列表。

#### Scenario: MySQL 驱动的 config_schema 包含 host/port/database/username/password

- **GIVEN** MySQL Driver 的 `config_schema` 为 JSON Schema 字符串，定义了 5 个字段的 required/type/default/placeholder
- **WHEN** `GeneralTab.vue` 挂载
- **THEN** 表单动态渲染 5 个输入框，label 来自 schema，placeholder 来自 schema
- **AND** 驱动切换时表单自动重新渲染

### Requirement: 环境选择器自动加载策略

`EnvironmentSelector` 的 `onEnvChange` 事件处理 SHALL 自动调用 `environmentStore.fetchPolicies(envId)`。调用方不再需要手动获取策略。

#### Scenario: 用户选择 "生产环境"

- **WHEN** 用户在下拉框选择 `G_env_prod`
- **THEN** `environmentStore.selectEnv('G_env_prod')` 被调用
- **AND** `environmentStore.fetchPolicies('G_env_prod')` 自动被调用
- **AND** 策略标签（只读/DDL确认/DROP策略等）自动更新

### Requirement: CapabilitiesTab 数据写入 advancedOptions

`CapabilitiesTab` SHALL 将其展示的驱动能力（supports_transaction, supports_arrow 等）通过 `extra-config` 事件传递给父组件，由父组件写入 `advancedOptions` 传递到后端。

#### Scenario: 驱动声明 supports_arrow=true

- **WHEN** 驱动 capabilities 包含 `"supports_arrow": true`
- **THEN** 后端 `create_database` 收到的 `advanced_options` 包含 `"driver_supports_arrow": true`
- **AND** 查询引擎可利用此元数据选择 Arrow 零拷贝路径

## MODIFIED Requirements

### Requirement: useNetworkChain 协议链数据

**变更**: `sshProfiles`/`sslProfiles`/`proxyProfiles` ref 从 `useNetworkChain` 移除，改为从 `useNetworkProfiles` 共享状态读取。

原有设计：

```
useNetworkChain {
  sshProfiles ← saveNewHop() 写入但不同步
  sslProfiles  ← 独立于 useNetworkProfiles
  proxyProfiles ← loadProfilesFromDb() 重复查询
}
useNetworkProfiles {
  sshProfiles ← loadAll() 写入
  sslProfiles  ← 独立于 useNetworkChain
  proxyProfiles ← 不同步
}
```

修改后：

```
useNetworkProfiles {
  sshProfiles  ← 唯一数据源
  sslProfiles  ← CRUD 唯一入口
  proxyProfiles ← useNetworkChain 读取但不写入
}
useNetworkChain {
  getProfiles() → useNetworkProfiles.sshProfiles.value
}
```

#### Scenario: 协议链拖拽排序后保存

- **WHEN** 用户拖拽调整协议链顺序
- **THEN** `networkConfig` computed 重新计算
- **AND** 所有被引用的配置文件仍来自 `useNetworkProfiles` 共享池

### Requirement: 环境选择器

**变更**: `onEnvChange` 内部自动触发 `fetchPolicies`，移除调用方的冗余 `await environmentStore.fetchPolicies(envId)` 逻辑。

### Requirement: StagingItem 密码安全

**变更**: `buildStagingItem()` 在写入 localStorage 前从 `formData` 中删除 `password` 键。

### Requirement: isResetting 标志

**变更**: 完全移除。`handleSelectStaging` 中将 `name` watcher 的赋值放到 `nextTick` 回调执行。

### Requirement: DuckDB 联邦查询引擎（描述修正）

**变更**: DuckDB 加速功能描述修正为：标记连接可被 DuckDB 引擎通过 ATTACH 机制直接查询，**不涉及数据拷贝**。区别于后续独立的"数据同步/物化"模块。

#### Scenario: 用户启用 DuckDB 加速

- **WHEN** 用户在 AdvancedTab 中勾选"启用 DuckDB 查询加速"
- **THEN** StagingItem.options 记录 `{ duckdb_accel: { enabled: true, syncStrategy, syncInterval, memory, threads } }`
- **AND** ConnectDatabaseInput.use_duckdb_fed = true
- **AND** 后续查询编辑器中，用户可选中此数据源进行联邦查询（DuckDB ATTACH 远程 DB，直接执行 SQL）

## REMOVED Requirements

### Requirement: GeneralTab 旧 DriverField 表单系统

**Reason**: 旧的 `DriverField[]` 类型和硬编码表单字段与 `config_schema` JSON Schema 驱动设计不一致。
**Migration**: `GeneralTab.vue` 重写为 `config_schema` → JSON Schema 解析器 → 动态表单渲染。旧的 `DriverField`/`DriverOption` 类型保留向后兼容，但 `GeneralTab` 不再使用。

## AUDIT FIX Requirements (2026-06-09)

### Requirement: StagingItem scope SHALL support 'both'

`StagingItem.scope` SHALL support `'global' | 'project' | 'both'` to preserve the user's intent when both checkboxes are selected. Recovery logic MUST set both `scope.global = true` and `scope.project = true` when scope is `'both'`.

#### Scenario: User checks both global and project

- **WHEN** user clicks "Save to Staging" with both `scope.global=true` and `scope.project=true`
- **THEN** the resulting StagingItem has `scope: 'both'`
- **AND** selecting this staging item restores both checkboxes to checked state

### Requirement: saveToProject SHALL use snapshot IDs

`handleCreateApply` SHALL pass `snapshotNetId` and `snapshotAuthId` (results from `snapshotIfNeeded`) to `saveToProject`, NOT the original `item.networkConfigId` / `item.authConfigId`.

### Requirement: validate SHALL reject port/url/name on backend

Backend connection creation commands SHALL validate:

- `name` is non-empty
- `url` is non-empty
- `port` is within 1-65535

### Requirement: DuckDB acceleration SHALL only show for supported drivers

`DuckDBAccelSection` SHALL only render when the driver is MySQL, PostgreSQL, SQLite, or DuckDB (drivers with DuckDB ATTACH extensions).

### Requirement: handleClose SHALL confirm when staging has unapplied changes

When staging items contain unapplied entries with names, `handleClose` SHALL show a confirmation modal before closing the dialog.

### Requirement: saveToStaging SHALL use dedicated refs for advanced fields

`saveToStaging()` and `syncCurrentToStaging()` SHALL read `schemaName`, `options`, `metadataPath`, `tags`, `useDuckdbFed` from the dedicated refs exported by `useAddDataSource`, not from `formData.value.schema_name` etc.

### Requirement: addStaging SHALL fully reset form state

`addStaging()` SHALL reset `formData`, `protocolChain`, `selectedEnvId`, and `driverProps` in addition to the already-reset auth/network/advanced fields.

### Requirement: NetworkTab SHALL reload project configs when scope.project changes

`NetworkTab.vue` SHALL watch `props.scope?.project` and reload project network configs when it becomes true after initial mount.

## AUDIT R3 Requirements (2026-06-09)

### Requirement: buildSavePayload SHALL support 'both' scope

`buildSavePayload().scope` SHALL emit `'both'` when both `scope.global` and `scope.project` are true. `SaveConnectionInput.scope` type SHALL include `'both'`.

### Requirement: formData SHALL use typed interface

`formData` ref SHALL use `ConnectionFormData` interface (with `host/port/database/username/password/filePath/url` fields) instead of bare `Record<string, unknown>`.

### Requirement: Driver/protocol whitelists SHALL use named constants

`isFileDatabase` driver list and `validateUrl` protocol list SHALL be extracted as `KNOWN_FILE_DBS` and `KNOWN_DB_PROTOCOLS` named constants at module level.

### Requirement: buildAuthData SHALL use declarative field mapping

Auth type → field mapping SHALL use `AUTH_TYPE_FIELDS` constant object, not if-else chains. New auth types added by adding entries to the object.

### Requirement: saveToStaging SHALL not overwrite wrong staging item

When current `stagingIndex` points to an existing named item, `saveToStaging` SHALL append a new item rather than overwriting the existing one.

### Requirement: addStaging SHALL not duplicate empty items

When the current staging item is empty (no name, not applied), `addStaging` SHALL reset it in-place rather than pushing a new empty item.

### Requirement: countNetworkHops SHALL exclude disabled hops

`countNetworkHops()` SHALL filter `h.enabled !== false` in addition to `h.protocol !== 'ssl'`.

### Requirement: handleTest SHALL validate before connecting

`handleTest()` SHALL call `validate()` and block execution if validation fails, showing the first error to the user.

### Requirement: Driver change SHALL skip redundant schema parse

`onDriverChange` SHALL short-circuit when the driver ID hasn't changed, avoiding redundant `config_schema` / `capabilities` JSON parsing.

## AUDIT R4 Requirements (2026-06-09)

### Requirement: NetworkTab SHALL use naive-ui notification instead of alert()

`testChainHop` SHALL use `useMessage()` (success/warning/error) instead of raw `alert()` for consistent desktop UI.

### Requirement: saveChainToDb SHALL log errors before returning null

`save_network_config` invocation SHALL use `.catch((e) => { console.error(...); return null })` instead of bare `.catch(() => null)`.

### Requirement: GeneralTab SHALL use NModal instead of prompt()

`createNewDbFile` SHALL render `NModal` + `NInput` with confirm/cancel buttons instead of `window.prompt()`.

### Requirement: GeneralTab SHALL log warnings on file dialog unavailable

Empty catch blocks in `browseFile`/`browseCert`/`browseKeytab` SHALL include `console.warn(...)` instead of bare comments.

### Requirement: initFromEdit SHALL include raw data in parse error logs

`advanced_options` JSON.parse catch block SHALL log the original `data.advanced_options` string alongside the error for debugging.

### Requirement: handleCreateApply SHALL mark staged items immediately

Each successfully applied staging item SHALL be marked via `markStagingApplied()` immediately after save, eliminating the `appliedIndices` array collection pattern.

## AUDIT R5 Requirements (2026-06-09)

### Requirement: isMaxNetworkHops SHALL use enabledNetworkHopCount

`isMaxNetworkHops` and `remainingHops` computed SHALL use `enabledNetworkHopCount` (which filters both `protocol !== 'ssl'` AND `enabled`) instead of `networkHopCount`.

### Requirement: doSaveAuth SHALL warn when project path is missing

When `scope.project` is true but `projectStore.currentProject?.path` is undefined during auth config save, the function SHALL show a warning message instead of silently returning.

### Requirement: headerData SHALL not contain dead editUriMode field

The `editUriMode` field in `headerData` reactive object SHALL be removed. The template uses the dialog-local `uriEditing` ref instead.

## AUDIT R5 Extension: GeneralTab (2026-06-09)

### Requirement: Config schema v-for SHALL not duplicate auth-managed fields

`AUTH_MANAGED_KEYS` SHALL include `'username'` so the config_schema v-for does not render a second username input alongside the auth section's dedicated username field.

### Requirement: Single schema parser for all form sections

Both the main v-for fields and the advanced schema fields SHALL be rendered using the same `parseConfigSchema` function, eliminating the `parseSchemaToFormFields` dual-parser inconsistency.

### Requirement: ConfigSchemaField SHALL carry layout metadata

`ConfigSchemaField` interface SHALL include optional `helpText`, `min`, `max`, and `rows` properties to support advanced field rendering without an alternative parser.

### Requirement: onMounted SHALL not duplicate watch-side-effect

`updateAdvancedSchemaFields()` SHALL only be called from `watch(driver, {}, {immediate: true})`, not from `onMounted`, to avoid double emission.

## AUDIT R12 Requirements (2026-06-11)

基于产品评估报告的 3 个核心用户体验痛点，修复连接名称重复、暂存项切换数据丢失、URL 自动解析问题。

### Requirement: handleCreateApply SHALL reject duplicate connection names

`handleCreateApply` SHALL check for duplicate names within the staging batch before applying. Backend `global_db.rs` and `project_connection_store.rs` SHALL enforce name uniqueness at the database level via `SELECT COUNT(*) FROM ... WHERE name = ?1 AND is_active = 1`.

#### Scenario: User adds two staging items with the same name

- **WHEN** user clicks "Apply" with two staging items both named "MyDB"
- **THEN** `handleCreateApply` detects the duplicate via `nameSet` and shows `message.warning("暂存列表中存在重复名称...")`
- **AND** apply is blocked until the user renames one of the duplicates

#### Scenario: User creates a connection with an existing name

- **WHEN** user creates a connection named "production-db" that already exists in global_connections
- **THEN** backend `create_connection` returns a `CoreError::DuplicateName` error
- **AND** frontend shows the error to the user

### Requirement: handleSelectStaging SHALL confirm before discarding unsaved changes

`handleSelectStaging` SHALL check a `stagingDirty` ref before switching staging items. When `stagingDirty` is true and the current staging item has a name, a `dialog.warning` confirmation SHALL be shown. The form watcher SHALL set `stagingDirty = true` on any form/protocol chain change.

#### Scenario: User switches staging item with unsaved changes

- **WHEN** user has modified form fields on the current staging item
- **AND** clicks another staging item in the sidebar
- **THEN** a confirmation dialog appears with title "切换暂存项" and content "当前表单有未保存的更改，切换后将丢失。确定要切换吗？"
- **AND** if user confirms, the switch proceeds and `stagingDirty` is reset
- **AND** if user cancels, the switch is aborted

### Requirement: URL parsing SHALL auto-fill connection form

`useUrlBuilder` SHALL expose a `parseUrl(raw: string): ParsedUrl | null` function that parses both file-based database URLs (`sqlite:///path/to/db.sqlite`) and standard database URLs (`mysql://user:pass@host:3306/db?params`). `DataSourceHeader` SHALL emit a `parseUrl` event when a "解析" button is clicked. `AddDataSourceDialog` SHALL handle this event by matching the parsed driver, setting the driver selection, and populating `formData`.

#### Scenario: User parses a MySQL URL

- **WHEN** user enters `mysql://root:password@localhost:3306/mydb` in the URI field and clicks "解析"
- **THEN** MySQL driver is selected automatically
- **AND** `formData` is populated with host=localhost, port=3306, database=mydb, username=root, password=password
- **AND** `message.success("URL 解析成功，已自动填充连接字段")` is shown

#### Scenario: User parses an invalid URL

- **WHEN** user enters a malformed URL that cannot be parsed
- **THEN** `parseUrl` returns null
- **AND** `message.warning("无法解析该 URL...")` is shown
- **AND** form data is not modified

## AUDIT R13 Requirements (2026-06-11)

基于全链路审计的 10 项修复 + 2 项假正确认。

### Requirement: handleCreateApply scope SHALL be authoritative

Dialog scope checkboxes (`scope.global` / `scope.project`) SHALL be the sole authority for connection creation scope. `StagingItem.scope` SHALL only be used for initial display when a staging item is selected.

#### Scenario: User unchecks global scope

- **WHEN** a staging item was saved with `scope: 'both'` but user unchecks "全局连接" in dialog
- **THEN** global connection is NOT created
- **AND** only project connection is created (if project is checked)

### Requirement: finalAuthMethod SHALL only fallback on null/undefined

`saveToProjectOnly` SHALL use `??` operator instead of `||` for `finalAuthMethod` resolution, preventing empty string from falling through to the current dialog's authMethod.

### Requirement: stagingDirty SHALL NOT be triggered during data restore

An `isRestoring` ref SHALL suspend the stagingDirty watch during `handleSelectStaging` data restoration. `isRestoring` SHALL be set to `true` before restoring and `false` after `nextTick`.

#### Scenario: User selects a staging item

- **WHEN** user clicks a staging item in the sidebar
- **THEN** form data, scope, authConfigId, etc. are restored
- **AND** stagingDirty is NOT set to true during restore
- **AND** subsequent form modifications correctly set stagingDirty

### Requirement: handleClose SHALL check stagingDirty

Dialog close handler SHALL include `stagingDirty.value` in the unsaved changes check, not just un-applied staging items.

#### Scenario: User modifies form but doesn't save to staging

- **WHEN** user edits form fields on the current staging item
- **AND** clicks close without clicking "暂存"
- **THEN** close confirmation dialog appears

### Requirement: All user-facing strings SHALL be internationalized

DataSourceHeader parse button, URL parse messages, and duplicate name warnings SHALL use `t()` from vue-i18n with corresponding keys in `zh-CN.json` and `en.json`.

#### Scenario: User uses English locale

- **WHEN** locale is set to English
- **THEN** parse button shows "Parse", not "解析"
- **AND** URL parse success shows "URL parsed successfully", not Chinese text

### Requirement: AdvancedTab SHALL emit environmentId consistently

AdvancedTab's `extra-config` emit SHALL use key `environmentId` (not `envId`) to match `AddDataSourceDialog.onExtraConfig` handler. The initial value SHALL be `null`, not a hardcoded `'env-dev'`.

#### Scenario: User selects an environment in AdvancedTab

- **WHEN** user changes environment in EnvironmentSection
- **THEN** `selectedEnvId` in AddDataSourceDialog is updated via `onExtraConfig`
- **AND** the environment ID is persisted when saving to staging

### Requirement: Staging items SHALL reload when dialog reopens

`watch(props.modelValue, ...)` SHALL call `loadStagingItems()` when the dialog opens, ensuring staging items from previous sessions and localStorage are properly loaded.

#### Scenario: User closes and reopens the dialog

- **WHEN** dialog is reopened after being closed
- **THEN** staging items are reloaded from localStorage
- **AND** the staging list in the sidebar reflects current state

## AUDIT R14 Requirements (2026-06-11)

基于深层审计的 8 项修复，聚焦异步竞态、错误处理、状态管理。

### Requirement: loadAll SHALL be awaited before dependent operations

`watch open` handler in AddDataSourceDialog and `onMounted` in NetworkTab SHALL `await loadAll()` before executing dependent operations (`initFromConnection`, `loadAllProject`).

#### Scenario: Edit connection with initialConnection prop

- **WHEN** dialog opens with `initialConnection` prop set
- **THEN** drivers are loaded before `initFromConnection` is called
- **AND** the driver is correctly matched by `driver_id`

#### Scenario: NetworkTab loads global then project profiles

- **WHEN** NetworkTab is mounted with project scope
- **THEN** global profiles are loaded first
- **AND** project profiles are loaded after
- **AND** project profiles are NOT lost due to race condition

### Requirement: useNetworkProfiles SHALL separate global and project profiles

Network profiles SHALL be stored in separate `global*Profiles` and `project*Profiles` arrays, with exposed `computed` merged lists. `loadAll` SHALL replace global arrays; `loadAllProject` SHALL replace project arrays.

#### Scenario: Dialog reopens with project context

- **WHEN** `loadAll()` is called followed by `loadAllProject(path)`
- **THEN** `sshProfiles` = global profiles + project profiles
- **AND** calling `loadAll()` again does NOT clear project profiles (they are in separate storage)

### Requirement: handleEditApply SHALL handle partial success

When editing a connection with both project and global scope, each update SHALL be independently error-handled. If one succeeds and one fails, the user SHALL be notified which one failed.

#### Scenario: Project update succeeds but global update fails

- **WHEN** scope includes both project and global
- **AND** `updateConnection()` succeeds
- **AND** `updateGlobalConnection()` fails
- **THEN** a warning message is shown: "Partial success: Project Connection saved, Global Connection failed"
- **AND** the dialog is closed (project changes are committed)

#### Scenario: Both updates fail

- **WHEN** both project and global updates fail
- **THEN** an error message is shown
- **AND** the dialog remains open

### Requirement: useDriverRegistry SHALL skip reload when already fetched

`loadAll()` SHALL return immediately if `fetched.value` is true, avoiding redundant network requests.

### Requirement: Password SHALL only be sent via IPC when non-empty

`buildConnectOpts` SHALL use `fd.password ? String(fd.password) : undefined` to avoid sending empty strings through IPC. Similarly, `handleEditApply` password fields SHALL use the same pattern.

### Requirement: Config type guards SHALL be mutually exclusive

`isSslConfig` and `isProxyConfig` SHALL include mutual exclusion checks (`!('type' in v)` / `!('mode' in v)`) to prevent ambiguous configurations from being matched by the wrong guard.

## AUDIT R15 Requirements (2026-06-11)

基于代码复用审计和死代码扫描的 4 项修复（3 死代码 + 1 Bug）。

### Requirement: Dead Pinia Store SHALL be removed

`networkConfigStore.ts` (106 lines) SHALL be deleted. The store has zero external imports and its functionality (SSH/SSL/Proxy profile CRUD) is fully covered by `useNetworkProfiles`. The store also uses dynamic `import('@tauri-apps/api/core')` which is an anti-pattern compared to the static imports used by composables.

#### Scenario: Grep for networkConfigStore imports

- **WHEN** searching all files in the connection module for `from.*networkConfigStore` or `import.*networkConfigStore`
- **THEN** zero matches are found
- **AND** the file is safe to delete

### Requirement: Dead Rust wrapper function SHALL be removed

`project_query_network_config` (L431-436 of `connection_commands.rs`) SHALL be deleted. The function is annotated with `#[allow(dead_code)]` and merely wraps `project_query_network_config_with_auth`, discarding `network_type` and `auth_config_id` from the return tuple. All callers require the full triple and use `_with_auth` directly.

#### Scenario: Dead code analysis

- **WHEN** `project_query_network_config` is searched for callers
- **THEN** only the definition and its `#[allow(dead_code)]` annotation are found
- **AND** all actual callers use `project_query_network_config_with_auth`

### Requirement: Deprecated no-op function SHALL be removed

`useNetworkChain.initProfiles()` SHALL be deleted from both the function definition and the return block. The function was marked `@deprecated` in Round 14 indicating profile state management moved to `useNetworkProfiles`, but the function body and export were not cleaned up.

#### Scenario: Verify zero callers

- **WHEN** searching for `initProfiles(` across the entire project
- **THEN** only the definition and return block export are found
- **AND** no actual callers exist

### Requirement: watch callback SHALL use async when containing await

`AddDataSourceDialog.vue` watch callback for `props.modelValue` SHALL use `async (open) => {` instead of `open => {`. The callback contains `await loadAll(...)` which requires an async context. Without async, TypeScript emits TS1308: "'await' expressions are only allowed within async functions".

#### Scenario: TypeScript compilation

- **WHEN** `vue-tsc --noEmit` is run
- **THEN** no TS1308 error is emitted for `AddDataSourceDialog.vue`
- **AND** the watch callback correctly awaits driver loading before calling `initFromConnection`

## AUDIT R16 Requirements (2026-06-12)

基于代码复用审计的 5 项修复，消除 `ConnectionInfoResponse` 和 `ConnectRequest` 两处重复构造逻辑，清理协议链死代码。

### Requirement: ConnectionInfoResponse SHALL have a single construction path

`ConnectionInfoResponse` SHALL be constructed via `ConnectionInfoResponse::from_info(info, is_active)` rather than duplicated struct literals in `get_connections`, `get_active_connection`, and `detect_global_connections_in_project`.

#### Scenario: New field added to ConnectionInfoResponse

- **WHEN** a new field is added to `ConnectionInfoResponse`
- **THEN** only `from_info` needs to be updated
- **AND** all three callers automatically include the new field

### Requirement: ConnectRequest SHALL be constructed via ConnectDatabaseInput::into_connect_request

`ConnectDatabaseInput` SHALL expose `into_connect_request(connection_type, network_method, skip_persistence)` that converts itself to `ConnectRequest`. Both `connect_database` and `test_connection` SHALL use this method instead of duplicating the 21-field struct literal.

#### Scenario: New field added to ConnectRequest

- **WHEN** a new field is added to `ConnectRequest` and `ConnectDatabaseInput`
- **THEN** only `into_connect_request` needs to be updated
- **AND** both `connect_database` and `test_connection` automatically include the new field

#### Scenario: test_connection creates a temporary connection

- **WHEN** `test_connection` is called
- **THEN** a `ConnectDatabaseInput` is constructed from individual params
- **AND** `into_connect_request(ConnectionType::Global, network_method, Some(true))` is called
- **AND** `skip_persistence` is set to `Some(true)` to prevent persisting test connections

### Requirement: useAddDataSource SHALL NOT duplicate protocol chain logic

`useAddDataSource.ts` SHALL NOT contain `ProtocolType`, `ChainHopItem`, `countNetworkHops`, `ensureSslAtEnd`, `addHop`, `removeHop`, `onDrop`, `toggleHop` definitions or functions. All protocol chain logic SHALL be delegated to `useNetworkChain`.

#### Scenario: Protocol chain manipulation

- **WHEN** any protocol chain operation is needed
- **THEN** `useNetworkChain` is the single source of truth
- **AND** `useAddDataSource` does not contain duplicate implementations

## AUDIT R17 Requirements (2026-06-12)

基于全链路可用性审计的 7 项修复，涵盖密码链路断裂、表单双向同步、URL 解析/构建健壮性、编辑模式安全性。

### Requirement: connectDatabase service SHALL pass password to backend

`connectDatabase` service function SHALL include `password` in its `opts` parameter and map it to `ConnectDatabaseInput.password`.

#### Scenario: User applies a connection with password

- **WHEN** `handleCreateApply` calls `connectDatabaseService` with password
- **THEN** `ConnectDatabaseInput.password` is set to the user's password
- **AND** the backend receives the password for encrypted storage

### Requirement: Password SHALL survive the staging strip

`buildStagingItem` strips password from `formData` for localStorage security. `handleCreateApply` SHALL capture `formData.value.password` before `syncCurrentToStaging()` and pass it via `currentPassword` to `buildConnectOpts`/`saveToProjectOnly`/`saveToProject`.

#### Scenario: User fills password and clicks Apply

- **WHEN** `handleCreateApply` runs
- **THEN** password is captured before `syncCurrentToStaging()` strips it
- **AND** password is passed to `buildConnectOpts` as `currentPassword`
- **AND** password is passed to `saveToProjectOnly`/`saveToProject`
- **AND** password reaches the backend via `ConnectDatabaseInput.password`

### Requirement: stagingDirty SHALL monitor all editable fields

The `stagingDirty` watch SHALL monitor all 13 editable state sources: `formData`, `headerData`, `selectedEnvId`, `networkConfigId`, `authConfigId`, `authMethod`, `driverPropertiesExtra`, `advancedOptions`, `schemaName`, `options`, `metadataPath`, `tags`, `useDuckdbFed`.

#### Scenario: User edits DriverProps tab

- **WHEN** `driverPropertiesExtra` changes
- **THEN** `stagingDirty` is set to `true`
- **AND** switching staging items prompts confirmation

### Requirement: GeneralTab SHALL synchronize local form from external formData changes

`GeneralTab` SHALL watch `props.formData` and call `syncFromFormData()` to update the `local` reactive object. This ensures URL parsing (`onParseUrl`) fills are reflected in input fields.

#### Scenario: URL parsing fills form fields

- **WHEN** `parseUrl` succeeds and `formData.value` is set
- **THEN** `GeneralTab` detects the change via `watch(props.formData, ...)`
- **AND** `local` reactive object is synchronized
- **AND** input fields display the parsed values

### Requirement: parseUrl SHALL support IPv6 addresses

`parseUrl` regex SHALL match IPv6 addresses in brackets: `[::1]`, `[2001:db8::1]`, etc.

#### Scenario: User pastes URL with IPv6 host

- **WHEN** user pastes `mysql://root@[::1]:3306/mydb`
- **THEN** `parseUrl` extracts host as `::1`
- **AND** port as `3306`
- **AND** database as `mydb`

### Requirement: buildUrl SHALL encode special characters in credentials

`buildUrl` and `applyTemplate` SHALL apply `encodeURIComponent()` to username and password values to prevent URL parsing ambiguity from `@`, `:`, `/` characters.

#### Scenario: User enters password containing special characters

- **WHEN** password is `p@ss:word`
- **THEN** `buildUrl` encodes it as `p%40ss%3Aword`
- **AND** the resulting URL is valid and parsable

### Requirement: handleEditApply SHALL NOT overwrite password with empty string

`handleEditApply` SHALL pass `undefined` (not `''`) when the user does not fill the password field, allowing the backend to preserve the existing encrypted password.

#### Scenario: User edits a connection but leaves password blank

- **WHEN** `handleEditApply` runs with empty password field
- **THEN** `password` is `undefined`
- **AND** backend preserves the existing encrypted password

## AUDIT R18 Requirements (2026-06-12)

基于功能完善度评估，修复测试连接流程中密码字段缺失问题。

### Requirement: test_connection SHALL accept and pass through password

`test_connection` Tauri command SHALL accept `password: Option<String>` parameter and pass it to `ConnectDatabaseInput` (previously hardcoded to `None`). This enables the test connection flow to verify the encrypted password storage chain.

#### Scenario: User tests connection with password

- **WHEN** `handleTest` calls `invoke('test_connection', params)` with `password` field
- **THEN** backend receives `password` in `ConnectDatabaseInput`
- **AND** `ConnectRequest.password` is set to the user's password
- **AND** the connection test validates the full credential chain (URL + encrypted password)

#### Scenario: User tests connection without password (e.g., os_auth/trust)

- **WHEN** `handleTest` calls `invoke('test_connection', params)` without `password` field
- **THEN** backend `password` is `None`
- **AND** connection proceeds using only the URL-based authentication
