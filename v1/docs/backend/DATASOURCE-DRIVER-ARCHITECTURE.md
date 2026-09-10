# 数据源模块架构文档

> 版本：v1.0
> 更新日期：2026-05-26
> 状态：✅ 完整

---

## 概述

本文档描述 RdataStation 数据源模块的完整架构设计，包括驱动系统、认证管理、网络配置等核心功能。

---

## 一、驱动系统架构

### 1.1 设计理念

驱动的所有配置信息（支持的认证类型、网络协议能力、表单字段定义）都存储在数据库中，而非硬编码在前端或后端代码中。这使得：

- ✅ **新增数据库类型**：只需在数据库中插入一行数据，无需修改代码或发版
- ✅ **动态 UI 渲染**：前端根据数据库中的配置动态渲染表单
- ✅ **统一管理**：所有驱动配置集中管理，易于维护

### 1.2 核心表结构

#### `drivers` 表

```sql
CREATE TABLE IF NOT EXISTS drivers (
    id                    TEXT PRIMARY KEY,           -- 'mysql', 'postgres'
    type_id               TEXT NOT NULL,              -- 数据源类型 ID
    name                  TEXT NOT NULL,              -- 'MySQL', 'PostgreSQL'
    driver_kind           TEXT DEFAULT 'native',      -- 'native', 'jdbc', 'wasm'

    -- 连接参数
    default_port          INTEGER,                    -- 默认端口：3306, 5432
    url_template          TEXT,                       -- URL 模板：mysql://{host}:{port}/{database}

    -- 前端动态渲染
    config_schema         TEXT NOT NULL,              -- JSON Schema → 前端动态表单
    supported_auth_types  TEXT,                      -- 支持的认证类型：["password","ssl"]
    capabilities          TEXT,                      -- 驱动能力：["ssl","ssh_tunnel"]

    -- 文件型数据库
    is_file               BOOLEAN DEFAULT 0,          -- 是否文件型数据库（SQLite/DuckDB）

    -- 元数据
    enabled               BOOLEAN DEFAULT 1,
    created_at            TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
```

### 1.3 字段说明

| 字段                   | 类型    | 说明                 | 示例                               |
| ---------------------- | ------- | -------------------- | ---------------------------------- |
| `id`                   | TEXT    | 驱动唯一标识         | `mysql`, `postgres`                |
| `name`                 | TEXT    | 驱动显示名称         | `MySQL`, `PostgreSQL`              |
| `driver_kind`          | TEXT    | 驱动类型             | `native`, `jdbc`, `wasm`           |
| `is_file`              | BOOLEAN | 是否文件型数据库     | `true` for SQLite/DuckDB           |
| `default_port`         | INTEGER | 默认端口             | `3306`, `5432`                     |
| `url_template`         | TEXT    | URL 模板             | `mysql://{host}:{port}/{database}` |
| `config_schema`        | TEXT    | JSON Schema 表单定义 | 见下方详细说明                     |
| `supported_auth_types` | TEXT    | 支持的认证类型       | `["password","ssl"]`               |
| `capabilities`         | TEXT    | 驱动能力             | `["ssl","ssh_tunnel"]`             |

### 1.4 默认驱动数据

```sql
INSERT INTO drivers (id, type_id, name, driver_kind, default_port, url_template, config_schema, supported_auth_types, capabilities, is_file) VALUES
('mysql', 'mysql', 'MySQL', 'native', 3306, 'mysql://{host}:{port}/{database}', '{"type":"object","properties":{"host":{"type":"string","title":"主机"},"port":{"type":"number","title":"端口"},"database":{"type":"string","title":"数据库"}},"required":["host","port","database"]}', '["password","sha256_password","ssl"]', '["ssl","ssl_tls"]', false),

('postgres', 'postgres', 'PostgreSQL', 'native', 5432, 'postgresql://{host}:{port}/{database}', '{"type":"object","properties":{"host":{"type":"string","title":"主机"},"port":{"type":"number","title":"端口"},"database":{"type":"string","title":"数据库"}},"required":["host","port","database"]}', '["password","trust","ident","sspi","cert","scram-sha-256","kerberos"]', '["ssl","ssl_tls","ssh_tunnel"]', false),

('sqlite', 'sqlite', 'SQLite', 'native', 0, 'sqlite://{database}', '{"type":"object","properties":{"database":{"type":"string","title":"数据库文件"}},"required":["database"]}', '[]', '[]', true),

('duckdb', 'duckdb', 'DuckDB', 'native', 0, 'duckdb://{database}', '{"type":"object","properties":{"database":{"type":"string","title":"数据库文件"}},"required":["database"]}', '[]', '[]', true);
```

---

## 二、认证配置架构

### 2.1 设计理念

认证配置采用统一存储、分层管理的架构：

- ✅ **统一认证管理**：所有类型的认证（数据库认证、网络认证）统一存储在 `auth_configs` 表
- ✅ **通过 `auth_type` 区分**：不同的认证类型存储不同的字段
- ✅ **密码加密存储**：敏感信息使用 AES-256-GCM 加密

### 2.2 核心表结构

```sql
CREATE TABLE IF NOT EXISTS auth_configs (
    id          TEXT PRIMARY KEY,
    name        TEXT,
    auth_type   TEXT NOT NULL,        -- 认证类型
    auth_data   TEXT NOT NULL,        -- 加密后的认证数据
    created_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
```

### 2.3 认证类型分类

#### 数据库认证类型

| auth_type  | 存储字段                              | 说明                   |
| ---------- | ------------------------------------- | ---------------------- |
| `password` | username, password                    | 用户名/密码认证        |
| `ldap`     | username, password                    | LDAP/AD 认证           |
| `pg_class` | certPath, certKeyPath                 | PostgreSQL 客户端证书  |
| `kerberos` | principal, keytabPath                 | Kerberos 认证          |
| `oauth2`   | tokenEndpoint, clientId, clientSecret | OAuth 2.0              |
| `os_auth`  | -                                     | 操作系统认证（无凭据） |
| `trust`    | -                                     | 信任认证（无密码）     |

#### 网络认证类型

| auth_type         | 存储字段            | 说明         |
| ----------------- | ------------------- | ------------ |
| `ssh_password`    | username, password  | SSH 密码认证 |
| `ssh_private_key` | keyPath, passphrase | SSH 私钥认证 |
| `proxy_password`  | username, password  | 代理密码认证 |

### 2.4 数据流

```
连接配置 (connections 表)
    │
    ├── auth_config_id ──────────→ auth_configs 表 (数据库认证)
    │                                   └── auth_type = 'password'
    │                                       └── auth_data = { username, password }
    │
    └── network_config_id ───────→ network_configs 表 (网络配置)
                                        │
                                        └── auth_config_id ──→ auth_configs 表 (网络认证)
                                                                    └── auth_type = 'ssh_password'
                                                                        └── auth_data = { username, password }
```

---

## 三、网络配置架构

### 3.1 核心表结构

```sql
CREATE TABLE IF NOT EXISTS network_configs (
    id              TEXT PRIMARY KEY,
    name            TEXT,
    network_type    TEXT NOT NULL,     -- 'ssh', 'ssl', 'proxy', 'chain'
    config          TEXT NOT NULL,      -- JSON 配置
    auth_config_id  TEXT,              -- 可选：关联的认证配置
    created_at      TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at      TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
```

### 3.2 网络类型

| network_type | 说明         | 协议链位置     |
| ------------ | ------------ | -------------- |
| `ssh`        | SSH 隧道     | 中间（可多跳） |
| `ssl`        | SSL/TLS 加密 | 末尾（固定）   |
| `proxy`      | 代理服务器   | 中间（可多跳） |
| `chain`      | 协议链       | 完整链         |

### 3.3 协议链设计

```
协议链结构：
┌─────────────────────────────────────────────────────────────┐
│                    SSH ─→ Proxy ─→ SSL                      │
│                     │        │        │                     │
│                   跳板机    代理    流加密（末尾）          │
└─────────────────────────────────────────────────────────────┘

最大跳数：4 跳（不含 SSL）
警告阈值：≥3 跳显示延迟警告
SSL 固定在末尾（流加密包装器）
```

---

## 四、前端动态渲染

### 4.1 基于 `capabilities` 的 UI 控制

前端根据驱动的 `capabilities` 字段动态显示/隐藏网络配置选项：

```typescript
// NetworkTab.vue
const supportsSsl = computed(() => {
  const caps = parseDriverCapabilities(props.driver?.capabilities)
  return caps.includes('ssl') || caps.includes('ssl_tls')
})

const supportsSsh = computed(() => {
  const caps = parseDriverCapabilities(props.driver?.capabilities)
  return caps.includes('ssh_tunnel') || caps.includes('ssh')
})
```

### 4.2 基于 `supported_auth_types` 的认证过滤

前端根据驱动的 `supported_auth_types` 字段过滤认证选项：

```typescript
// useAuthConfig.ts
const filteredAuthConfigs = computed(() => {
  if (!props.driver?.supported_auth_types) return authConfigs.value
  const supportedTypes = parseSupportedAuthTypes(props.driver.supported_auth_types)
  return authConfigs.value.filter(cfg => supportedTypes.includes(cfg.auth_type))
})
```

### 4.3 基于 `config_schema` 的动态表单

前端根据驱动的 `config_schema` 字段动态渲染高级连接参数：

```typescript
// schema-parser.ts
export function parseSchemaToFormFields(schema: string): FormFieldConfig[] {
  const parsed = JSON.parse(schema)
  // 解析 JSON Schema 为表单字段配置
  return fields
}
```

---

## 五、新增数据库类型

### 5.1 操作步骤

新增数据库类型只需在数据库中插入数据，**无需修改代码或发版**：

```sql
-- 1. 添加驱动配置
INSERT INTO drivers (id, type_id, name, driver_kind, default_port, url_template, config_schema, supported_auth_types, capabilities, is_file)
VALUES (
  'oceanbase',
  'oceanbase',
  'OceanBase',
  'native',
  2883,
  'oceanbase://{host}:{port}/{database}',
  '{"type":"object","properties":{"host":{"type":"string"},"port":{"type":"number"},"database":{"type":"string"}}}',
  '["password","sha256_password"]',
  '["ssl"]',
  false
);

-- 2. 添加数据源类型（可选）
INSERT INTO data_source_types (id, name, category, icon, enabled)
VALUES ('oceanbase', 'OceanBase', '关系型数据库', '🗄️', true);
```

### 5.2 配置说明

#### `config_schema` 字段

JSON Schema 格式，定义驱动的高级连接参数：

```json
{
  "type": "object",
  "properties": {
    "timeout": {
      "type": "number",
      "title": "连接超时",
      "description": "连接超时时间（秒）",
      "default": 30
    },
    "charset": {
      "type": "string",
      "title": "字符集",
      "enum": ["utf8mb4", "latin1", "gbk"]
    }
  }
}
```

#### `supported_auth_types` 字段

JSON 数组格式，定义驱动支持的认证类型：

```json
["password", "sha256_password", "ssl", "kerberos"]
```

#### `capabilities` 字段

JSON 数组格式，定义驱动的网络能力：

```json
["ssl", "ssl_tls", "ssh_tunnel", "proxy"]
```

---

## 六、数据流总结

### 6.1 完整数据流

```
用户选择驱动
    ↓
前端加载驱动配置
    ├─ config_schema → 渲染高级参数表单
    ├─ supported_auth_types → 过滤认证选项
    └─ capabilities → 控制网络配置 UI
    ↓
用户填写连接信息
    ↓
用户选择/创建认证配置
    ├─ auth_type = 'password' → 保存到 auth_configs 表
    └─ auth_config_id → 关联到 connections 表
    ↓
用户选择/创建网络配置
    ├─ network_type = 'ssh' → 保存到 network_configs 表
    ├─ auth_config_id → 可选：关联 SSH 认证配置
    └─ network_config_id → 关联到 connections 表
    ↓
用户测试连接
    ↓
后端解析网络配置 → 建立 SSH 隧道/代理
    ↓
后端读取认证配置 → 解密并注入 URL
    ↓
建立数据库连接
```

### 6.2 关键设计原则

1. ✅ **数据库驱动**：所有配置存储在数据库，便于动态扩展
2. ✅ **统一认证管理**：通过 `auth_type` 区分数据库认证和网络认证
3. ✅ **冗余设计**：`network_configs.config` 保留完整配置，同时支持关联 `auth_config_id`
4. ✅ **加密存储**：敏感信息使用 AES-256-GCM 加密
5. ✅ **动态 UI**：前端根据数据库配置动态渲染表单

---

## 七、文件结构

```
src/
├── extensions/builtin/connection/
│   ├── domain/
│   │   └── types.ts                 # 驱动相关类型定义
│   ├── ui/
│   │   ├── components/
│   │   │   ├── tabs/
│   │   │   │   ├── GeneralTab.vue   # 基本信息 Tab（使用 capabilities）
│   │   │   │   └── NetworkTab.vue   # 网络配置 Tab（使用 capabilities）
│   │   │   └── ...
│   │   ├── composables/
│   │   │   ├── useDriverRegistry.ts # 驱动注册管理
│   │   │   └── useAuthConfig.ts     # 认证配置管理
│   │   ├── utils/
│   │   │   └── schema-parser.ts     # JSON Schema 解析器
│   │   └── services/
│   │       └── driver.ts            # 驱动服务调用
│   └── extension.ts                 # 扩展入口

src-tauri/
├── migrations/global/
│   ├── 008_add_data_source_module.sql  # 驱动、认证、网络配置表
│   └── 012_add_network_auth_config_id.sql  # 添加 auth_config_id 字段
├── src/
│   ├── core/
│   │   ├── persistence/
│   │   │   ├── driver_store.rs     # 驱动存储
│   │   │   └── auth_store.rs       # 认证存储
│   │   └── services/
│   │       └── connection_service.rs # 连接服务（含网络配置解析）
│   └── commands/
│       └── connection_commands.rs   # 连接命令
└── Cargo.toml                      # 依赖配置
```

---

## 八、版本历史

| 版本 | 日期       | 说明                       |
| ---- | ---------- | -------------------------- |
| v1.0 | 2026-05-26 | 初始版本，包含完整架构设计 |
