# 新增数据源模块：后端架构 · 设计 · 开发 · 接口文档

> 版本：v2.2
> 初稿日期：2026-05-18
> 更新时间：2026-05-27（v0.5.3 驱动 ID 对齐 Registry：mysql-native→mysql 等，新增 014/015 修复迁移，seed 6 驱动）
> 对应后端版本：R25+
> 实现状态：✅ 后端 v1.6 已完成 core store + migrations；v2.0 中 ID前缀约定 + 快照溯源 + 环境 Seed 已实现；快照 store + 链校验待开发

---

## 概述

新增数据源模块为 RdataStation 提供可插拔的数据库驱动管理体系，支持：

- **驱动定义**：全局驱动目录（`global.db.drivers`）
- **驱动安装**：按需下载外部驱动文件（JDBC/WASM/Python）
- **项目隔离**：每个项目独立声明使用哪些驱动（`project_drivers` 表）
- **跨机器迁移**：项目迁移后自动检测缺失驱动，引导安装
- **环境与策略**：生产/预发布/开发环境 + 安全策略
- **可复用配置**：认证配置、网络配置独立管理

---

## 一、核心设计原则

### 1.1 驱动可见性：严格项目隔离

```
┌──────────────────────────────────────────────────────────────┐
│  铁律：项目 B 没有安装 Oracle 驱动 = 项目 B 看不到 Oracle     │
│                                                              │
│  实现方式：project_drivers 表（在项目 meta.db 中）             │
│  - 新项目创建时，只自动启用 4 个内置 Native 驱动               │
│  - Oracle JDBC 需要用户手动 "安装到项目" 才会出现条目          │
│  - 没有任何 project_drivers 条目 = 前端驱动列表不显示          │
└──────────────────────────────────────────────────────────────┘
```

### 1.2 三层驱动生命周期

```
 ┌──────────┐      ┌──────────┐      ┌──────────┐
 │ 1. 已定义 │ ──→  │ 2. 已安装 │ ──→  │ 3. 已启用 │ ──→ 可创建连接
 │ (Defined)│      │(Installed)│     │ (Enabled) │
 └──────────┘      └──────────┘      └──────────┘
 global.db         global.db         project
 .drivers          .driver_files     meta.db
                   + 文件系统        .project_drivers

 例：Oracle JDBC
 Step 1: drivers 表有 oracle-jdbc 定义（全局目录）            → 用户可以从 "驱动市场" 看到
 Step 2: 下载 ojdbc8.jar 到本机，注册到 driver_files         → 本机已安装
 Step 3: 项目 A 在 project_drivers 中启用 oracle-jdbc        → 项目 A 可以创建 Oracle 连接
         项目 B 的 project_drivers 中没有 oracle-jdbc        → 项目 B 看不到 Oracle 🔒
```

### 1.3 Native vs External 驱动

| DriverKind | 驱动文件来源                              | 需要 driver_files? | 需要下载? | 迁移行为                  |
| ---------- | ----------------------------------------- | ------------------ | --------- | ------------------------- |
| `Native`   | Rust 二进制内置 (sqlx/rusqlite/duckdb-rs) | ❌ 不需要          | ❌ 不需要 | 透传，无额外操作          |
| `Jdbc`     | .jar 文件                                 | ✅ 需要            | ✅ 需要   | 检查文件 → 缺失时引导下载 |
| `Odbc`     | 系统 ODBC 驱动                            | ✅ 需要            | ✅ 需要   | 检查系统驱动              |
| `Wasm`     | .wasm 文件                                | ✅ 需要            | ✅ 需要   | 检查文件 → 缺失时引导下载 |
| `Python`   | .py 脚本                                  | ✅ 需要            | ✅ 需要   | 检查文件 → 缺失时引导下载 |

---

## 二、场景验证

### 场景 A：项目级驱动隔离（正常运行）

```
前提：全球驱动目录 global.db.drivers 中有 oracle-jdbc 定义

机器 M 上：

项目 A（已安装 Oracle JDBC）              项目 B（从未安装 Oracle）
  .RSmeta/meta.db                           .RSmeta/meta.db
    project_drivers:                          project_drivers:
      ✅ mysql                                ✅ mysql
      ✅ mysql_native                         ✅ mysql_native
      ✅ postgres                             ✅ postgres
      ✅ postgres_native                      ✅ postgres_native
      ✅ sqlite                               ✅ sqlite
      ✅ duckdb                               ✅ duckdb
      ✅ oracle-jdbc     ← 用户主动安装的      (无 oracle-jdbc)  ← 就这么干净

结果：
  项目 A 前端驱动列表：MySQL (sqlx), MySQL (Official), PostgreSQL (sqlx), PostgreSQL (Official), SQLite, DuckDB, Oracle  ← 全部7个
  项目 B 前端驱动列表：MySQL (sqlx), MySQL (Official), PostgreSQL (sqlx), PostgreSQL (Official), SQLite, DuckDB          ← 只有6个
  项目 B 无法创建 Oracle 连接 🔒
```

### 场景 B：项目迁移到无驱动的新机器

```
机器 X（开发机）                              机器 Y（新电脑）
─────────────────                             ─────────────────
global.db:                                    global.db:
  driver_files: oracle-jdbc ✅                  driver_files: (空，新装应用)

项目 A meta.db → ──────── 复制项目目录 ────────→ 项目 A meta.db
  project_drivers:                                project_drivers:
    oracle-jdbc ✅                                  oracle-jdbc ✅ (已迁移)

迁移后打开项目 A：
  ┌──────────────────────────────────────────────────┐
  │  项目 A 驱动自检结果：                             │
  │                                                  │
  │  MySQL      ✅ 可用 (Native，Rust 二进制内置)      │
  │  PostgreSQL ✅ 可用 (Native，Rust 二进制内置)      │
  │  SQLite     ✅ 可用 (Native，Rust 二进制内置)      │
  │  DuckDB     ✅ 可用 (Native，Rust 二进制内置)      │
  │  Oracle     ⚠️ 驱动文件未安装                      │
  │                 [安装驱动]  [暂时禁用]              │
  │                                                  │
  │  点击 [暂时禁用] → Oracle 从当前会话隐藏            │
  │  点击 [安装驱动] → 下载 ojdbc8.jar → 自动可用      │
  └──────────────────────────────────────────────────┘
```

### 场景 C：新项目默认驱动集

```
用户创建 "项目 C"
    ↓
自动在项目 C 的 project_drivers 中种子 6 个内置驱动（v0.5.3+）：
    mysql              ✅  (sqlx)
    mysql_native       ✅  (mysql_async)
    postgres           ✅  (sqlx)
    postgres_native    ✅  (tokio-postgres)
    sqlite             ✅  (rusqlite)
    duckdb             ✅  (duckdb-rs)
    (仅此而已，无其他驱动)

项目 C 前端驱动列表：MySQL (sqlx), MySQL (Official), PostgreSQL (sqlx), PostgreSQL (Official), SQLite, DuckDB
```

---

## 三、数据库表设计

### 3.1 表分布

```
 global.db（系统级）                    project meta.db（项目级）
 ─────────────────                     ──────────────────────
 data_source_types  ← 类型目录
 drivers            ← 驱动定义目录      project_drivers  ← 项目驱动启用表 ⭐
 driver_files       ← 本机文件注册      environments     ← 项目环境
 environments       ← 全局环境          env_policies     ← 项目策略
 env_policies       ← 全局策略          auth_configs     ← 项目认证
 auth_configs       ← 全局认证          network_configs  ← 项目网络
 network_configs    ← 全局网络
 global_connections ← 全局连接 (扩展)   connections      ← 项目连接 (扩展)
```

### 3.2 全局表 DDL

> 迁移文件：`src-tauri/migrations/global/008_add_data_source_module.sql`

```sql
-- ============================================================================
-- 1. 数据源类型目录（MySQL / PostgreSQL / Oracle / MongoDB / Kafka ...）
-- ============================================================================
CREATE TABLE IF NOT EXISTS data_source_types (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    category    TEXT NOT NULL,       -- relational | file-based | nosql | analytics | cloud | mq | http
    icon        TEXT,
    enabled     BOOLEAN DEFAULT 1,
    created_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- ============================================================================
-- 2. 驱动定义目录（全局注册表，所有项目共享）
--    描述驱动的元数据、连接参数 Schema、下载地址
-- ============================================================================
DROP TABLE IF EXISTS global_drivers;

CREATE TABLE IF NOT EXISTS drivers (
    id                    TEXT PRIMARY KEY,
    type_id               TEXT NOT NULL REFERENCES data_source_types(id) ON DELETE CASCADE,
    name                  TEXT NOT NULL,
    driver_kind           TEXT NOT NULL DEFAULT 'native', -- native|jdbc|odbc|wasm|adbc|http|python|js
    is_file               BOOLEAN DEFAULT 0,
    default_port          INTEGER,
    url_template          TEXT,
    download_url          TEXT,           -- ⭐ 外部驱动下载 URL
    download_checksum     TEXT,           -- ⭐ SHA256 校验和
    version               TEXT,           -- ⭐ 驱动版本号
    config_schema         TEXT NOT NULL,  -- JSON Schema：前端动态生成连接表单
    supported_auth_types  TEXT,           -- JSON 数组：支持的认证方式
    capabilities          TEXT,           -- JSON 数组：支持的能力
    enabled               BOOLEAN DEFAULT 1,
    created_at            TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_drivers_type ON drivers(type_id);
CREATE INDEX IF NOT EXISTS idx_drivers_kind ON drivers(driver_kind);
CREATE INDEX IF NOT EXISTS idx_drivers_enabled ON drivers(enabled);

-- ============================================================================
-- 3. 本机驱动文件注册表（记录本机已下载的外部驱动文件）
--    Native 驱动不需要此行
-- ============================================================================
CREATE TABLE IF NOT EXISTS driver_files (
    id           TEXT PRIMARY KEY,
    driver_id    TEXT NOT NULL REFERENCES drivers(id),
    file_path    TEXT NOT NULL,           -- 相对路径：{app_data}/RdataStation/drivers/{driver_id}/{version}/
    file_name    TEXT NOT NULL,
    file_size    INTEGER,
    checksum     TEXT,
    version      TEXT NOT NULL,
    installed_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at   TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_driver_files_driver ON driver_files(driver_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_driver_files_unique ON driver_files(driver_id, version);

-- ============================================================================
-- 4. 环境表
-- ============================================================================
CREATE TABLE IF NOT EXISTS environments (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    description TEXT,
    color       TEXT,
    sort_order  INTEGER DEFAULT 0,
    created_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- ============================================================================
-- 5. 环境策略表
-- ============================================================================
CREATE TABLE IF NOT EXISTS environment_policies (
    id              TEXT PRIMARY KEY,
    environment_id  TEXT NOT NULL REFERENCES environments(id) ON DELETE CASCADE,
    policy_type     TEXT NOT NULL,       -- read_only|navigation_filter|query_timeout|max_rows|ddl_blocked|dml_blocked|confirm_required
    policy_config   TEXT,
    enabled         BOOLEAN DEFAULT 1,
    created_at      TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_env_policies_env ON environment_policies(environment_id);

-- ============================================================================
-- 6. 认证配置表（密码 AES-256-GCM 加密存储）
-- ============================================================================
CREATE TABLE IF NOT EXISTS auth_configs (
    id          TEXT PRIMARY KEY,
    name        TEXT,
    auth_type   TEXT NOT NULL,           -- password|keyfile|kerberos|oauth2|aws_iam|gcp_sa
    auth_data   TEXT NOT NULL,           -- JSON，密码字段已加密
    created_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- ============================================================================
-- 7. 网络配置表（SSH 隧道 / 代理 / SSL）
-- ============================================================================
CREATE TABLE IF NOT EXISTS network_configs (
    id            TEXT PRIMARY KEY,
    name          TEXT,
    network_type  TEXT NOT NULL,         -- ssh|proxy|ssl|none
    config        TEXT NOT NULL,         -- JSON，具体结构依赖 network_type
    created_at    TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at    TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- ============================================================================
-- 8. 扩展 global_connections 表（ALTER TABLE，不丢失数据）
-- ============================================================================
ALTER TABLE global_connections ADD COLUMN description TEXT;
ALTER TABLE global_connections ADD COLUMN driver_id TEXT;
ALTER TABLE global_connections ADD COLUMN environment_id TEXT;
ALTER TABLE global_connections ADD COLUMN auth_config_id TEXT;
ALTER TABLE global_connections ADD COLUMN auth_method TEXT;
ALTER TABLE global_connections ADD COLUMN network_config_id TEXT;
ALTER TABLE global_connections ADD COLUMN driver_properties TEXT;
ALTER TABLE global_connections ADD COLUMN advanced_options TEXT;

UPDATE global_connections SET driver_id = driver WHERE driver_id IS NULL;

CREATE INDEX IF NOT EXISTS idx_gc_driver_id ON global_connections(driver_id);
CREATE INDEX IF NOT EXISTS idx_gc_env ON global_connections(environment_id);

-- ============================================================================
-- 9. 种子数据
-- ============================================================================
INSERT OR IGNORE INTO data_source_types (id, name, category, icon, enabled) VALUES
    ('mysql',      'MySQL',           'relational', '🐬', 1),
    ('postgresql', 'PostgreSQL',      'relational', '🐘', 1),
    ('sqlite',     'SQLite',          'file-based', '🪶', 1),
    ('duckdb',     'DuckDB',          'analytics',  '🦆', 1),
    ('mariadb',    'MariaDB',         'relational', '🦭', 1),
    ('oracle',     'Oracle',          'relational', '🔴', 1),
    ('mssql',      'SQL Server',      'relational', '🟢', 1),
    ('clickhouse', 'ClickHouse',      'analytics',  '🔵', 1),
    ('mongodb',    'MongoDB',         'nosql',      '🍃', 1),
    ('redis',      'Redis',           'nosql',      '🔶', 1);

-- 4 个内置 Native 驱动（driver_files 不需要，编译在 Rust 二进制中）
-- 驱动 ID 与 Runtime DriverRegistry key 严格对齐
INSERT OR IGNORE INTO drivers (id, type_id, name, driver_kind, is_file, default_port,
    url_template, download_url, version, config_schema, supported_auth_types, capabilities, enabled) VALUES
('mysql', 'mysql', 'MySQL (sqlx)', 'native', 0, 3306,
 'mysql://{username}:{password}@{host}:{port}/{database}',
 NULL, 'builtin',
 '{"type":"object","properties":{"host":{"type":"string","title":"主机","default":"localhost"},"port":{"type":"integer","title":"端口","default":3306},"database":{"type":"string","title":"数据库名"},"username":{"type":"string","title":"用户名"},"password":{"type":"string","title":"密码","format":"password"}},"required":["host","port","username"]}',
 '["password","ssl"]',
 '["tree","health_check","transactions","prepared_stmts"]',
 1),

('postgres', 'postgresql', 'PostgreSQL (sqlx)', 'native', 0, 5432,
 'postgres://{username}:{password}@{host}:{port}/{database}',
 NULL, 'builtin',
 '{"type":"object","properties":{"host":{"type":"string","title":"主机","default":"localhost"},"port":{"type":"integer","title":"端口","default":5432},"database":{"type":"string","title":"数据库名"},"username":{"type":"string","title":"用户名"},"password":{"type":"string","title":"密码","format":"password"}},"required":["host","port","database","username"]}',
 '["password","ssl","kerberos"]',
 '["tree","health_check","schema_filter","transactions","prepared_stmts"]',
 1),

('sqlite', 'sqlite', 'SQLite (rusqlite)', 'native', 1, NULL,
 'sqlite://{file_path}',
 NULL, 'builtin',
 '{"type":"object","properties":{"file_path":{"type":"string","title":"数据库文件路径","format":"file"}},"required":["file_path"]}',
 '["none"]',
 '["tree","transactions","in_memory"]',
 1),

('duckdb', 'duckdb', 'DuckDB (duckdb-rs)', 'native', 1, NULL,
 'duckdb://{file_path}',
 NULL, 'builtin',
 '{"type":"object","properties":{"file_path":{"type":"string","title":"数据库文件路径","format":"file"},"memory_limit":{"type":"string","title":"内存限制"}},"required":["file_path"]}',
 '["none"]',
 '["tree","health_check","arrow","federated","transactions"]',
 1);
```

### 3.3 项目级表 DDL

> 迁移文件：`src-tauri/migrations/project_meta/010_add_data_source_module.sql`

```sql
-- ============================================================================
-- 1. 项目驱动启用表（⭐ 核心隔离表）
--    决定当前项目能看到哪些驱动
--    新项目只自动种子4个 Native 驱动
--    没有记录 = 前端不显示
-- ============================================================================
CREATE TABLE IF NOT EXISTS project_drivers (
    id           TEXT PRIMARY KEY,
    driver_id    TEXT NOT NULL,              -- 逻辑引用 global.db.drivers.id (跨DB，Rust校验)
    enabled      BOOLEAN DEFAULT 1,
    installed_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_project_drivers_unique ON project_drivers(driver_id);
CREATE INDEX IF NOT EXISTS idx_project_drivers_enabled ON project_drivers(enabled);

-- ============================================================================
-- 2. 环境表（项目独立）
-- ============================================================================
CREATE TABLE IF NOT EXISTS environments (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    description TEXT,
    color       TEXT,
    sort_order  INTEGER DEFAULT 0,
    created_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- ============================================================================
-- 3. 环境策略表（项目独立）
-- ============================================================================
CREATE TABLE IF NOT EXISTS environment_policies (
    id              TEXT PRIMARY KEY,
    environment_id  TEXT NOT NULL REFERENCES environments(id) ON DELETE CASCADE,
    policy_type     TEXT NOT NULL,
    policy_config   TEXT,
    enabled         BOOLEAN DEFAULT 1,
    created_at      TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_env_policies_env ON environment_policies(environment_id);

-- ============================================================================
-- 4. 认证配置表（项目独立）
-- ============================================================================
CREATE TABLE IF NOT EXISTS auth_configs (
    id          TEXT PRIMARY KEY,
    name        TEXT,
    auth_type   TEXT NOT NULL,
    auth_data   TEXT NOT NULL,
    created_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- ============================================================================
-- 5. 网络配置表（项目独立）
-- ============================================================================
CREATE TABLE IF NOT EXISTS network_configs (
    id            TEXT PRIMARY KEY,
    name          TEXT,
    network_type  TEXT NOT NULL,
    config        TEXT NOT NULL,
    created_at    TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at    TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- ============================================================================
-- 6. 扩展 connections 表（ALTER TABLE，不丢失数据）
-- ============================================================================
ALTER TABLE connections ADD COLUMN description TEXT;
ALTER TABLE connections ADD COLUMN driver_id TEXT;
ALTER TABLE connections ADD COLUMN environment_id TEXT;
ALTER TABLE connections ADD COLUMN auth_config_id TEXT;
ALTER TABLE connections ADD COLUMN auth_method TEXT;
ALTER TABLE connections ADD COLUMN network_config_id TEXT;
ALTER TABLE connections ADD COLUMN driver_properties TEXT;
ALTER TABLE connections ADD COLUMN advanced_options TEXT;

UPDATE connections SET driver_id = driver WHERE driver_id IS NULL;

CREATE INDEX IF NOT EXISTS idx_conn_driver_id ON connections(driver_id);
CREATE INDEX IF NOT EXISTS idx_conn_env ON connections(environment_id);
```

### 3.4 表关系全景

```
 global.db                                       project meta.db
 ─────────                                       ───────────────

 data_source_types (1)
       │
       │ 1:N (global FK)
       ▼
 drivers (N)  ←──────── driver_id (逻辑引用) ──────────┐
       │                                                │
       │ 1:N (global FK)                               │
       ▼                                                │
 driver_files (0..N)                                   │
                                                        │
 environments ────── 独立副本 ────── environments       │
       │                                    │           │
       │ 1:N (同DB FK)                     │ 1:N       │
       ▼                                    ▼           │
 environment_policies              environment_policies │
                                                        │
 auth_configs ──────── 独立副本 ────── auth_configs     │
 network_configs ───── 独立副本 ────── network_configs  │
                                                        │
 global_connections                connections          │
  ├── FK → environments  (global)   ├── FK → environments  (project)  │
  ├── FK → auth_configs  (global)   ├── FK → auth_configs  (project)  │
  ├── FK → network_configs(global)  ├── FK → network_configs(project) │
  └── driver_id (逻辑→drivers)      └── driver_id (逻辑→drivers)      │
                                                        │
                                   project_drivers ⭐    │
                                    └── driver_id (逻辑→drivers)       │
```

### 3.5 ID 前缀约定与快照溯源机制 ⭐ v2.0 新增

> 迁移文件：`src-tauri/migrations/project_meta/011_add_id_prefix_snapshot.sql`

#### 3.5.1 ID 前缀规则

```
G_xxx  = 全局表主键，存储于 global.db
P_xxx  = 项目表主键（本地创建），存储于 project.db
GP_xxx = 从全局快照到项目的数据，存储于 project.db
```

| 前缀     | 存储位置   | 创建方式            | 生命周期                   |
| -------- | ---------- | ------------------- | -------------------------- |
| `G_xxx`  | global.db  | 用户在应用级创建    | 全局共享，所有项目可见     |
| `P_xxx`  | project.db | 用户在项目内创建    | 仅该项目可见               |
| `GP_xxx` | project.db | 从 `G_xxx` 快照而来 | 项目独立副本，可随项目迁移 |

#### 3.5.2 快照溯源字段

以下三张 project.db 表通过 migration 011 新增三个字段：

```sql
ALTER TABLE environments    ADD COLUMN origin      TEXT DEFAULT 'project';
ALTER TABLE environments    ADD COLUMN source_id   TEXT;
ALTER TABLE environments    ADD COLUMN snapshot_at TIMESTAMP;

ALTER TABLE network_configs ADD COLUMN origin      TEXT DEFAULT 'project';
ALTER TABLE network_configs ADD COLUMN source_id   TEXT;
ALTER TABLE network_configs ADD COLUMN snapshot_at TIMESTAMP;

ALTER TABLE auth_configs    ADD COLUMN origin      TEXT DEFAULT 'project';
ALTER TABLE auth_configs    ADD COLUMN source_id   TEXT;
ALTER TABLE auth_configs    ADD COLUMN snapshot_at TIMESTAMP;
```

| 字段          | 类型      | 说明                                                     |
| ------------- | --------- | -------------------------------------------------------- |
| `origin`      | TEXT      | `'project'` = 本地创建；`'global_snapshot'` = 从全局快照 |
| `source_id`   | TEXT      | 快照来源的全局 `G_xxx` ID（origin='project' 时为 NULL）  |
| `snapshot_at` | TIMESTAMP | 快照创建时间戳                                           |

#### 3.5.3 项目引用全局配置的完整链路

```
用户在项目 A 中选择了一个全局环境 "生产环境 (G_env_001)"

    ┌──────────────────────────────────────────────────────────┐
    │ Step 1: 检查 project.db 是否已有该环境的快照               │
    │   SELECT * FROM environments                              │
    │   WHERE source_id = 'G_env_001'                           │
    │                                                          │
    │   有 → 直接使用 GP_xxx ID                                 │
    │   无 → Step 2                                             │
    ├──────────────────────────────────────────────────────────┤
    │ Step 2: 从 global.db 读取完整环境 + 策略数据               │
    │   G_env_001 → { name, color, policies: [...] }            │
    ├──────────────────────────────────────────────────────────┤
    │ Step 3: 写入 project.db（生成 GP_xxx ID）                  │
    │   INSERT INTO environments (id, name, color,              │
    │       origin, source_id, snapshot_at)                     │
    │   VALUES ('GP_env_001', '生产环境', '#F5222D',            │
    │       'global_snapshot', 'G_env_001', datetime('now'))    │
    │                                                          │
    │   INSERT INTO environment_policies ... (关联 GP_env_001)   │
    ├──────────────────────────────────────────────────────────┤
    │ Step 4: 连接引用快照 ID                                    │
    │   INSERT INTO connections (..., environment_id)            │
    │   VALUES (..., 'GP_env_001')                              │
    └──────────────────────────────────────────────────────────┘
```

#### 3.5.4 前端识别来源的方式

前端获取项目连接时，通过 `environment_id` 前缀即可判断来源：

```typescript
function getEnvironmentSource(envId: string): 'global' | 'project' {
  if (envId.startsWith('GP_')) return 'global' // 快照自全局 → 显示 🌐 全局
  if (envId.startsWith('P_')) return 'project' // 本地创建 → 显示 📁 项目
  if (envId.startsWith('G_')) return 'global' // 纯全局引用（global_connections 表）
  return 'project'
}
```

#### 3.5.5 作用域可见性规则

```
                      ┌──────────────────────────────┐
                      │       应用级（Global）          │
                      │    G_env_xxx  /  G_net_xxx     │
                      │    G_auth_xxx                  │
                      └──────────┬───────────────────┘
                                 │
                    ✅ 项目可引用  │  ❌ 应用不可引用
                    （触发快照）   │  （不能向下看项目私有）
                                 │
              ┌──────────────────┴───────────────────┐
              │                                      │
     ┌────────┴────────┐                    ┌────────┴────────┐
     │    项目 A 连接    │                    │    项目 B 连接    │
     │  P_env_xxx       │                    │  P_env_yyy       │
     │  GP_env_001 ←快照 │                    │  P_auth_zzz      │
     │  P_net_xxx       │                    │                   │
     └─────────────────┘                    └─────────────────┘
```

**规则**：

- 项目连接可引用 `P_xxx`（本地创建）+ `GP_xxx`（全局快照）
- 应用连接只能引用 `G_xxx`（不能访问项目私有 `P_xxx`）
- 快照在**用户首次引用**时自动触发，项目迁移后快照本地可用无需全局依赖

---

## 四、Rust 模块设计

### 4.1 文件变更清单

```
v1.6 已完成文件：
  新增文件：
    src-tauri/src/core/persistence/driver_store.rs      — drivers + driver_files CRUD (仅 global)
    src-tauri/src/core/persistence/env_store.rs         — environments + policies CRUD (纯函数)
    src-tauri/src/core/persistence/auth_store.rs        — auth_configs CRUD (纯函数)
    src-tauri/src/core/persistence/network_store.rs     — network_configs CRUD (纯函数)
    src-tauri/src/core/services/driver_service.rs       — 驱动发现/安装/验证
    src-tauri/src/commands/data_source_commands.rs      — 前端 API 命令

  修改文件：
    src-tauri/src/core/driver/registry/descriptors.rs   — DriverDescriptor +7 字段
    src-tauri/src/core/persistence/global_db.rs         — 集成新表 CRUD；GlobalConnectionInfo +8 字段（含 auth_method）
    src-tauri/src/core/persistence/project_connection_store.rs — ProjectConnection +8 字段（含 auth_method）
    src-tauri/src/core/persistence/project_db.rs        — 集成 project_drivers CRUD
    src-tauri/src/core/persistence/mod.rs               — 注册新子模块
    src-tauri/src/core/services/connection_service.rs   — 连接创建时驱动校验 + auth_method 透传
    src-tauri/src/core/commands/connection_commands.rs  — 新字段传递（含 auth_method）
    src-tauri/src/lib.rs                                — 注册新 commands

v2.0 已完成文件（本次开发已实现）：
  src-tauri/migrations/global/009_add_id_prefix_convention.sql — ID 前缀规范 + 5 环境 + 25 策略 Seed
  src-tauri/migrations/project_meta/011_add_id_prefix_snapshot.sql — origin/source_id/snapshot_at 快照溯源字段

v2.0 待新增文件（本次开发计划）：
  src-tauri/src/core/persistence/id_prefix.rs           — ID 前缀生成器 (generate_gid/generate_pid/generate_gpid)
  src-tauri/src/core/persistence/snapshot_store.rs      — 快照 store (snapshot_environment/network_config/auth_config)
  src-tauri/src/core/driver/connection/chain_validator.rs — 协议链校验 (validate_protocol_chain)

v2.0 已新增迁移（auth_method 补充）：
  src-tauri/migrations/global/010_add_auth_method.sql  — global_connections 增加 auth_method 列
  src-tauri/migrations/project_meta/012_add_auth_method.sql — connections 增加 auth_method 列

v2.0 待修改文件：
  src-tauri/src/core/persistence/env_store.rs           — + origin/source_id/snapshot_at 读写
  src-tauri/src/core/persistence/network_store.rs       — + origin/source_id/snapshot_at 读写
  src-tauri/src/core/persistence/auth_store.rs          — + origin/source_id/snapshot_at 读写
  src-tauri/src/commands/data_source_commands.rs        — + snapshot/env merge/chain validate IPC
  src-tauri/src/commands/connection_commands.rs         — 透传 protocol_chain + env_policies 字段

迁移文件：
  src-tauri/migrations/global/008_add_data_source_module.sql
  src-tauri/migrations/global/009_add_id_prefix_convention.sql   ← ID 前缀 + 环境 Seed
  src-tauri/migrations/global/010_add_auth_method.sql            ← auth_method 列
  src-tauri/migrations/project_meta/010_add_data_source_module.sql
  src-tauri/migrations/project_meta/011_add_id_prefix_snapshot.sql ← 快照溯源字段
  src-tauri/migrations/project_meta/012_add_auth_method.sql       ← auth_method 列
```

### 4.2 Rust 结构体

```rust
// ---------- 驱动相关 ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSourceType {
    pub id: String,
    pub name: String,
    pub category: String,
    pub icon: Option<String>,
    pub enabled: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Driver {
    pub id: String,
    pub type_id: String,
    pub name: String,
    pub driver_kind: String,
    pub is_file: bool,
    pub default_port: Option<i32>,
    pub url_template: Option<String>,
    pub download_url: Option<String>,
    pub download_checksum: Option<String>,
    pub version: Option<String>,
    pub config_schema: String,
    pub supported_auth_types: Option<String>,
    pub capabilities: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriverFile {
    pub id: String,
    pub driver_id: String,
    pub file_path: String,
    pub file_name: String,
    pub file_size: Option<i64>,
    pub checksum: Option<String>,
    pub version: String,
    pub installed_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DriverAvailability {
    Ready,                                    // 可用
    NotInstalled { download_url: String },    // 未安装但可下载
    NotEnabled,                               // 未在项目中启用
    NotDefined,                               // 驱动定义不存在
}

// ---------- 环境相关 ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Environment {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub color: Option<String>,
    pub sort_order: i32,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentPolicy {
    pub id: String,
    pub environment_id: String,
    pub policy_type: String,    // read_only|navigation_filter|query_timeout|max_rows|ddl_blocked|dml_blocked|confirm_required
    pub policy_config: Option<String>,
    pub enabled: bool,
    pub created_at: String,
}

// ---------- 认证/网络配置 ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    pub id: String,
    pub name: Option<String>,
    pub auth_type: String,      // password|keyfile|kerberos|oauth2|aws_iam|gcp_sa
    pub auth_data: String,      // JSON，密码已 AES-256-GCM 加密
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub id: String,
    pub name: Option<String>,
    pub network_type: String,   // ssh|proxy|ssl|none
    pub config: String,         // JSON，结构依赖 network_type
    pub created_at: String,
    pub updated_at: String,
}

// ---------- 连接扩展字段（追加到已有结构体）----------
// GlobalConnectionInfo 和 ProjectConnection 各自新增：
// pub description: Option<String>,
// pub driver_id: Option<String>,
// pub environment_id: Option<String>,
// pub auth_config_id: Option<String>,
// pub network_config_id: Option<String>,
// pub driver_properties: Option<String>,   // JSON
// pub advanced_options: Option<String>,    // JSON

// ---------- 驱动可用性枚举（已序列化）----------
// DriverAvailability 已 derive(serde::Serialize)，tag="status" 模式
// 返回值：{ "status": "ready" } | { "status": "not_installed", "download_url": "..." } | ...
```

### 4.3 纯函数共享模式

四个 store 模块（`env_store` / `auth_store` / `network_store` / `driver_store`）设计为**纯函数**，接收 `rusqlite::Connection` 引用而非持有状态：

```rust
// env_store.rs
pub fn create_environment(conn: &Connection, env: &Environment) -> Result<(), CoreError> { ... }
pub fn list_environments(conn: &Connection) -> Result<Vec<Environment>, CoreError> { ... }
pub fn update_environment(conn: &Connection, env: &Environment) -> Result<(), CoreError> { ... }
pub fn delete_environment(conn: &Connection, id: &str) -> Result<(), CoreError> { ... }
```

调用方各自传入自己的 DB connection：

```rust
// global_db.rs 调用
let conn = self.sqlite_pool.acquire().await?;
env_store::create_environment(conn.inner()?, &env)?;

// project_db.rs 调用
let conn = self.get_connection()?;
env_store::create_environment(&conn, &env)?;
```

### 4.4 键逻辑：驱动可用性检查

```rust
// driver_service.rs

impl DriverService {
    /// 检查驱动对指定项目是否可用
    pub async fn check_driver_for_project(
        &self,
        driver_id: &str,
        project_db: &ProjectDatabaseManager,
    ) -> Result<DriverAvailability, CoreError> {
        // Step 1: 检查驱动定义是否存在
        let driver = match self.find_driver(driver_id)? {
            Some(d) => d,
            None => return Ok(DriverAvailability::NotDefined),
        };

        // Step 2: 检查项目是否启用了该驱动
        if !project_db.is_driver_enabled(driver_id)? {
            return Ok(DriverAvailability::NotEnabled);
        }

        // Step 3: Native 驱动无需文件检查
        if driver.driver_kind == "native" {
            return Ok(DriverAvailability::Ready);
        }

        // Step 4: 检查本机是否安装了驱动文件
        if self.is_driver_installed_locally(driver_id)? {
            Ok(DriverAvailability::Ready)
        } else {
            Ok(DriverAvailability::NotInstalled {
                download_url: driver.download_url.unwrap_or_default(),
            })
        }
    }
}
```

### 4.5 项目打开时驱动自检

```rust
// core/project/store.rs 中的独立异步函数
// 在 Tauri 命令 open_project_by_path / open_project_by_id 中调用

pub async fn check_project_missing_drivers(
    project_path: &Path,
) -> Result<Vec<MissingDriver>, CoreError> {
    // 1. 打开项目 meta.db 读取 project_drivers
    // 2. 对比 global.db.driver_files 检查本机安装状态
    // 3. Native 驱动自动跳过
    // 4. 返回缺失的外部驱动列表
}

// Tauri 命令中：
// let missing = check_project_missing_drivers(Path::new(&path)).await?;
// response.missing_drivers = missing;
```

> 已实现：`open_project_by_path` 和 `open_project_by_id` 打开项目后自动调用，结果通过 `ProjectInfoResponse.missing_drivers` 返回前端

---

## 五、API 接口文档

### 5.1 驱动管理

#### `get_data_source_types`

```
获取数据源类型目录（按 category 分组）
Tauri Command: get_data_source_types
参数: { category?: string }
返回: DataSourceType[]

前端用途: 左侧数据源树顶层
```

#### `get_available_drivers`

```
获取指定项目可用的驱动列表
Tauri Command: get_available_drivers
参数: { project_id?: string }
返回: { drivers: Driver[], missing: MissingDriver[] }

drivers 包含所有已启用且就绪的驱动
missing 包含已启用但本机未安装外部文件的驱动

前端用途:
  - 新建连接时的驱动下拉
  - 项目打开时弹窗展示缺失驱动
```

#### `get_driver_detail`

```
获取单个驱动完整信息（含 config_schema）
Tauri Command: get_driver_detail
参数: { driver_id: string }
返回: Driver + { availability: string, ... }

前端用途: 按 JSON Schema 动态渲染连接表单
```

#### `install_driver`

```
下载并安装外部驱动文件
Tauri Command: install_driver
参数: { driver_id: string }
返回: { success: bool, message: string }

流程: 下载 → 校验 checksum → 写入驱动文件目录 → 注册到 driver_files 表
```

#### `enable_driver_for_project`

```
为指定项目启用一个驱动（写入 project_drivers 表）
Tauri Command: enable_driver_for_project
参数: { project_id: string, driver_id: string }
返回: { success: bool }

流程:
  1. 检查 driver_id 在 global.db.drivers 中是否存在
  2. INSERT INTO project_drivers (driver_id, enabled)
  3. 如果 driver_kind != native 且本机未安装 → 前端收到 missing 状态 → 引导安装
```

#### `disable_driver_for_project`

```
为指定项目禁用一个驱动（软操作：project_drivers.enabled = 0）
Tauri Command: disable_driver_for_project
参数: { project_id: string, driver_id: string }
返回: { success: bool }

注意: 已有连接不会自动删除，只是不能创建新连接
```

#### `get_all_drivers_catalog`

```
获取全局驱动目录（驱动市场，所有已定义的驱动）
Tauri Command: get_all_drivers_catalog
参数: { category?: string, driver_kind?: string }
返回: Driver[]

前端用途: "驱动市场" 页面，展示所有可安装的驱动
```

### 5.2 环境管理

#### `list_environments`

```
获取环境列表
Tauri Command: list_environments
参数: { scope: "global" | "project", project_id?: string }
返回: Environment[]
```

#### `create_environment` / `update_environment` / `delete_environment`

```
环境 CRUD
参数: { scope, project_id?, name, description?, color?, sort_order? }
返回: Environment / { success: bool }
```

### 5.3 环境策略

#### `list_environment_policies`

```
获取某环境的所有策略
Tauri Command: list_environment_policies
参数: { scope, project_id?, environment_id }
返回: EnvironmentPolicy[]
```

#### `create_environment_policy` / `update_environment_policy` / `delete_environment_policy`

```
策略 CRUD
参数: { scope, project_id?, environment_id, policy_type, policy_config?, enabled }
返回: EnvironmentPolicy / { success: bool }
```

### 5.4 认证配置

#### `list_auth_configs`

```
获取认证配置列表（auth_data 脱敏返回）
Tauri Command: list_auth_configs
参数: { scope, project_id?, auth_type? }
返回: { id, name, auth_type, created_at, updated_at }[]  // 不含 auth_data
```

#### `create_auth_config` / `delete_auth_config`

```
认证配置创建/删除（密码自动 AES-256-GCM 加密）
参数: { scope, project_id?, name, auth_type, auth_data: object }
返回: AuthConfig / { success: bool }
```

### 5.5 网络配置

#### `list_network_configs`

```
获取网络配置列表
Tauri Command: list_network_configs
参数: { scope, project_id?, network_type? }
返回: NetworkConfig[]
```

#### `create_network_config` / `update_network_config` / `delete_network_config`

```
网络配置 CRUD
参数: { scope, project_id?, name, network_type, config: object }
返回: NetworkConfig / { success: bool }
```

### 5.6 连接（扩展现有命令）

#### `connect_database` (扩展)

已扩展为完整的数据源连接命令，包含驱动校验链：

```
connect_database 命令校验链:
  1. 基础校验：URL 非空、connection_type 合法
  2. 驱动校验（仅当 driver_id 非空时）：
     a. driver_id 是否在 global.db.drivers 中已定义？
     b. 项目连接：driver_id 是否在 project_drivers 中启用？
     c. 外部驱动：driver_id 对应文件是否在 driver_files 中已安装？
  3. 连接建立：通过 ConnectionService 创建数据库连接
  4. 持久化：全局连接自动保存到 global_connections 表（含所有新字段）
```

新增参数（已有旧参数不变）：

```
{
  driver_id?: string,
  environment_id?: string,
  auth_config_id?: string,
  network_config_id?: string,
  driver_properties?: string,   // JSON
  advanced_options?: string,    // JSON
}
```

#### `test_connection`

```
测试连接（不持久化），复用 connection_service 的 connect_with_type 管道
参数: { db_type, url }
返回: TestConnectionResponse { success, message, latency_ms }
```

> 备注：test_connection 当前复用基础连接管道，不单独做数据库保存。连接配置测试由前端通过 connect_database 验证。

---

## 六、前端交互流程

### 6.1 新建连接

```
用户点击 [新建连接]
    ↓
① get_data_source_types() → 左侧数据源类型树（按 category 分组）
    ↓
② 用户选择 "Oracle"
    ↓
③ get_available_drivers(project_id) → 过滤 type_id=oracle → 返回可用驱动
    ├── 如果是项目 B（没安装 Oracle）→ 返回空列表 → 前端显示 "暂无可用驱动" 🔒
    └── 如果是项目 A（已安装 Oracle）→ 返回 oracle-jdbc → 继续
    ↓
④ 用户选择 "Oracle JDBC" → get_driver_detail("oracle-jdbc") → 获取 config_schema
    ↓
⑤ 按 JSON Schema 动态渲染表单：主机/端口/数据库/用户名/密码/SSL...
    ↓
⑥ 可选：选择环境 / 认证配置 / 网络配置
    ↓
⑦ 可选：点击 [测试连接]
    ↓
⑧ 点击 [保存] → create_connection()
```

### 6.2 驱动市场（安装新驱动）

```
用户进入 "驱动市场" 页面
    ↓
① get_all_drivers_catalog() → 所有已定义的驱动
    ↓
② 用户找到 "Oracle JDBC" → 点击 [安装]
    ↓
③ enable_driver_for_project(project_id, "oracle-jdbc")
    ├── 写入 project_drivers
    └── 如果未安装驱动文件 → 引导下载
    ↓
④ install_driver("oracle-jdbc")
    ├── 下载 ojdbc8.jar
    ├── 校验 checksum
    └── 注册到 driver_files
    ↓
⑤ 驱动就绪 → 出现在项目的可用驱动列表中
```

### 6.3 项目迁移后驱动自检

```
用户打开项目 A（从其他机器迁移过来的）
    ↓
① ProjectStore::open() → check_missing_drivers()
    ├── 遍历 project_drivers 中所有启用的驱动
    ├── Native 驱动 (mysql/postgres/sqlite/duckdb) → Ready ✅
    └── Oracle JDBC → 检查 driver_files + 文件系统 → 文件不存在 → NotInstalled ⚠️
    ↓
② 返回 missing 列表给前端
    ↓
③ 前端弹窗：
    ┌───────────────────────────────────────────────┐
    │  ⚠️ 检测到缺失驱动                              │
    │                                                │
    │  以下驱动在 project_drivers 中启用但本机未安装：  │
    │  🔴 Oracle JDBC (21.0.0)                        │
    │     描述：Oracle 数据库 JDBC 驱动                │
    │     [安装驱动（自动下载）]  [暂时禁用]            │
    │                                                │
    │  点击 "暂时禁用" → project_drivers.enabled = 0   │
    │  点击 "安装驱动" → 下载并安装                      │
    └───────────────────────────────────────────────┘
```

---

## 七、v2.0 完整开发计划

> 本计划覆盖：新增数据源连接的全链路（环境选择、网络协议链、认证配置），
> 包含 ID 前缀约定、快照机制、5 类环境策略的完整前后端实现。

### 7.1 开发阶段总览

```
Phase 0: 基础就绪（0.5 天）  →  011 migration + ID 前缀生成器 + snapshot store
Phase 1: 后端 IPC（1.5 天）   →  快照 IPC + Seed 环境策略 + 协议链校验
Phase 2: 前端核心（5 天）     →  NetworkTab 重写 + AdvancedTab 改造 + 覆盖层
Phase 3: Stores + 集成（3 天）→  Pinia stores + IPC 对接 + 表单聚合
Phase 4: 联调测试（2 天）     →  端到端测试 + 迁移验证
─────────────────────────────────────────────────────────
共计：12 天（前后端并行可压缩至 8 天）
```

---

### 7.2 Phase 0：基础就绪（后端）

| #   | 任务                                        | 文件                                                     | 说明                                                                           |
| --- | ------------------------------------------- | -------------------------------------------------------- | ------------------------------------------------------------------------------ |
| 0-1 | 011 migration 注册                          | `migrations/project_meta/011_add_id_prefix_snapshot.sql` | 已创建，确认迁移系统加载顺序                                                   |
| 0-2 | ID 前缀生成器                               | `core/persistence/id_prefix.rs` 新建                     | 工具函数：`generate_gid()`, `generate_pid()`, `generate_gpid()`                |
| 0-3 | 快照 store                                  | `core/persistence/snapshot_store.rs` 新建                | `snapshot_global_to_project()` 纯函数：接收 global/project 两个 conn，执行快照 |
| 0-4 | 更新 env_store / network_store / auth_store | 修改                                                     | 新增 `origin`, `source_id`, `snapshot_at` 字段读写                             |
| 0-5 | 更新已有迁移                                | `global/008` + `project_meta/010`                        | 现有 INSERT 语句的 ID 加上前缀（如 `G_ENV_DEV`, `P_xxx`）                      |

#### ID 前缀生成器设计

```rust
// core/persistence/id_prefix.rs

pub const ID_SEP: &str = "_";

pub fn generate_gid(prefix: &str, table: &str) -> String {
    // G_{TABLE}_{UUID_SHORT}  例: G_ENV_a1b2c3d4
    format!("G{}{}{}{}", ID_SEP, table, ID_SEP, nanoid::nanoid!(12))
}

pub fn generate_pid(prefix: &str, table: &str) -> String {
    // P_{TABLE}_{UUID_SHORT}  例: P_NET_a1b2c3d4
    format!("P{}{}{}{}", ID_SEP, table, ID_SEP, nanoid::nanoid!(12))
}

pub fn generate_gpid(source_gid: &str) -> String {
    // GP_{TABLE}_{UUID_SHORT}  例: GP_ENV_a1b2c3d4
    let table = source_gid.split(ID_SEP).nth(1).unwrap_or("UNK");
    format!("GP{}{}{}{}", ID_SEP, table, ID_SEP, nanoid::nanoid!(12))
}

pub fn parse_id_source(id: &str) -> IdSource {
    if id.starts_with("GP_") { IdSource::GlobalSnapshot }
    else if id.starts_with("P_") { IdSource::Project }
    else if id.starts_with("G_") { IdSource::Global }
    else { IdSource::Unknown }
}

pub fn extract_table(id: &str) -> &str {
    // G_ENV_xxx → "ENV";  P_NET_xxx → "NET";  GP_ENV_xxx → "ENV"
    id.split(ID_SEP).nth(1).unwrap_or("UNK")
}
```

#### 快照 store 设计

```rust
// core/persistence/snapshot_store.rs

/// 从全局快照环境到项目
/// 返回: 生成的 GP_xxx ID
pub fn snapshot_environment(
    global_conn: &rusqlite::Connection,
    project_conn: &rusqlite::Connection,
    global_env_id: &str,
) -> Result<(String, Vec<String>), CoreError> {
    // 1. 读 global 环境 + 策略
    let env = env_store::get_environment(global_conn, global_env_id)?;
    let policies = env_store::list_environment_policies(global_conn, global_env_id)?;

    // 2. 查重：project 中是否已有此快照
    if let Some(existing) = env_store::find_by_source_id(project_conn, global_env_id)? {
        return Ok((existing.id, vec![])); // 已存在，复用
    }

    // 3. 生成 GP_xxx ID，写入 project
    let gpid = generate_gpid(global_env_id);
    env_store::create_environment_snapshot(project_conn, &gpid, &env, global_env_id)?;
    let policy_ids: Vec<String> = policies.iter().map(|p| {
        let pid = generate_gpid(&p.id);
        env_store::create_policy_snapshot(project_conn, &pid, p, &gpid)?;
        Ok(pid)
    }).collect::<Result<_, CoreError>>()?;

    Ok((gpid, policy_ids))
}
```

---

### 7.3 Phase 1：后端 IPC 层（Rust）

| #   | 任务              | 文件                                             | 说明                                                                         |
| --- | ----------------- | ------------------------------------------------ | ---------------------------------------------------------------------------- |
| 1-1 | 全局环境 Seed     | `global/009_seed_environments.sql` 新建          | 5 环境 + 25 策略 policy_config JSON                                          |
| 1-2 | 快照 IPC Commands | `data_source_commands.rs` 新增                   | `snapshot_global_env`, `snapshot_global_network`, `snapshot_global_auth`     |
| 1-3 | 协议链校验        | `core/driver/connection/chain_validator.rs` 新建 | `validate_protocol_chain()`, `max_ss_proxy_hops=4`, `ssl_must_be_last`       |
| 1-4 | 协议链校验 IPC    | `data_source_commands.rs` 新增                   | `validate_protocol_chain` Command                                            |
| 1-5 | 合并环境列表 IPC  | `data_source_commands.rs` 新增                   | `list_environments_for_connection(scope, project_id?)` 合并 global + project |

#### Seed 数据：5 环境 + 25 策略

```sql
-- global/009_seed_environments.sql
INSERT OR IGNORE INTO environments (id, name, description, color, sort_order) VALUES
    ('G_ENV_dev',  '开发环境', '本地开发与联调',             '#52C41A', 1),
    ('G_ENV_test', '测试环境', '功能验证与冒烟测试',         '#1890FF', 2),
    ('G_ENV_stag', '预发布环境', '生产前最后验证',           '#FA8C16', 3),
    ('G_ENV_prod', '生产环境', '正式线上服务',               '#F5222D', 4),
    ('G_ENV_sand', '沙箱环境', '数据探索与临时分析',         '#722ED1', 5);

-- 每个环境 5 条策略（security / schema / performance / audit / ui）
INSERT OR IGNORE INTO environment_policies (id, environment_id, policy_type, policy_config, enabled) VALUES
    -- 开发环境：最宽松
    ('G_POL_dev_sec',  'G_ENV_dev',  'security',    '{"readonly":false,"writeConfirm":false,"ddlConfirm":false,"dropConfirm":"allow","autocommit":true,"rowLimit":0,"sizeLimit":0}', 1),
    ('G_POL_dev_sch',  'G_ENV_dev',  'schema',      '{"autoLoad":true,"loadDepth":3,"showSystem":true,"refreshInterval":120}', 1),
    ('G_POL_dev_perf', 'G_ENV_dev',  'performance', '{"poolSize":5,"queryTimeout":60,"connectTimeout":10,"heartbeat":300,"maxReconnect":3}', 1),
    ('G_POL_dev_audit','G_ENV_dev',  'audit',       '{"sqlLog":false,"operationRecord":false,"sensitiveTableAlert":false}', 1),
    ('G_POL_dev_ui',   'G_ENV_dev',  'ui',          '{"topBarColor":"#52C41A","tabIndicator":true,"sqlWarningBanner":false,"writeBtnStyle":"normal"}', 1),

    -- 测试环境：中等
    ('G_POL_test_sec', 'G_ENV_test', 'security',    '{"readonly":false,"writeConfirm":true,"ddlConfirm":true,"dropConfirm":"confirm","autocommit":true,"rowLimit":5000,"sizeLimit":0}', 1),
    ('G_POL_test_sch', 'G_ENV_test', 'schema',      '{"autoLoad":true,"loadDepth":3,"showSystem":false,"refreshInterval":300}', 1),
    ('G_POL_test_perf','G_ENV_test', 'performance', '{"poolSize":5,"queryTimeout":120,"connectTimeout":10,"heartbeat":300,"maxReconnect":3}', 1),
    ('G_POL_test_aud','G_ENV_test',  'audit',       '{"sqlLog":true,"operationRecord":false,"sensitiveTableAlert":false}', 1),
    ('G_POL_test_ui',  'G_ENV_test',  'ui',          '{"topBarColor":"#1890FF","tabIndicator":true,"sqlWarningBanner":true,"writeBtnStyle":"confirm"}', 1),

    -- 预发布环境：接近生产
    ('G_POL_stag_sec', 'G_ENV_stag', 'security',    '{"readonly":false,"writeConfirm":true,"ddlConfirm":true,"dropConfirm":"confirm","autocommit":false,"rowLimit":10000,"sizeLimit":500}', 1),
    ('G_POL_stag_sch', 'G_ENV_stag', 'schema',      '{"autoLoad":true,"loadDepth":3,"showSystem":false,"refreshInterval":600}', 1),
    ('G_POL_stag_perf','G_ENV_stag', 'performance', '{"poolSize":10,"queryTimeout":300,"connectTimeout":15,"heartbeat":120,"maxReconnect":5}', 1),
    ('G_POL_stag_aud','G_ENV_stag',  'audit',       '{"sqlLog":true,"operationRecord":true,"sensitiveTableAlert":true}', 1),
    ('G_POL_stag_ui',  'G_ENV_stag',  'ui',          '{"topBarColor":"#FA8C16","tabIndicator":true,"sqlWarningBanner":true,"writeBtnStyle":"confirm"}', 1),

    -- 生产环境：最严格
    ('G_POL_prod_sec', 'G_ENV_prod', 'security',    '{"readonly":false,"writeConfirm":true,"ddlConfirm":true,"dropConfirm":"disable","autocommit":false,"rowLimit":1000,"sizeLimit":100}', 1),
    ('G_POL_prod_sch', 'G_ENV_prod', 'schema',      '{"autoLoad":true,"loadDepth":2,"showSystem":false,"refreshInterval":900}', 1),
    ('G_POL_prod_perf','G_ENV_prod', 'performance', '{"poolSize":20,"queryTimeout":600,"connectTimeout":20,"heartbeat":60,"maxReconnect":10}', 1),
    ('G_POL_prod_aud','G_ENV_prod',  'audit',       '{"sqlLog":true,"operationRecord":true,"sensitiveTableAlert":true}', 1),
    ('G_POL_prod_ui',  'G_ENV_prod',  'ui',          '{"topBarColor":"#F5222D","tabIndicator":true,"sqlWarningBanner":true,"writeBtnStyle":"danger"}', 1),

    -- 沙箱环境：分析用途
    ('G_POL_sand_sec', 'G_ENV_sand', 'security',    '{"readonly":true,"writeConfirm":false,"ddlConfirm":false,"dropConfirm":"disable","autocommit":false,"rowLimit":0,"sizeLimit":0}', 1),
    ('G_POL_sand_sch', 'G_ENV_sand', 'schema',      '{"autoLoad":false,"loadDepth":1,"showSystem":false,"refreshInterval":0}', 1),
    ('G_POL_sand_perf','G_ENV_sand', 'performance', '{"poolSize":3,"queryTimeout":1800,"connectTimeout":30,"heartbeat":600,"maxReconnect":1}', 1),
    ('G_POL_sand_aud','G_ENV_sand',  'audit',       '{"sqlLog":true,"operationRecord":false,"sensitiveTableAlert":false}', 1),
    ('G_POL_sand_ui',  'G_ENV_sand',  'ui',          '{"topBarColor":"#722ED1","tabIndicator":false,"sqlWarningBanner":false,"writeBtnStyle":"normal"}', 1);
```

---

### 7.4-7.6 前端实现 → 独立文档

前端核心组件（NetworkTab 重写、AdvancedTab 改造）、Stores、Composable、覆盖层管理器的详细设计，
已独立为前端文档：

> 📄 **[../frontend/connection/add-datasource-frontend-plan.md](../frontend/connection/add-datasource-frontend-plan.md)**

该文档覆盖：

- 组件树 + Props/Emits 契约
- TypeScript 类型体系（ChainHopItem / Environment / EnvironmentPolicies 等）
- Pinia Stores（environmentStore / networkConfigStore）
- useAddDataSource composable（表单聚合 + 校验 + 提交）
- NetworkTab 协议链交互伪代码
- AdvancedTab 环境选择联动逻辑
- 覆盖层管理器设计
- 13 个端到端测试场景

---

### 7.7 IPC 接口完整清单

#### 新增 IPC Commands

| Command                               | 入参                             | 返回                  | 用途                                             |
| ------------------------------------- | -------------------------------- | --------------------- | ------------------------------------------------ |
| `snapshot_global_env`                 | `{ global_env_id, project_id }`  | `String` (GP_xxx)     | 快照全局环境到项目                               |
| `snapshot_global_network`             | `{ global_net_id, project_id }`  | `String` (GP_xxx)     | 快照全局网络配置到项目                           |
| `snapshot_global_auth`                | `{ global_auth_id, project_id }` | `String` (GP_xxx)     | 快照全局认证配置到项目                           |
| `validate_protocol_chain`             | `{ chain: ChainHopItem[] }`      | `{ valid, errors[] }` | 校验协议链合法性                                 |
| `list_environments_for_connection`    | `{ scope, project_id? }`         | `Environment[]`       | 合并 global + project 环境列表（带 source 标记） |
| `list_network_configs_for_connection` | `{ scope, project_id? }`         | `NetworkConfig[]`     | 合并 global + project 网络配置列表               |
| `seed_default_environments`           | `{ scope, project_id? }`         | `void`                | 预置默认 5 环境 + 25 策略                        |

#### 已有 IPC（无需改动）

| Command                                                                                              | 说明                             |
| ---------------------------------------------------------------------------------------------------- | -------------------------------- |
| `get_drivers` / `load_driver_schema`                                                                 | 驱动加载                         |
| `test_connection`                                                                                    | 连接测试                         |
| `connect_database`                                                                                   | 建立连接                         |
| `list_environments` / `create_environment` / `update_environment` / `delete_environment`             | 环境 CRUD（已有）                |
| `list_network_configs` / `create_network_config` / `update_network_config` / `delete_network_config` | 网络 CRUD（已有）                |
| `list_auth_configs` / `create_auth_config` / `delete_auth_config`                                    | 认证 CRUD（已有）                |
| `save_connection` / `update_connection`                                                              | 连接保存（已有，需扩展字段透传） |

---

### 7.8 风险与缓解

| 风险                                       | 影响 | 缓解措施                                                                    |
| ------------------------------------------ | ---- | --------------------------------------------------------------------------- |
| naive-ui NPopover 环境下拉体验             | 中   | 降级为 NSelect + 自定义渲染模板；实测 NPopover 在 NTabs 内可用              |
| HTML5 Drag & Drop 在 Electron/Tauri 兼容性 | 低   | 降级为手动 ↑↓ 排序按钮；Tauri webview 支持标准 DnD API                      |
| 快照触发时机（首次引用 vs 手动同步）       | 中   | 首次引用时自动触发；提供"刷新快照"按钮用于全局配置更新后同步                |
| GP_xxx ID 前缀与已有数据兼容               | 低   | 011 migration 对存量数据 origin 默认 'project'；已有连接不受影响            |
| 环境策略 JSON 序列化复杂                   | 低   | 使用 serde_json Value 存储，Rust 层不做 schema 校验；前端按类型维护 TS 接口 |

---

## 八、关键设计决策

| 决策                 | 结论                                                    | 理由                                                  |
| -------------------- | ------------------------------------------------------- | ----------------------------------------------------- |
| `project_drivers` 表 | 在 project meta.db 中                                   | 实现项目级驱动可见性隔离。新项目只种子4个 Native 驱动 |
| `drivers` 表         | 仅 global.db                                            | 驱动定义是 "通用数据库知识"，不随项目变化             |
| `driver_files` 表    | 仅 global.db                                            | 记录 "本机安装了什么"，是机器级信息                   |
| 跨 DB 引用           | 不用 FK，Rust 层校验                                    | SQLite 不支持跨文件外键                               |
| Native 驱动          | 不注册 driver_files                                     | 编译在 Rust 二进制中，无需外部文件                    |
| 密码加密             | 复用 crypto.rs AES-256-GCM                              | 已有成熟方案                                          |
| Store 复用           | 纯函数模式                                              | Global 和 Project 各传自己的 Connection，零代码重复   |
| 迁移策略             | ALTER TABLE ADD COLUMN                                  | 不丢失现有数据                                        |
| ⭐ ID 前缀约定       | G*/P*/GP\_ 三前缀                                       | 通过 ID 前缀识别来源，无需额外字段或跨表 JOIN         |
| ⭐ 快照机制          | 首次引用时自动触发                                      | 项目迁移后快照本地可用，无需 global.db 依赖           |
| ⭐ 协议链存储        | 序列化到 `advanced_options` JSON                        | 灵活支持动态长度，不固化为固定列数                    |
| ⭐ 环境策略结构      | 5 类 policy_type (security/schema/performance/audit/ui) | 每类 policy_config 为独立 JSON 对象，扩展性强         |

---

## 九、相关文档

| 文档                 | 路径                                                                                                                                                   | 说明                                  |
| -------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------- |
| 数据库字典（含新表） | [DATABASE-DICTIONARY.md](./DATABASE-DICTIONARY.md)                                                                                                     | 所有表完整 DDL + 字段说明             |
| 后端架构             | [ARCHITECTURE.md](./ARCHITECTURE.md)                                                                                                                   | 四层微内核架构                        |
| 迁移系统             | [MIGRATION_SYSTEM.md](./MIGRATION_SYSTEM.md)                                                                                                           | Migration 框架设计                    |
| 项目模块架构         | [PROJECT_MODULE_ARCHITECTURE.md](./PROJECT_MODULE_ARCHITECTURE.md)                                                                                     | 项目 + 应用双层存储设计               |
| ⭐ 连接方法设计      | [CONNECTION-METHOD-DESIGN.md](./CONNECTION-METHOD-DESIGN.md)                                                                                           | 协议链 (Chain) 后端设计 + TunnelGuard |
| ⭐ 网络配置 UI       | [../frontend/NETWORK-CONFIG-UI-DESIGN.md](../frontend/NETWORK-CONFIG-UI-DESIGN.md)                                                                     | 协议链 + 环境策略 UI v2.0             |
| ⭐ v5 原型           | [../../../prototype/add-datasource-v5.html](../../../prototype/add-datasource-v5.html)                                                                 | 前端目标原型                          |
| ⭐ 011 migration     | [../../../src-tauri/migrations/project_meta/011_add_id_prefix_snapshot.sql](../../../src-tauri/migrations/project_meta/011_add_id_prefix_snapshot.sql) | ID 前缀 + 快照溯源迁移                |

---

## 十、实现状态

### 10.1 已完成的文件

| 类型        | 文件                                                     | 说明                                                                                                                              |
| ----------- | -------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| Migration   | `migrations/global/008_add_data_source_module.sql`       | 全局新表 + 种子数据 + global_connections 扩展                                                                                     |
| Migration   | `migrations/project_meta/010_add_data_source_module.sql` | 项目新表 + project_drivers + connections 扩展                                                                                     |
| Store       | `core/persistence/driver_store.rs`                       | DataSourceType/Driver/DriverFile 结构体 + CRUD                                                                                    |
| Store       | `core/persistence/env_store.rs`                          | Environment/EnvironmentPolicy 结构体 + CRUD                                                                                       |
| Store       | `core/persistence/auth_store.rs`                         | AuthConfig 结构体 + CRUD                                                                                                          |
| Store       | `core/persistence/network_store.rs`                      | NetworkConfig 结构体 + CRUD                                                                                                       |
| Service     | `core/services/driver_service.rs`                        | DriverService + DriverAvailability + MissingDriver                                                                                |
| Command     | `commands/data_source_commands.rs`                       | 40 个 Tauri Commands（8 全局环境 + 8 全局策略 + 6 全局认证网络 + 4 驱动 + 14 项目级新增 env/auth/network）                        |
| Model       | `core/driver/registry/descriptors.rs`                    | DriverDescriptor +4 字段 (icon/capabilities/supported_auth_types/enabled)                                                         |
| Model       | `core/persistence/global_db.rs`                          | GlobalConnectionInfo +8 字段 + 24 个新 CRUD 方法                                                                                  |
| Model       | `core/persistence/project_connection_store.rs`           | ProjectConnection +8 字段 + project_drivers CRUD + seed_default_drivers                                                           |
| Model       | `core/persistence/project_db.rs`                         | ProjectDatabaseManager +14 项目级 CRUD 方法 (env/auth/network)                                                                    |
| Registry    | `core/persistence/mod.rs`                                | 注册 4 个新子模块                                                                                                                 |
| Registry    | `commands/mod.rs`                                        | 注册 data_source_commands                                                                                                         |
| Registry    | `core/services/mod.rs`                                   | 注册 driver_service                                                                                                               |
| Registry    | `lib.rs`                                                 | 24 个新 Tauri Command 登记                                                                                                        |
| Integration | `commands/project_store_commands.rs`                     | ProjectConnectionResponse 扩展                                                                                                    |
| Integration | `core/services/connection_service.rs`                    | save_global_connection 参数扩展                                                                                                   |
| Integration | `commands/connection_commands.rs`                        | ConnectDatabaseInput/ConnectionInfoResponse/GlobalConnectionInfoResponse 扩展 +7 新字段；connect_database 传递新字段 + 驱动校验链 |
| Integration | `core/project/store.rs`                                  | check_project_missing_drivers 异步自检函数，已接入 project_commands.rs 的项目打开命令                                             |
| Integration | `core/services/connection_service.rs`                    | connect_with_type 扩展 7 个数据源新参数，save_global_connection_to_db 透传                                                        |

### 10.2 编译状态

- ✅ `cargo check` 通过，零错误零警告
- ✅ `pnpm typecheck` 通过（仅存量文件报错，本次改动零新增）

### 10.2.1 v2.0 动态驱动注册修复（2026-05-20）

**问题**：`get_drivers` / `get_driver_info` 命令走内存 `DriverRegistry`（4个硬编码驱动），SQLite `drivers` 表有 10 种数据源类型 + 4 个驱动 seed 数据但未被命令层读取。新增驱动需要改 Rust 代码 + 发版。

**修复**：

| 文件                                  | 改动                                                                                                                                     |
| ------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ------------------- |
| `commands/driver_commands.rs`         | `get_drivers()` → `global_db.get_all_drivers()`（SQLite）；`get_driver_info()` → `global_db.get_driver()`                                |
| `ui/composables/useDriverRegistry.ts` | 从 TODO stub 改为 `invoke('get_data_source_types')` + `invoke('get_available_drivers')`                                                  |
| `domain/types.ts`                     | 新增 `DataSourceType`、`Driver`、`MissingDriver`、`DriverListResponse` 接口；`DataSourceCategory` / `DriverKind` 加 `                    | (string & {})` 兜底 |
| `ui/adapters/driver-adapter.ts`       | `parseConfigSchema()` 兼容两种格式：JSON Schema `{type,properties,required}` 和自定义 `{fields,options}`；类型引用改用 `domain/types.ts` |

**数据流**：

```
前端侧边栏                          Tauri Command                      SQLite
─────────                           ────────────                      ──────
useDriverRegistry.loadAll() ──→ get_data_source_types(None) ──→ data_source_types 表 (10行)
                            ──→ get_available_drivers(None) ──→ drivers 表 (4行)
                                   │
                                   └── config_schema JSON ──→ parseConfigSchema()
                                         ↓
                                   DriverFormSchema ──→ DynamicFormRenderer
```

**新增驱动无需发版**：`INSERT INTO drivers (...) VALUES (...)` 即可，前端下次 `get_available_drivers` 自动可见。

### 10.2.2 三方对齐验证（2026-05-20）

**Backend → Frontend struct 逐字段比对**：

| 结构体               | 字段数 | 对齐 |
| -------------------- | ------ | :--: |
| `DataSourceType`     | 6      |  ✅  |
| `Driver`             | 14     |  ✅  |
| `MissingDriver`      | 3      |  ✅  |
| `DriverListResponse` | 2      |  ✅  |

**Command 签名比对**：

| Command                 | Backend 参数                   | Frontend invoke   | 对齐 |
| ----------------------- | ------------------------------ | ----------------- | :--: |
| `get_data_source_types` | `category: Option<String>`     | 无参 → `None`     |  ✅  |
| `get_available_drivers` | `project_path: Option<String>` | `{ projectPath }` |  ✅  |

> Tauri `#[tauri::command]` 自动 **snake_case → camelCase** 重命名，`project_path` ↔ `projectPath` 映射正确。

**config_schema JSON 格式比对**：

SQLite seed `{fields:[{key,label,type,required,default,...}], options:[...]}` → `parseConfigSchema` 第一分支（自定义格式）✅

**兜底保护**：

- `DataSourceCategory` 类型：`'relational' | 'file-based' | ... | (string & {})` — 保留智能提示，同时兼容后端任意 `String` 值
- `DriverKind` 类型：同上，兼容后端未来新增 `driver_kind` 值

### 10.2.3 数据流全链路打通（2026-05-20）

**前端组件重建**（全部使用 Naive UI 组件，对齐原型 v5 布局）：

| 文件                      | 改动                                                                                                                         |
| ------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `AddDataSourceDialog.vue` | 重写：`useDriverRegistry.loadAll()` 在 `onMounted` 触发 → 数据通过 props 传递到 Sidebar 和 5 个 Tab；NTabs 渲染 5 个 TabPane |
| `DataSourceSidebar.vue`   | 重写：NInput 搜索 → 暂存列表(NButton +/X) → 分割线 → 4 分类折叠(NCollapse) → 驱动子项（按 `type_id` 关联）                   |
| `GeneralTab.vue`          | 重写：接收 `driver: Driver`，通过 `configSchemaToFormSchema()` → `DynamicFormRenderer` 动态渲染表单字段                      |
| `NetworkTab.vue`          | 重写：NCollapse 三段（SSH 隧道 / SSL TLS / HTTP 代理），typed refs                                                           |
| `CapabilitiesTab.vue`     | 新建：从 `driver.capabilities` JSON 解析能力列表，chip 样式展示                                                              |
| `DriverPropsTab.vue`      | 重写：key-value 属性行，动态增删                                                                                             |
| `AdvancedTab.vue`         | 重写：NSelect 环境 + NSwitch DuckDB 加速 + NCheckbox 只读 + 2x2 网格参数                                                     |
| `zh-CN.json` + `en.json`  | 新增 12 个 i18n key                                                                                                          |

**数据流**：

```
onMounted → loadAll()
  └→ invoke('get_data_source_types')  → dataSourceTypes[] → Sidebar(DatabaseTypes)
  └→ invoke('get_available_drivers') → drivers[]          → Sidebar(Drivers per type)
                                          │
                                          └→ selectedDriver
                                               └→ GeneralTab → configSchemaToFormSchema()
                                                    └→ DynamicFormRenderer
                                               └→ CapabilitiesTab → capabilities JSON
                                               └→ Network/DriverProps/Advanced tabs
```

### 10.3 后续待完成（按优先级）

| 优先级 | 任务                                          | 阶段    | 说明                                                                            |
| ------ | --------------------------------------------- | ------- | ------------------------------------------------------------------------------- |
| 🔴 P0  | ID 前缀生成器 `id_prefix.rs`                  | Phase 0 | `generate_gid()`, `generate_pid()`, `generate_gpid()`, `parse_id_source()`      |
| 🔴 P0  | 快照 store `snapshot_store.rs`                | Phase 0 | `snapshot_environment()`, `snapshot_network_config()`, `snapshot_auth_config()` |
| 🔴 P0  | 011 migration 注册到迁移系统                  | Phase 0 | 确认加载顺序，存量数据 origin='project'                                         |
| 🔴 P0  | 更新 env/network/auth store 读/写 origin 字段 | Phase 0 | origin / source_id / snapshot_at                                                |
| 🔴 P1  | Seed 5 环境 + 25 策略                         | Phase 1 | `global/009_seed_environments.sql` + 注册到迁移系统                             |
| 🔴 P1  | 快照 IPC Commands                             | Phase 1 | `snapshot_global_env`, `snapshot_global_network`, `snapshot_global_auth`        |
| 🔴 P1  | 协议链校验 `chain_validator.rs`               | Phase 1 | max 4 SSH/Proxy hops + SSL must be last + hop_name 合法                         |
| 🔴 P1  | 合并环境列表 IPC                              | Phase 1 | `list_environments_for_connection()` 合并 global + project                      |
| 🟡 P1  | 前端 TS 类型扩展                              | Phase 2 | ChainHopItem / Environment / EnvironmentPolicies / ConnectionScope              |
| 🟡 P1  | NetworkTab.vue 完整重写                       | Phase 2 | ✅ 已完成（Naive UI NCollapse 三段 SSH/SSL/Proxy）                              |
| 🟡 P1  | AdvancedTab.vue 改造                          | Phase 2 | ✅ 已完成（NSelect 环境 + NSwitch DuckDB + NCheckbox 只读 + 2x2 网格）          |
| 🟡 P2  | EnvironmentSelector.vue                       | Phase 2 | 环境紧凑下拉组件                                                                |
| 🟡 P2  | SecurityPolicySection.vue                     | Phase 2 | 安全策略可折叠                                                                  |
| 🟡 P2  | NetworkProfileManager.vue                     | Phase 2 | 覆盖层：网络配置文件管理器                                                      |
| 🟡 P2  | EnvironmentManager.vue                        | Phase 2 | 覆盖层：环境管理器                                                              |
| 🟢 P3  | environmentStore.ts                           | Phase 3 | Pinia store：环境列表 + CRUD                                                    |
| 🟢 P3  | networkConfigStore.ts 改造                    | Phase 3 | scope 筛选 + global/project 合并                                                |
| 🟢 P3  | useAddDataSource.ts composable                | Phase 3 | 表单聚合 + 校验 + payload 构建                                                  |
| 🟢 P4  | 端到端集成联调                                | Phase 4 | 13 个测试场景验证                                                               |
| 🟢 P4  | `install_driver` 实际下载逻辑                 | 后续    | 当前为占位实现，需实现实际的 .jar/.wasm 下载                                    |

### 10.4 架构红线合规

- ✅ 无 unwrap/expect
- ✅ Services 层只调用 connection/driver，不直接碰 datasource
- ✅ Pool 只负责连接，不负责 SQL 执行
- ✅ mod.rs 无测试代码
- ✅ 跨 DB 引用使用 Rust 层校验
- ✅ 纯函数模式：四个新 store 零代码重复
