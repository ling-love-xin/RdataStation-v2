# 新增数据源页面文档

> **⚠️ 已废弃 (Deprecated)**
> 本文档已被 [add-datasource-frontend-plan.md](./add-datasource-frontend-plan.md) 替代。
> 请使用新版文档，本文档仅保留作历史参考，不再更新。

> 版本：v2.0
> 最后更新：2026-05-18
> 状态：⛔ 已废弃（参考 add-datasource-frontend-plan.md）

---

## 概述

新增数据源页面是 RdataStation 的核心功能之一，用于创建和管理数据源连接。页面采用 **V3 多驱动架构**，支持一种数据库类型对应多种驱动（如 MySQL 下的 sqlx / diesel / Python WASI / Go WASI），每种驱动有独立的表单字段、能力矩阵和可调属性。表单通过 JSON Schema 配置文件实现动态渲染。

## 架构演进

```
V1: datagrip-style-connection.html   →  ConnectionModal.vue（已移除）
V2: connection-modal.md（本文档初版）→  基于 JSON Schema 的动态表单
V3: add-datasource-v3.html（原型）   →  AddDataSourceDialog.vue（当前实现）
         + multi-driver-architecture.html（多驱动架构理念）
```

> **V3 核心变化**："一种数据库，多驱动选择"——用户在侧边栏选择数据库类型后，在顶部选择具体驱动，每种驱动提供独立的表单、能力和属性。

## 目录结构

```
src/extensions/builtin/connection/ui/
├── components/
│   ├── AddDataSourceDialog.vue      # 主对话框组件（V3）
│   ├── DataSourceHeader.vue         # 顶部：名称、描述、URI、驱动选择
│   ├── DataSourceSidebar.vue        # 左侧：暂存列表 + 数据库分类树
│   ├── DatabaseManager.vue          # 数据库管理器（调用方）
│   ├── DynamicFormRenderer.vue      # 动态表单渲染器
│   ├── FieldRenderer.vue            # 字段渲染器
│   └── tabs/
│       ├── GeneralTab.vue           # 常规配置标签页（动态表单）
│       ├── NetworkTab.vue           # 网络/SSH/代理配置
│       ├── CapabilitiesTab.vue      # 驱动能力展示
│       ├── DriverPropsTab.vue       # 驱动属性/自定义选项
│       └── AdvancedTab.vue          # 高级配置（DuckDB加速等）
├── composables/
│   └── useStagingList.ts            # 暂存列表管理器
├── schemas/
│   ├── mysql.json                   # MySQL 连接配置
│   ├── postgresql.json              # PostgreSQL 连接配置
│   ├── sqlite.json                  # SQLite 连接配置
│   └── duckdb.json                  # DuckDB 连接配置
├── types/
│   ├── form-schema.ts               # 表单类型定义 + 解析函数
│   ├── connection.ts                # 连接类型定义
│   └── driver.ts                    # 驱动选项类型
└── utils/
    └── schema-loader.ts             # Schema 加载器（待重新接入 V3）
```

## 核心组件

### 1. AddDataSourceDialog.vue

V3 主对话框组件，采用左右分栏 + 顶部驱动的布局：

- **左侧边栏**（DataSourceSidebar）：暂存列表 + 按分类分组的数据库类型树，支持搜索和驱动数量显示
- **顶部区域**（DataSourceHeader）：名称输入、描述、URI 显示、驱动选择下拉
- **右侧标签页**：
  - **常规（GeneralTab）**：基于 JSON Schema 动态渲染的连接表单
  - **网络（NetworkTab）**：SSH 隧道、HTTP/SOCKS 代理配置
  - **能力（CapabilitiesTab）**：驱动支持的功能矩阵展示
  - **属性（DriverPropsTab）**：驱动级别自定义选项
  - **高级（AdvancedTab）**：DuckDB 加速、缓存等高级配置
- **保存操作**：emit 给 DatabaseManager 处理连接创建和持久化

### 2. DataSourceSidebar.vue

左侧边栏组件：

- **暂存列表**：当前会话中未保存的数据源（useStagingList）
- **数据库分类树**：按类别（关系型、文件型、NoSQL、分析型）分组
- **搜索功能**：按名称过滤数据库类型
- **驱动数量徽标**：每种数据库类型显示可用驱动数

### 3. DataSourceHeader.vue

顶部区域：

- 数据源名称输入
- 描述输入
- 驱动选择下拉（根据选中 DB 类型动态过滤）
- URI 预览（实时生成连接 URL）

### 4. DynamicFormRenderer.vue

动态表单渲染器，根据 JSON Schema 渲染表单：

- 支持多种字段类型：text、password、number、select、checkbox、file、textarea
- 支持字段依赖关系（dependsOn）
- 支持折叠区块（collapsible）
- 支持 inline 布局（紧凑排列）

### 5. FieldRenderer.vue

字段渲染器组件，负责渲染单个表单字段：

- 处理不同类型的输入控件
- 处理错误状态显示
- 处理密码可见性切换
- 处理文件选择

## JSON Schema 配置

### Schema 结构

每个数据库类型的配置文件遵循以下结构：

```json
{
  "driverId": "mysql",
  "driverName": "MySQL",
  "version": "8.0",
  "metadata": {
    "category": "relational",
    "description": "MySQL 关系型数据库",
    "features": ["事务", "存储过程", "触发器", "视图"],
    "defaultPort": 3306,
    "requireFile": false,
    "supportsSsl": true,
    "supportsSshTunnel": true
  },
  "sections": [
    {
      "key": "connection",
      "title": "连接设置",
      "icon": "database",
      "collapsible": false,
      "fields": [...]
    }
  ]
}
```

### Metadata 字段说明

| 字段              | 类型     | 说明                                             |
| ----------------- | -------- | ------------------------------------------------ |
| category          | string   | 数据库类别：relational（关系型）、file（文件型） |
| description       | string   | 数据库描述                                       |
| features          | string[] | 支持的功能特性                                   |
| defaultPort       | number   | 默认端口号                                       |
| requireFile       | boolean  | 是否需要文件路径（文件数据库为 true）            |
| supportsSsl       | boolean  | 是否支持 SSL                                     |
| supportsSshTunnel | boolean  | 是否支持 SSH 隧道                                |

### Section 字段说明

| 字段        | 类型          | 说明                        |
| ----------- | ------------- | --------------------------- |
| key         | string        | 区块唯一标识                |
| title       | string        | 区块标题                    |
| icon        | string        | 图标名称（lucide-vue-next） |
| collapsible | boolean       | 是否可折叠                  |
| collapsed   | boolean       | 默认是否折叠                |
| fields      | FieldConfig[] | 字段配置数组                |

### Field 字段说明

| 字段        | 类型     | 说明                                                               |
| ----------- | -------- | ------------------------------------------------------------------ |
| key         | string   | 字段唯一标识                                                       |
| label       | string   | 字段标签                                                           |
| type        | string   | 字段类型：text、password、number、select、checkbox、file、textarea |
| placeholder | string   | 占位符文本                                                         |
| required    | boolean  | 是否必填                                                           |
| default     | any      | 默认值                                                             |
| options     | Option[] | 下拉选项（type 为 select 时使用）                                  |
| validation  | object   | 验证规则                                                           |
| tooltip     | string   | 提示文本                                                           |
| hidden      | boolean  | 是否隐藏                                                           |
| dependsOn   | object   | 依赖条件                                                           |
| inline      | boolean  | 是否行内布局                                                       |
| flex        | number   | 行内布局的 flex 比例                                               |

### 示例：添加新的数据库类型

要添加新的数据库类型（如 Oracle），只需创建 `oracle.json` 文件：

```json
{
  "driverId": "oracle",
  "driverName": "Oracle",
  "version": "21c",
  "metadata": {
    "category": "relational",
    "description": "Oracle 关系型数据库",
    "features": ["事务", "存储过程", "触发器", "视图", "分区表"],
    "defaultPort": 1521,
    "requireFile": false,
    "supportsSsl": true,
    "supportsSshTunnel": true
  },
  "sections": [
    {
      "key": "connection",
      "title": "连接设置",
      "icon": "database",
      "fields": [
        {
          "key": "host",
          "label": "主机",
          "type": "text",
          "placeholder": "localhost",
          "required": true,
          "inline": true,
          "flex": 2
        },
        {
          "key": "port",
          "label": "端口",
          "type": "number",
          "placeholder": "1521",
          "default": 1521,
          "required": true,
          "inline": true,
          "flex": 1
        },
        {
          "key": "serviceName",
          "label": "服务名",
          "type": "text",
          "placeholder": "ORCL",
          "required": true
        }
      ]
    },
    {
      "key": "authentication",
      "title": "认证",
      "icon": "user",
      "fields": [
        {
          "key": "username",
          "label": "用户名",
          "type": "text",
          "placeholder": "system",
          "required": true
        },
        {
          "key": "password",
          "label": "密码",
          "type": "password",
          "placeholder": "输入密码",
          "required": true
        }
      ]
    }
  ]
}
```

将文件放入 `schemas/` 目录后，页面会通过 `schema-loader.ts` 加载并渲染新的数据库类型配置表单。

## 多驱动架构（V3）

### 设计理念

同一种数据库可以有多种驱动实现，每种驱动提供不同的能力：

```
MySQL
  ├── sqlx (Rust, async)         ← 默认驱动，编译期 SQL 校验 + Arrow 零拷贝
  ├── diesel (Rust, ORM)         ← ORM 风格 + Schema DSL
  ├── Python (WASI, PyMySQL)     ← Pandas DataFrame 直写
  └── Go (WASI, go-sql-driver)   ← Go WASI 高性能查询引擎
```

### 驱动注册

后端通过 `DriverRegistry` + `DriverFactory::descriptor()` 注册驱动元数据，前端通过 `invoke('get_drivers')` 获取 `DriverDescriptor[]`：

- `fields: DriverField[]` — 驱动需要的表单字段（平铺列表）
- `extraOptions: DriverOption[]` — 驱动级别自定义选项
- `capabilities` — 驱动能力矩阵（Arrow、Streaming、事务等）

### 表单数据流

```
用户选择数据库类型（侧边栏）
  → 筛选可用驱动列表（DataSourceHeader 下拉）
  → 用户选择驱动
  → schema-loader 加载对应 JSON Schema
  → parseDriverSchema() 转换为 FormSectionConfig[]
  → GeneralTab 传入 DynamicFormRenderer
  → 动态渲染表单
```

> ⚠️ **当前状态**：`AddDataSourceDialog` 尚未传入 `formSections` 给 `GeneralTab`，常规 Tab 显示为空。需完成 schema-loader 与 AddDataSourceDialog 的对接。

## 布局设计

### V3 IDE 风格布局

参考 DataGrip + VSCode 的布局设计：

1. **左侧数据库类型树**：暂存列表 + 按类别分组显示所有支持的数据库类型
2. **顶部连接头**：名称、描述、URI 预览、驱动选择
3. **右侧标签页区域**：常规 / 网络 / 能力 / 属性 / 高级 五大标签页
4. **底部操作栏**：测试连接、保存、取消按钮

### 文件数据库特殊处理

- 文件数据库（SQLite、DuckDB）不显示主机端口连接配置，改为文件路径选择
- 文件数据库不显示 SSH/SSL 网络配置
- 文件数据库能力矩阵中网络相关能力自动隐藏

## 功能特性

### 1. 动态表单渲染

- 基于 JSON Schema 配置
- 支持字段依赖关系
- 支持表单验证
- 支持默认值填充

### 2. 连接作用域

- **全局连接**：所有项目可用
- **项目连接**：仅当前项目可用

### 3. DuckDB 本地加速

- 仅对网络数据库可用（MySQL、PostgreSQL 等）
- 支持配置缓存策略、性能设置、过期策略

### 4. 暂存列表

- 当前会话中未保存的数据源自动暂存
- 在侧边栏顶部显示，支持快速切换

### 5. 连接测试

- 点击"测试连接"按钮验证配置
- 显示测试结果（连通性、延迟、版本信息）

### 6. URI 实时预览

- 根据表单字段实时生成连接 URL
- 支持手动编辑 URI

## 类型定义

### FormFieldConfig

```typescript
export interface FormFieldConfig {
  key: string
  label: string
  type: 'text' | 'password' | 'number' | 'select' | 'checkbox' | 'file' | 'textarea'
  placeholder?: string
  required?: boolean
  default?: unknown
  options?: Array<{ label: string; value: string | number }>
  validation?: {
    pattern?: string
    min?: number
    max?: number
    minLength?: number
    maxLength?: number
    message?: string
  }
  tooltip?: string
  hidden?: boolean
  dependsOn?: {
    field: string
    value: unknown
  }
  inline?: boolean
  flex?: number
}
```

### FormSectionConfig

```typescript
export interface FormSectionConfig {
  key: string
  title: string
  icon?: string
  description?: string
  fields: FormFieldConfig[]
  collapsible?: boolean
  collapsed?: boolean
}
```

### DriverFormSchema

```typescript
export interface DriverFormSchema {
  driverId: string
  driverName: string
  version: string
  metadata: {
    category: string
    description: string
    features: string[]
    defaultPort: number | null
    requireFile: boolean
    supportsSsl: boolean
    supportsSshTunnel: boolean
  }
  sections: FormSectionConfig[]
}
```

### DriverDescriptor（来自后端）

```typescript
export interface DriverDescriptor {
  id: string
  name: string
  icon: string
  version?: string
  features: string[]
  category?: string
  defaultPort?: number
  description?: string
  driverKind?: string
  urlTemplate?: string
  fields?: DriverField[]
  extraOptions?: DriverOption[]
  requireFile?: boolean
  requireDatabase?: boolean
  supportsSsl?: boolean
  supportsSshTunnel?: boolean
  supportsHttpProxy?: boolean
  supportsSocksProxy?: boolean
}
```

## 注意事項

1. **文件数据库**：`requireFile: true` 时，不显示主机、端口、SSH、SSL 等配置
2. **多驱动选择**：选择数据库类型后，需在顶部下拉选择具体驱动，不同驱动的表单可能不同
3. **DuckDB 加速**：仅对网络数据库可用，文件数据库自动隐藏
4. **字段依赖**：使用 `dependsOn` 实现条件显示
5. **inline 布局**：使用 `inline: true` 和 `flex` 实现紧凑布局
6. **schema-loader 待接入**：当前 `AddDataSourceDialog` 尚未调用 `schema-loader` 加载 JSON Schema 传给 `GeneralTab`

## 扩展指南

### 添加新的数据库类型

1. 在 `schemas/` 目录创建 JSON 配置文件
2. 确保 `driverId` 与后端驱动标识一致
3. 配置 metadata 中的 `category` 和 `requireFile`
4. 定义 sections 和 fields

### 添加新的驱动（同一数据库）

1. 在后端实现 `DriverFactory` trait，注册到 `DriverRegistry`
2. `DriverDescriptor.fields` 定义该驱动的表单字段
3. 前端自动通过 `get_drivers` 获取新驱动信息

### 添加新的字段类型

1. 在 `FormFieldConfig` 类型中添加新类型
2. 在 `FieldRenderer.vue` 中实现渲染逻辑
3. 更新 CSS 样式

### 添加新的标签页

1. 在 `AddDataSourceDialog.vue` 的 `activeTab` 中添加新标签页
2. 创建对应的 tab 组件
3. 在标签页内容区域渲染新组件

## TODO / 未来规划

### 1. GeneralTab 动态表单接入

> 状态：⚠️ 待完成

当前 `AddDataSourceDialog` 尚未将 `formSections` 传给 `GeneralTab`，选择数据库类型后常规 Tab 显示为空。
需要完成 `schema-loader` → `parseDriverSchema()` → `FormSectionConfig[]` → `GeneralTab` 的数据链路。

### 2. 网络配置管理（Network Profile）

> 状态：📋 已设计，待实现
> 参考：DBeaver Network Profiles

将 SSH 隧道和代理配置从连接配置中解耦，实现"创建一次、多处复用"。

**核心设计**：

- `NetworkProfile`：命名的、可复用的网络配置档案（SSH / Proxy / SSH+Proxy 组合）
- 持久化跟随连接作用域（全局连接 → 全局 profile，项目连接 → 项目 profile，由 migrations 管理）
- 连接配置通过 `profileId` 引用 profile，不再内联 SSH/Proxy 字段

**新增模块**：

```
后端: src-tauri/src/core/network/{models,store,manager}.rs
前端: ui/components/network/{NetworkProfileSelector,NetworkProfileEditor,NetworkProfileManager}.vue
```

**数据流**：

```
创建数据源 → NetworkTab → NetworkProfileSelector
  ├── [选择已有 profile] → 一键应用 SSH/Proxy
  ├── [新建 profile] → NetworkProfileEditor → 保存到全局/项目 store
  └── [不启用] → 直连模式
```

### 3. V3 按钮对接状态

> 状态：🟢 基本完成（2026-05-18）

| 按钮/功能           | 状态 | 说明                                                  |
| ------------------- | :--: | ----------------------------------------------------- |
| 测试连接            |  ✅  | `invoke('test_connection', {dbType, url})`            |
| 保存                |  ✅  | `emit('save')` → DatabaseManager → `connect_database` |
| 常规 Tab 表单       |  ✅  | `loadDriverSchema()` → `formSections` → `GeneralTab`  |
| 文件选择            |  ✅  | `@tauri-apps/plugin-dialog` `open()`                  |
| 文件创建            |  ✅  | `save()` + `invoke('create_database_file')`           |
| NetworkTab 数据流   |  ✅  | `emit('update:config')` + `watch` deep                |
| URI 实时预览        |  ✅  | `connectionUrl` computed 值                           |
| 编辑 URI            |  ✅  | `editUriMode` toggle                                  |
| 暂存列表            |  ✅  | `useStagingList().selectEntry()`                      |
| 驱动加载            |  ✅  | `invoke('get_drivers')`                               |
| NetworkProfile 管理 |  📋  | 已设计，待后续实现                                    |

### 4. 类型定义去重

`DriverDescriptor` 在 `ui/types/connection.ts` 和 `ui/types/driver.ts` 中重复定义，需统一。`ConnectionConfig` 在 4 个文件中分别定义且字段不一致。建议以 `domain/types.ts` 为单一数据源。
