# 新增数据源原型 v5 — 全面结构化分析文档

> 文件来源：`prototype/add-datasource-v5.html`（约 3457 行，173KB）  
> 分析日期：2026-05-21  
> 分析语言：zh-CN

---

## 目录

1. [所有标签页（Tab）及其表单字段](#1-所有标签页tab及其表单字段)
2. [认证系统](#2-认证系统)
3. [网络/协议链系统](#3-网络协议链系统)
4. [驱动选择和能力展示](#4-驱动选择和能力展示)
5. [存储范围切换](#5-存储范围切换)
6. [配置文件管理器](#6-配置文件管理器)
7. [认证配置管理器](#7-认证配置管理器)
8. [数据源列表表（左侧面板）](#8-数据源列表表左侧面板)
9. [所有模态框和覆盖层](#9-所有模态框和覆盖层)
10. [状态管理与事件流](#10-状态管理与事件流)
11. [CSS 变量和样式模式](#11-css-变量和样式模式)
12. [响应式行为](#12-响应式行为)

---

## 1. 所有标签页（Tab）及其表单字段

主面板的 `tabs-bar` 包含 **5 个标签页**：常规、网络、能力、驱动属性、高级。切换通过 `switchTab(name)` 实现。

### 1.1 常规（`tab-general`）

这是默认激活标签页，包含连接核心配置：

| 字段区域                                              | 字段                              | 输入类型                 | 默认值          | 说明                                                                                  |
| ----------------------------------------------------- | --------------------------------- | ------------------------ | --------------- | ------------------------------------------------------------------------------------- |
| **连接参数**（`form-network`，网络数据库可见）        | 主机                              | `input type="text"`      | `localhost`     | 数据库服务器地址                                                                      |
|                                                       | 端口                              | `input type="number"`    | `3306`          | 连接端口，随驱动变化                                                                  |
|                                                       | 数据库                            | `input type="text"`      | `mydb`          | 数据库名称                                                                            |
| **文件路径**（`form-file`，文件数据库可见，默认隐藏） | 数据库文件                        | `input type="text"`      | `./database.db` | 文件路径，配`📂 浏览`和`📄 新建`按钮                                                  |
| **数据库认证**（`form-auth`，网络数据库可见）         | 认证方法类型（左列）              | `select`                 | `password`      | 下拉选项：SCRAM-SHA-256、SSL客户端证书(mTLS)、GSSAPI Kerberos、OAuth 2.0 Bearer Token |
|                                                       | 已保存配置（右列）                | `select` + `📋 管理`按钮 | —               | 根据左侧类型过滤可选已保存配置                                                        |
|                                                       | 动态字段区（`authDynamicFields`） | 动态渲染                 | **见第2节**     | 根据认证方法类型动态渲染用户名/密码/证书/Keytab等字段                                 |

**UI 特性**：

- `info-banner` 显示驱动信息横幅
- `drv-tag` 显示驱动类型标签（sqlx/diesel/python/go/native）
- 文件数据库和网络数据库的表单通过 `display:none/''` 动态切换
- `📄 新建` 按钮调用 `createNewDbFile()`，弹出 prompt 输入路径

### 1.2 网络（`tab-network`）

**为网络数据库显示协议链编辑器**，文件数据库显示提示信息（`network-file-db-hint`）。

| 组件                                | 说明                                                    |
| ----------------------------------- | ------------------------------------------------------- |
| `network-hint`                      | 说明文字：动态协议链、最大 4 跳、SSL 在末尾、拖拽排序   |
| `chain-header`                      | 列头：拖拽列、顺序列(#)、协议列、配置列、启用列、操作列 |
| `chain-list`                        | 协议链节点列表（动态渲染）                              |
| `chain-warning`                     | 延迟警告横幅（≥3 跳显示），显示跳数和预估延迟           |
| `add-hop-section` + `hop-type-menu` | 弹出菜单添加 SSH/Proxy/SSL 节点                         |
| `topo-preview`                      | 数据路径拓扑预览图                                      |
| `network-file-hint`                 | 文件数据库提示（隐藏时显示）                            |

详细协议链字段见**第3节**。

### 1.3 能力（`tab-capabilities`）

展示当前驱动器的功能矩阵，动态渲染到 `capTableBody`。

| 列     | 内容                                                              |
| ------ | ----------------------------------------------------------------- |
| 能力项 | 事务支持、预编译语句、流式查询、Schema 自省、Arrow 导出、插件兼容 |
| 状态   | `✓ 支持`（绿色cap-badge）或 `✗ 不支持`（红色cap-badge）           |
| 说明   | 固定 `—`                                                          |

数据来源：`currentDriver.capabilities` 对象。

### 1.4 驱动属性（`tab-properties`）

可扩展的 key-value 属性表，渲染到 `propsTableBody`。

| 列     | 内容                                             |
| ------ | ------------------------------------------------ |
| 属性名 | 如 `pool.max_connections`、`diesel.print_schema` |
| 值     | `input` 可编辑                                   |
| 说明   | 固定 `—`                                         |

数据来源：`currentDriver.properties` 对象。

### 1.5 高级（`tab-advanced`）

最复杂的标签页，包含以下区域：

| 区域                   | 子组件                         | 字段/控件                                                                                                          | 说明                               |
| ---------------------- | ------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ---------------------------------- |
| **环境选择**           | `env-dropdown` 紧凑下拉        | 当前环境名称、颜色点、箭头                                                                                         | 展开列出所有环境，显示策略摘要标签 |
|                        | `env-policy-inline` 策略摘要行 | 只读/写确认/DDL确认/DROP禁用/行限/审计等标签                                                                       | 根据当前环境的策略动态渲染         |
| **DuckDB 本地加速**    | `accel-card-enhanced` 加速卡片 | 开关切换(`switch-toggle`)、同步策略(`select`)、同步间隔(分钟)、内存上限(MB)、线程数                                | 可展开/折叠 body                   |
| **安全策略（可折叠）** | `securityPolicySection`        | 见下方                                                                                                             | 可折叠，带摘要行                   |
|                        | 安全策略行 1                   | 默认只读连接(`switch-toggle`)、写操作二次确认(`switch-toggle`)、DDL操作确认(`switch-toggle`)                       |                                    |
|                        | 安全策略行 2                   | DROP/TRUNCATE(`select`: 允许/确认/禁用)、自动提交(`switch-toggle`)、结果行数上限(`input`)、结果集上限(MB)(`input`) |                                    |
| **连接参数**           | `adv-grid` 2x2网格             | 连接超时(秒, 默认 30)、查询超时(秒, 默认 0)、保活间隔(秒, 默认 60)、最大重连次数(默认 3)                           |                                    |
| **Schema + 编码**      | `adv-inline-row`               | Schema加载(`select`: 自动/按需)、字符编码(`select`: UTF-8/GBK)                                                     | 同一行排列                         |

**环境策略引擎**（对标 DBeaver Connection Type）：

- 5 个内置环境：开发(dev)、测试(test)、预发布(staging)、生产(prod)、沙箱(sandbox)
- 每个环境定义五类策略：`security`（安全）、`schema`、`performance`（性能）、`audit`（审计）、`ui`
- 新建环境可从现有模板继承策略
- 策略字段可被用户覆盖（手动修改后标记为"已覆盖"）

---

## 2. 认证系统

### 2.1 布局结构

认证采用**两列布局**：

| 左列                                | 右列                                                |
| ----------------------------------- | --------------------------------------------------- |
| 认证方法类型选择器 `authTypeSelect` | 已保存配置选择器 `authConfigSelect` + `📋 管理`按钮 |

左侧决定**认证方法类型**（password / pg_class / kerberos / oauth2），右侧可选**预保存的认证配置**，也可不选。

### 2.2 认证类型定义（`AUTH_TYPE_DEFS`）

| 类型 key          | 类别     | 图标 | 标签                                  | 字段                                        |
| ----------------- | -------- | ---- | ------------------------------------- | ------------------------------------------- |
| `password`        | database | 🔑   | SCRAM-SHA-256 / mysql_native_password | `username`, `password`                      |
| `pg_class`        | database | 📜   | SSL 客户端证书 (mTLS)                 | `certPath`, `certKeyPath`                   |
| `kerberos`        | database | 🎫   | GSSAPI Kerberos                       | `principal`, `keytabPath`                   |
| `oauth2`          | database | 🔗   | OAuth 2.0 Bearer Token                | `tokenEndpoint`, `clientId`, `clientSecret` |
| `ssh_password`    | ssh      | 🔑   | SSH 密码认证                          | `username`, `password`                      |
| `ssh_private_key` | ssh      | 🔐   | SSH 公钥认证 (RSA/ED25519/ECDSA)      | `username`, `keyPath`, `passphrase`         |

### 2.3 认证配置选择与渲染逻辑

**选择流程**：

1. 用户选择认证类型（`onAuthTypeChange`）→ 重置已保存配置选择，按类型过滤右侧下拉框（`populateAuthConfigSelect`），渲染对应动态字段（`renderAuthFields`）
2. 用户选择已保存配置（`onAuthConfigSelect`）→ 自动同步左侧认证类型，渲染预填禁用表单，显示预览提示
3. 不选已保存配置 → 手动填写的可编辑表单

**已保存配置数据结构（`AUTH_CONFIGS`）**：

| 字段                                                                                                                        | 说明                             |
| --------------------------------------------------------------------------------------------------------------------------- | -------------------------------- |
| `id`                                                                                                                        | 唯一标识（如 `auth-prod-mysql`） |
| `name`                                                                                                                      | 用户命名                         |
| `scope`                                                                                                                     | `global` / `project`             |
| `auth_type`                                                                                                                 | 认证类型 key                     |
| `desc`                                                                                                                      | 备注                             |
| `username` / `password` / `certPath` / `keyPath` / `principal` / `keytabPath` / `tokenEndpoint` / `clientId` / `passphrase` | 各类型特有字段                   |

**预置数据**：

- 数据库认证 5 条（3 条 password、1 条 pg_class、1 条 kerberos）
- SSH 认证 3 条（1 条 ssh_password、2 条 ssh_private_key）

**UX 细节**：

- 选择已保存配置后，动态表单字段**只读（disabled + opacity 0.65）**
- 显示加密提示：`🔐 凭据来自认证配置「XX」，已加密存储 (AES-256-GCM)`
- 附加预览提示行（`authPreviewHint`）：名称、类型标签、范围、描述

---

## 3. 网络/协议链系统

### 3.1 核心数据模型

```js
protocolChain: Array<{
  id: string,            // 'hop-1', 'hop-2', ...
  protocol: 'ssh'|'proxy'|'ssl',
  enabled: boolean,
  mode: 'select'|'new'|'custom',
  profileId: string      // 引用的配置文件 ID
}>
```

### 3.2 约束规则

| 规则             | 说明                                                                 |
| ---------------- | -------------------------------------------------------------------- |
| **SSL 固定末尾** | SSL/TLS 是流加密包装器，不产生新网络节点，始终位于协议链末尾         |
| **跳数上限**     | SSH/Proxy 网络跳硬上限 `MAX_NETWORK_HOPS = 4`                        |
| **延迟警告**     | ≥ `WARN_NETWORK_HOPS = 3` 跳时显示黄色警告横幅（预估每跳 25ms 延迟） |
| **不删底线**     | 每种协议至少保留 1 个实例（删除按钮禁用，cursor:not-allowed）        |
| **SSL 唯一**     | 只能有一个 SSL 节点，添加新 SSL 会替换旧的                           |

### 3.3 三种协议类型的配置参数

| 协议      | 新建表单字段                                                                                                                                                                                                                                        | 存储位置           |
| --------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------ |
| **SSH**   | 名称、范围归属（自动跟随全局/项目勾选）、主机、端口、SSH 认证类型（密码/公钥）、已保存 SSH 认证配置下拉、用户/密码/私钥文件/Passphrase 动态字段、保活(秒)、本地端口(如不填则`null`=自动分配)、远程地址(如不填则`''`)、远程端口(如不填则`null`=自动) | `SSH_PROFILES[]`   |
| **SSL**   | 名称、范围归属、模式(verify-full/verify-ca/require)、CA证书、客户端证书                                                                                                                                                                             | `SSL_PROFILES[]`   |
| **Proxy** | 名称、范围归属、类型(SOCKS5/HTTP/SOCKS4)、主机、端口、代理认证（无认证/用户名密码）、已保存代理认证配置下拉、用户名/密码动态字段                                                                                                                    | `PROXY_PROFILES[]` |

### 3.4 操作状态机

```
select → new → 填表保存 → select (profileId=新ID)
select → custom → 一次性自定义（不保存） → 关闭 → select
```

- `switchHopMode(hopId, mode)` 切换模式
- `saveNewHop(hopId)` 保存新配置到 Profiles 数组，自动应用
- 保存后弹窗确认范围归属（🌐全局 / 📝项目）

### 3.5 配置文件数据结构

**SSH_PROFILES**：

```js
{
  ;(id,
    name,
    scope,
    host,
    port,
    username,
    authType, // authType: 'ssh_password'|'ssh_private_key'|'key'
    authConfigId,
    keyPath,
    localPort,
    remoteHost,
    remotePort,
    keepAlive)
}
```

**SSL_PROFILES**：

```js
{
  ;(id, name, scope, mode, ca, cert)
}
```

**PROXY_PROFILES**：

```js
{
  ;(id, name, scope, type, host, port, proxyAuthType, proxyAuthConfigId, proxyUser)
}
```

### 3.6 拖拽排序

- HTML5 Drag & Drop API
- 状态变量：`dragSrcId`、`dragOverId`
- SSL 节点**不能拖到中间位置**（只允许末尾）
- 非 SSL 节点**不能拖到 SSL 后面**
- 每次 drop 后自动执行 `ensureSslAtEnd()` 纠正 SSL 位置
- CSS 反馈：`.drag-over`（蓝色边框高亮）、`.dragging`（低透明度虚线）

### 3.7 拓扑预览（`topo-preview`）

- 显示本机 → SSH/Proxy 节点 → TLS 箭头 → 目标数据库的可视化路径
- 各节点用不同颜色：`self`(蓝)、`ssh-jump`(绿)、`proxy-node`(紫)、`db-target`(橙)
- TLS 加密段用虚线箭头 + 蓝色标签区分
- 若引用了已保存配置，显示配置名称和转发目标信息

---

## 4. 驱动选择和能力展示

### 4.1 驱动注册表（`DRIVERS`）

每个驱动对象结构：

```js
{
  id, dbType, lang, tag, tagClass, name, urlTemplate,
  defaultPort, isFile, info,
  capabilities: { transactions, preparedStmt, streaming, introspection, arrowExport, pluginCompat },
  properties: { key: value, ... }
}
```

**已注册驱动**：

| 数据库     | 驱动                    | 语言   | 标签   | 类型 |
| ---------- | ----------------------- | ------ | ------ | ---- |
| MySQL      | sqlx (async)            | Rust   | sqlx   | 网络 |
| MySQL      | diesel (ORM)            | Rust   | diesel | 网络 |
| MySQL      | Python (WASI, PyMySQL)  | Python | python | 网络 |
| MySQL      | Go (WASI)               | Go     | go     | 网络 |
| PostgreSQL | sqlx (async)            | Rust   | sqlx   | 网络 |
| PostgreSQL | diesel (ORM)            | Rust   | diesel | 网络 |
| PostgreSQL | Python (WASI, psycopg2) | Python | python | 网络 |
| SQLite     | rusqlite (native)       | Rust   | native | 文件 |
| SQLite     | Python (WASI, sqlite3)  | Python | python | 文件 |
| DuckDB     | duckdb-rs (native)      | Rust   | native | 文件 |
| DuckDB     | Python (WASI, duckdb)   | Python | python | 文件 |

### 4.2 驱动选择 UI

- 位于面板头部：`driver-select` 下拉框（以 CSS 自定义箭头图标）
- 旁边显示实时生成的 **URI 预览**（`uri-display`），monospace 字体
- `✎` 按钮切换 URI 编辑模式
- 名称/描述/范围复选也在同一 header 区域

### 4.3 能力矩阵展示

- 能力页签中的 `props-table`
- 6 项能力，值从 `capabilities` 对象读取
- ✓ 支持 = 绿色 `cap-badge.yes`，✗ 不支持 = 红色 `cap-badge.no`
- 标签：`事务支持`、`预编译语句`、`流式查询`、`Schema 自省`、`Arrow 导出`、`插件兼容`

### 4.4 驱动属性编辑

- 属性页签中的可编辑 key-value 表
- 值列可输入编辑
- 来源于 `driver.properties` 对象

---

## 5. 存储范围切换

### 5.1 UI 控件

位于面板头部第一行的右侧，两个复选框：

```html
<label class="scope-checkbox"> <input type="checkbox" id="scopeGlobal" checked /> 全局链接 </label>
<label class="scope-checkbox"> <input type="checkbox" id="scopeProject" /> 项目链接 </label>
```

### 5.2 状态查询

```js
getActiveScopes() → { global, project, both, none }
getScopeLabel()  → '全局' | '项目' | '全局+项目'（双选）
getScopeClass()  → 'global' | 'project'
```

### 5.3 影响范围

- `onScopeChange()` 触发拓扑预览更新
- 新建配置的范围**自动跟随**当前存储范围（`getActiveScopes()`）
- 配置文件在管理器中独立选择范围
- 保存数据源时，若未选任何范围则弹窗阻止

---

## 6. 配置文件管理器

### 6.1 入口

协议链中每个节点的 `📋 管理` 按钮 → `openProfileManager(protocol)`

### 6.2 覆盖层结构

```
profileManagerOverlay (固定定位全屏遮罩, z-index:10000)
└── manager-dialog (720px宽, 620px高)
    ├── manager-titlebar (动态标题切换)
    ├── manager-tabs (SSH 隧道 | SSL/TLS | 代理)
    └── manager-body (滚动内容)
```

### 6.3 CRUD 功能

| 操作     | 实现方式                                                                                       |
| -------- | ---------------------------------------------------------------------------------------------- |
| **列表** | `profile-card` 卡片列表，显示范围徽章、名称、详情、编辑/删除按钮                               |
| **新建** | `addManagerProfile()` → 渲染完整表单（根据当前 tab 不同）                                      |
| **编辑** | `editManagerProfile(id)` → 从数组删除旧条目 → 调用 `addManagerProfile()` → `setTimeout` 预填值 |
| **删除** | `deleteManagerProfile(id)` → 确认后 splice 数组 → `updateManagerUI()`                          |
| **测试** | `testManagerProfile()` → 模拟异步 800ms 延迟 → 弹窗显示结果                                    |

**SSH 管理器新建表单**包含两列认证结构（左：认证类型下拉，右：已保存配置下拉），与主数据源的认证系统布局一致。

### 6.4 范围独立

管理器中新建配置的范围通过 `mgrScope` 下拉框**独立选择**（不同于主数据源的自动跟随）。

---

## 7. 认证配置管理器

### 7.1 入口

常规标签页的 `📋 管理` 按钮 → `openAuthConfigManager()`

### 7.2 覆盖层结构

```
authManagerOverlay
└── manager-dialog (620px宽)
    ├── manager-titlebar (含内嵌 sub-tabs: 📊 数据库认证 | 🖥 SSH 认证)
    └── manager-body
```

### 7.3 分类管理

- **数据库认证**子 tab：展示 password、pg_class、kerberos、oauth2 四类的已保存配置
- **SSH 认证**子 tab：展示 ssh_password、ssh_private_key 两类的已保存配置

每种类型有分组标题，每个配置以 `profile-card` 展示。

### 7.4 CRUD 功能

| 操作     | 实现                                                                                |
| -------- | ----------------------------------------------------------------------------------- |
| **新建** | `showAddAuthForm()` → 选择认证类型/范围/名称 → 动态字段 → 保存                      |
| **编辑** | `editAuthConfig(id)` → 切换到正确分类 tab → 渲染表单预填 → `saveEditAuthConfig(id)` |
| **删除** | `deleteAuthConfig(id)` → 确认 → splice → 若当前数据源引用了该配置则回退             |

### 7.5 与主数据源的联动

- 关闭管理器时自动刷新 `authConfigSelect` 下拉框（`populateAuthConfigSelect()`）
- 重置认证类型为 `password`（`onAuthTypeChange('password')`）
- 恢复已选中的配置 ID

---

## 8. 数据源列表表（左侧面板）

### 8.1 结构

```
sidebar (240px 宽, 固定宽度)
├── sidebar-search (搜索框 + 🔍图标)
├── saved-section (暂存列表)
│   ├── saved-section-title "暂存列表 [+ 添加]"
│   └── saved-item 列表（可点击切换）
├── sidebar-divider
└── db-section (数据库类型)
    ├── db-section-title "数据库类型"
    ├── db-category[expanded] "关系型数据库 (4)"
    │   └── db-type-item[selected] MySQL (4 驱动)
    │   └── db-type-item PostgreSQL (3 驱动)
    │   └── db-type-item MariaDB (1 驱动)
    │   └── db-type-item SQL Server (1 驱动)
    ├── db-category "文件数据库 (2)" [collapsed]
    │   └── SQLite (2 驱动)
    │   └── DuckDB (2 驱动)
    ├── db-category "NoSQL (2)" [collapsed]
    │   └── MongoDB (1 驱动)
    │   └── Redis (1 驱动)
    └── db-category "分析型数据库 (1)" [collapsed]
        └── ClickHouse (1 驱动)
```

### 8.2 交互

- 分类可展开/折叠（`toggleCategory`），箭头旋转 90°
- 数据库项点击选中（`selectDbType`），高亮蓝色，自动加载默认驱动
- 每个项显示驱动数量徽章
- 搜索框绑定 `#sidebarSearch`（UI 就绪，但 JS 未实现搜索过滤）
- 暂存列表支持动态添加/删除条目，删除到最后一条会自动补回

### 8.3 数据库图标

MySQL: `#00758f` 背景、PostgreSQL: `#336791`、SQLite: `#003b57` 绿字、DuckDB: `#f9a825` 黑字、MariaDB: `#c0765a`、SQL Server: `#cc2927`、MongoDB: `#4db33d`、Redis: `#d82c20`、ClickHouse: `#f9a825`

---

## 9. 所有模态框和覆盖层

### 9.1 主对话框（非模态）

```html
<div class="dialog">
  (max-height:800px, 固定在页面中作为主内容) ├── dialog-titlebar (✦ 添加数据源 + 三个彩色圆点) ├──
  dialog-body (flex: 左右两栏) └── dialog-footer (测试连接 / 取消 / 保存数据源)</div
>
```

### 9.2 配置文件管理器覆盖层

```
id: profileManagerOverlay
触发器: 协议链节点的 📋 按钮, openProfileManager(protocol)
关闭: closeProfileManager(event) — 点击遮罩或 ✕ 按钮
内容: SSH / SSL / Proxy 三个子 tab 的 CRUD
尺寸: 720px × 620px
```

### 9.3 环境类型管理器覆盖层

```
id: envManagerOverlay
触发器: "管理" 按钮 (高级标签页), openEnvManager()
关闭: closeEnvManager(event) — 点击遮罩或 ✕ 按钮
内容: 5 个内置环境的策略摘要卡片 + 新建自定义环境表单 + 继承策略模板
尺寸: 520px (最大宽度)
```

### 9.4 认证配置管理器覆盖层

```
id: authManagerOverlay
触发器: 常规标签页 📋 管理 按钮, openAuthConfigManager()
关闭: closeAuthConfigManager(event) — 点击遮罩或 ✕ 按钮
内容: 内嵌 sub-tabs (📊 数据库认证 | 🖥 SSH 认证) + CRUD
尺寸: 620px
```

### 9.5 协议类型弹出菜单

```
id: hopTypeMenu (class: hop-type-menu)
触发器: + 添加协议节点 按钮, toggleHopMenu(event)
关闭: 点击菜单外部 (document click 监听器)
内容: SSH 隧道 / 代理 / SSL/TLS 加密 + 剩余跳数提示
定位: position:fixed，计算按钮底部位置
```

### 9.6 环境下拉菜单

```
id: envDropdownMenu (class: env-dropdown-menu)
触发器: 环境下拉框, toggleEnvDropdown(event)
关闭: 点击外部 (document click 监听器)
内容: 选择环境标题 + 管理按钮 + 环境列表 + 策略摘要标签 + 选中标记
定位: position:absolute，跟随 env-dropdown
```

### 9.7 连接测试提示

```
pretendTest() → 在 testResultArea 显示结果 (6秒后自动清除)
alert() 弹窗 — 用于新建数据库文件、保存配置、删除确认、测试结果等
prompt() — 用于新建数据库文件路径输入
confirm() — 用于删除操作确认
```

---

## 10. 状态管理与事件流

### 10.1 全局状态变量

| 变量               | 初始值                    | 说明                   |
| ------------------ | ------------------------- | ---------------------- |
| `currentDbType`    | `'mysql'`                 | 当前选中的数据库类型   |
| `currentEnvId`     | `'env-dev'`               | 当前选中的环境 ID      |
| `currentDriver`    | `DRIVERS['mysql-sqlx']`   | 当前选中的驱动对象     |
| `hopIdCounter`     | 1                         | 协议节点 ID 自增计数器 |
| `protocolChain`    | `[{ssh}, {proxy}, {ssl}]` | 协议链数组             |
| `dragSrcId`        | `null`                    | 拖拽源节点 ID          |
| `dragOverId`       | `null`                    | 拖拽悬停节点 ID        |
| `selectedAuthType` | `'password'`              | 认证方法类型           |
| `selectedAuthId`   | `''`                      | 已保存认证配置 ID      |
| `managerTab`       | `'ssh'`                   | 配置文件管理器当前 tab |
| `authManagerTab`   | `'database'`              | 认证配置管理器当前 tab |
| `stagingCount`     | 1                         | 暂存列表条目计数       |

### 10.2 数据存储（内存数组）

| 数组             | 存储内容               | 预置条目             |
| ---------------- | ---------------------- | -------------------- |
| `DRIVERS`        | 驱动注册表             | 11 条                |
| `SSH_PROFILES`   | SSH 隧道配置           | 4 条                 |
| `SSL_PROFILES`   | SSL/TLS 配置           | 4 条                 |
| `PROXY_PROFILES` | 代理配置               | 3 条                 |
| `AUTH_CONFIGS`   | 认证配置（数据库+SSH） | 8 条                 |
| `ENVIRONMENTS`   | 环境配置               | 5 条（内置，不可删） |

### 10.3 核心事件流

```
页面加载
└── applyDriver(currentDriver)
    ├── 渲染驱动选择器
    ├── 生成 URI 预览
    ├── 切换表单类型（网络 vs 文件）
    ├── 渲染能力矩阵
    ├── 渲染驱动属性
    ├── 应用驱动标签
    ├── renderChain() → 渲染协议链
    └── updateTopology() → 更新拓扑预览

选择数据库类型
└── selectDbType(dbType)
    └── applyDriver(defaultDriver(dbType))

切换驱动
└── onDriverChange(id)
    └── applyDriver(DRIVERS[id])

认证类型切换
└── onAuthTypeChange(type)
    ├── 重置 selectedAuthId
    ├── populateAuthConfigSelect() → 过滤已保存配置
    └── renderAuthFields(type, null) → 渲染可编辑字段

认证配置选择
└── onAuthConfigSelect(configId)
    ├── 同步左侧认证类型
    ├── renderAuthFields(type, config) → 预填+禁用
    └── renderAuthPreview(config) → 显示预览提示

协议链操作
├── addHop(protocol) → 插入节点 → renderChain() + updateTopology()
├── toggleHop(id) → 切换启用 → renderChain() + updateTopology()
├── deleteHop(id) → 删除节点 → renderChain() + updateTopology()
└── switchHopMode(id, mode) → 切换模式 → renderChain() + updateTopology()

拖拽排序
├── handleDragStart → dragSrcId = hopId, 添加 .dragging class
├── handleDragOver → 添加 .drag-over class
├── handleDrop → 验证约束, splice 数组, ensureSslAtEnd()
└── handleDragEnd → 清除样式, dragSrcId = null

环境选择
└── selectEnv(envId)
    └── applyEnvPolicies()
        ├── 设置所有策略开关
        ├── 设置连接参数
        ├── 设置 Schema 策略
        └── updateSecurityPolicySummary()

策略覆盖
└── onPolicyOverride()
    ├── 更新指示器为 "⚠ 已覆盖 XX 环境 预设"
    └── updateSecurityPolicySummary()

保存数据源
└── handleSave()
    ├── 验证名称和范围
    ├── 收集所有状态 → 汇总到 alert() 弹窗
    └── 摘要：名称、范围、环境、策略、认证、网络跳

连接测试
└── pretendTest()
    ├── 计算路径描述（含各跳节点名）
    └── 显示 testResultArea (success, 6秒后清除)
```

### 10.4 数据流（无后端持久化）

当前原型**全部数据存储在 JavaScript 内存数组**中，无 localStorage/IndexedDB/Tauri 后端。实际实现时需映射到：

- Protocol Chain → Tauri Command → `connection_manager` → `datasource/*`
- Profiles → persistence 层 (`connection_store` / 项目级持久化)
- Auth Configs → 安全的凭证存储（加密 AES-256-GCM）
- Environments → 项目级策略持久化

---

## 11. CSS 变量和样式模式

### 11.1 CSS 变量（`:root`）

| 变量               | 值                       | 用途                |
| ------------------ | ------------------------ | ------------------- |
| `--bg-base`        | `#1e1e2e`                | 页面背景            |
| `--bg-surface`     | `#1a1b26`                | 对话框/卡片表面     |
| `--bg-raised`      | `#11111b`                | 输入框/突起元素背景 |
| `--bg-hover`       | `rgba(255,255,255,0.05)` | hover 高亮          |
| `--bg-active`      | `#2a2a3c`                | active/选中背景     |
| `--border`         | `rgba(255,255,255,0.07)` | 边框色              |
| `--text-primary`   | `#cdd6f4`                | 主要文字            |
| `--text-secondary` | `#a6adc8`                | 次要文字            |
| `--text-muted`     | `#6c7086`                | 辅助/弱化文字       |
| `--accent`         | `#89b4fa`                | 强调色（蓝）        |
| `--accent-bg`      | `rgba(137,180,250,0.1)`  | 强调色背景          |
| `--success`        | `#a6e3a1`                | 成功/SSH 色（绿）   |
| `--warning`        | `#f9e2af`                | 警告色（黄）        |
| `--danger`         | `#f38ba8`                | 危险/错误色（红）   |
| `--purple`         | `#cba6f7`                | 紫色（代理主题）    |
| `--green`          | `#94e2d5`                | 青色                |
| `--orange`         | `#fab387`                | 橙色                |
| `--radius`         | `8px`                    | 通用圆角            |
| `--ssh-color`      | `#a6e3a1`                | SSH 协议专属色      |
| `--ssl-color`      | `#89b4fa`                | SSL 协议专属色      |
| `--proxy-color`    | `#fab387`                | 代理协议专属色      |

**配色方案**：暗色 Catppuccin Mocha 主题风格。

### 11.2 样式模式总结

| 模式           | 说明                                                                                                 |
| -------------- | ---------------------------------------------------------------------------------------------------- |
| **表单输入**   | `form-input` / `form-select` — 34px 高，bg-raised 背景，focus 蓝边框                                 |
| **按钮**       | `btn-primary`(蓝底白字), `btn-test`(方框), `btn-cancel`(透明), `btn-inline.save/cancel/test`         |
| **开关**       | `switch-toggle` — 36x20px 胶囊，`::after` 白色圆点滑动，ON 状态变色                                  |
| **复选框**     | `scope-checkbox` — 隐藏原生 input，用伪元素 `::after` 渲染 ✓，checked 变蓝                           |
| **标签**       | `drv-tag`(sqlx/diesel/python/go/native), `cap-badge`(yes/no), `benefit-tag`, `epi-tag`, `policy-tag` |
| **范围徽章**   | `profile-scope-badge.global`(绿), `.project`(紫)                                                     |
| **滚动条**     | 5px 宽，透明轨道，半透明滑块，hover 变亮                                                             |
| **过渡动画**   | 0.12s-0.2s ease/transition，应用于 border/background/color/opacity                                   |
| **折叠**       | `.collapse-header` + `.collapse-body.hidden`，箭头 ▶/▼ 切换                                          |
| **卡片**       | `profile-card` — flexbox 布局，hover 边框高亮                                                        |
| **覆盖层**     | `.overlay` — `position:fixed; inset:0` 全屏遮罩，rgba(0,0,0,0.6)，flex center                        |
| **协议链节点** | `chain-item`(通用) / `chain-item-ssl`(蓝色左边框 + ::after "末尾层" 标签)                            |

### 11.3 z-index 层级

| 层级  | 元素                               |
| ----- | ---------------------------------- |
| 100   | `hop-type-menu` (弹出菜单)         |
| 200   | `env-dropdown-menu` (环境下拉菜单) |
| 10000 | `.overlay` (覆盖层遮罩)            |

---

## 12. 响应式行为

### 12.1 当前状态

该原型**未实现完整的响应式设计**。具体观察：

| 特征           | 值                                                                  | 影响                       |
| -------------- | ------------------------------------------------------------------- | -------------------------- |
| 固定侧边栏宽度 | `240px` / `min-width: 240px`                                        | 小屏幕下占用比例过大       |
| 固定对话框宽度 | `max-width: 1200px` (.outer)                                        | 超出此宽度居中，低于则缩小 |
| 固定管理器宽度 | `720px` / `620px` / `520px`                                         | 小屏幕可能溢出             |
| 固定对话框高度 | `max-height: 800px`                                                 | 内部滚动                   |
| Flexbox 布局   | `.dialog-body { display: flex }`                                    | 支持适度弹性               |
| 字体           | `-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif` | 系统字体栈，无需加载       |
| 无媒体查询     | **全部 CSS 中无 `@media` 规则**                                     | 无断点适配                 |

### 12.2 存在的自适应行为

- 主面板 `.main-panel` 使用 `flex: 1`，随窗口弹性伸缩
- 表单行 `.form-row` 使用 `flex + gap`，支持换行
- URI 显示区域使用 `overflow: hidden; white-space: nowrap; min-width: 0`，长文本截断
- 拓扑预览路径使用 `flex-wrap: wrap`，节点可换行
- 策略摘要标签使用 `flex-wrap: wrap`，标签可换行

### 12.3 建议的改进方向

1. 添加 `@media (max-width: 900px)` 断点，侧边栏折叠为汉堡菜单或水平 tabs
2. 表单行在窄屏转为单列布局
3. 管理器对话框在窄屏使用 `width: calc(100vw - 32px)` 代替固定宽度
4. 协议链在窄屏使用紧凑布局（省略部分列）

---

## 附录 A：关键技术标识

| 标识       | 说明                                            |
| ---------- | ----------------------------------------------- |
| `v5.1`     | 版本号（TLS末尾 + 跳数上限 + 交替穿插）         |
| 对标产品   | DBeaver Connection Type（环境策略引擎概念来源） |
| 纯前端原型 | 无 Tauri IPC、无数据库持久化                    |
| 暗色主题   | Catppuccin Mocha 风格                           |
| 零外部依赖 | 纯 HTML+CSS+JS，无框架无库                      |

## 附录 B：文件结构地图

```
add-datasource-v5.html
├── <style> (~700 行 CSS)
├── <body>
│   ├── .dialog (主对话框)
│   │   ├── .dialog-titlebar
│   │   ├── .dialog-body
│   │   │   ├── .sidebar (左侧面板)
│   │   │   └── .main-panel (右侧面板)
│   │   │       ├── .panel-header (名称/描述/驱动/URI/范围)
│   │   │       ├── .tabs-bar (5个标签)
│   │   │       ├── .tab-content (5个标签页内容)
│   │   │       └── .dialog-footer (测试/取消/保存)
│   │   └── ...
│   ├── #profileManagerOverlay (网络配置文件管理器)
│   ├── #envManagerOverlay (环境类型管理器)
│   ├── #authManagerOverlay (认证配置管理器)
│   └── <script> (~2300 行 JS)
```

---

_文档完。_
