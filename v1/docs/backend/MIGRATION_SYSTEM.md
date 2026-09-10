# 迁移系统文档

> 版本：v1.1
> 最后更新：2026-05-06
> 状态：✅ 持续更新

---

## 一、设计目标

### 1.1 核心原则

- **零硬编码**：所有 SQL 在独立文件中，Rust 代码只负责加载和执行
- **版本控制**：支持增量迁移，数据库结构可平滑升级
- **可维护性**：DBA 可直接修改 SQL 文件，无需懂 Rust
- **可测试性**：SQL 文件可独立测试语法和逻辑
- **多数据库支持**：按数据库类型分目录

### 1.2 为什么不选 ORM

| 维度         | ORM                      | SQL 迁移           |
| ------------ | ------------------------ | ------------------ |
| 适用场景     | 业务应用、固定模型       | 系统工具、动态结构 |
| 多数据库支持 | 抽象层丢失特性           | 原生 SQL 100% 支持 |
| 性能         | 多层抽象开销             | 直接执行           |
| 可控性       | 黑盒生成 SQL             | 完全透明           |
| 维护成本     | 高（依赖 + 模型 + 迁移） | 低（仅 SQL 文件）  |

---

## 二、目录结构

```
src-tauri/
├── migrations/                           # SQL 迁移文件目录（编译时嵌入）
│   ├── global/                           # 全局系统库迁移
│   │   ├── 001_init.sql                  # 应用信息、项目索引、全局连接
│   │   ├── 002_add_schema_name.sql       # 添加 schema_name 字段
│   │   ├── 003_add_duckdb_fed.sql        # 添加 use_duckdb_fed 字段
│   │   └── 004_add_metadata_path.sql     # 添加 metadata_path 字段
│   ├── project_meta/                     # 项目级元数据库迁移
│   │   ├── 001_init.sql                  # 项目信息、连接配置、SQL历史
│   │   ├── 002_refactor_query_history.sql # 重构 sql_history 为 query_history
│   │   ├── 003_add_schema_name.sql       # 添加 schema_name 字段
│   │   ├── 004_add_duckdb_fed.sql        # 添加 use_duckdb_fed 字段
│   │   └── 005_add_metadata_path.sql     # 添加 metadata_path 字段
│   ├── project_analysis/                 # 项目级分析引擎迁移（DuckDB）
│   │   └── 001_init.sql                  # 查询缓存、联邦连接、文件数据集
│   ├── connection_metadata/              # 连接级元数据库迁移
│       ├── 001_init.sql                  # 初始统一表结构
│       ├── 002_add_cache_version_and_compression.sql  # 版本控制与压缩
│       ├── 003_add_fts_search.sql       # FTS5 全文搜索支持
│       ├── 004_refactor_to_normalized.sql # 规范化表结构重构
│       ├── 005_normalized_fts_and_cascade.sql  # FTS 同步与级联删除
│       ├── 006_add_metadata_index.sql   # 索引表支持分页懒加载
│       └── 007_incremental_sync.sql     # 增量同步支持（V7）
│
└── src/core/
    └── migration/                        # 迁移系统实现
        ├── mod.rs                        # 模块导出
        ├── manager.rs                    # 迁移管理器核心
        ├── schema.rs                     # 版本追踪
        ├── executor.rs                   # SQL 执行器
        └── global_init.rs                # 全局系统初始化
```

---

## 三、迁移文件命名规范

### 3.1 格式

```
{版本号}_{描述}.sql
```

- **版本号**：三位数字，如 `001`、`002`、`010`
- **描述**：小写字母+下划线，如 `init`、`add_plugins`、`fix_indexes`

### 3.2 示例

```
001_init.sql              # 初始表结构
002_add_plugins.sql       # 新增插件相关表
003_add_credential_slots.sql
010_optimize_indexes.sql
```

### 3.3 执行顺序

- 按版本号升序执行
- 已执行的版本跳过
- 事务包裹，失败自动回滚

---

## 四、数据库层级

### 4.1 全局系统库

**路径**：`{data_dir}/RdataStation/rdata_station.global.sqlite`

**迁移类型**：`MigrationType::Global`

**职责**：

- 应用版本和运行记录
- 全局项目索引
- 插件注册中心（唯一权威）
- 全局系统设置
- 凭据管理（系统密钥链引用）

### 4.2 项目级元数据库

**路径**：`{project_path}/.rdata-station/meta/project.sqlite`

**迁移类型**：`MigrationType::ProjectMeta`

**职责**：

- 项目基础信息
- 项目内数据库连接配置
- SQL 执行历史
- 项目收藏 SQL
- UI 状态（导航树、布局）
- DuckDB 分析引擎配置
- 项目插件引用（锁定版本）

### 4.3 项目级分析引擎

**路径**：`{project_path}/.rdata-station/analytics/data.duckdb`

**迁移类型**：`MigrationType::ProjectAnalysis`

**职责**：

- 查询结果缓存
- 联邦连接状态（ATTACH 管理）
- 文件数据集索引（CSV/Parquet/Excel）
- 用户自建分析表

### 4.4 连接级元数据库

**路径**：`{data_dir}/RdataStation/metadata/conn_{conn_id}.sqlite`

**迁移类型**：`MigrationType::ConnectionMetadata`

**职责**：

- 数据库对象元数据缓存（表/视图/列/索引/函数）
- 元数据同步日志
- 每个连接独立，互不干扰

---

## 五、核心 API

### 5.1 迁移管理器

```rust
use crate::core::migration::{MigrationManager, MigrationType};

let manager = MigrationManager::new();

// 执行迁移
let applied = manager.migrate(&db_path, MigrationType::Global)?;

// 获取当前版本
let version = manager.get_current_version(&db_path)?;

// 获取已应用版本列表
let versions = manager.get_applied_versions(&db_path)?;

// 验证迁移文件（不执行）
let pending = manager.validate(MigrationType::ProjectMeta)?;
```

### 5.2 全局初始化

```rust
use crate::core::migration::initialize_global_system;

// 应用启动时调用
initialize_global_system()?;
```

### 5.3 项目创建时

```rust
// 在 ProjectStore::create() 中自动执行
let meta_db_path = meta_dir.join("meta").join("project.sqlite");
manager.migrate(&meta_db_path, MigrationType::ProjectMeta)?;

let analysis_db_path = meta_dir.join("analytics").join("data.duckdb");
manager.migrate(&analysis_db_path, MigrationType::ProjectAnalysis)?;
```

### 5.4 连接创建时

```rust
// 在 ConnectionService::connect() 中自动执行
let metadata_db_path = metadata_dir.join(format!("conn_{}.sqlite", conn_id));
manager.migrate(&metadata_db_path, MigrationType::ConnectionMetadata)?;
```

---

## 六、版本追踪

### 6.1 schema_version 表

每个迁移后的数据库都会包含：

```sql
CREATE TABLE IF NOT EXISTS schema_version (
    version     INTEGER PRIMARY KEY,
    name        TEXT NOT NULL,
    applied_at  INTEGER NOT NULL
);
```

### 6.2 字段说明

| 字段       | 类型    | 说明                       |
| ---------- | ------- | -------------------------- |
| version    | INTEGER | 迁移版本号                 |
| name       | TEXT    | 迁移描述（不含版本号前缀） |
| applied_at | INTEGER | 应用时间戳（Unix 秒）      |

---

## 七、事务保证

### 7.1 原子性

每个迁移在事务中执行：

```rust
let tx = conn.unchecked_transaction()?;
tx.execute_batch(&migration.sql)?;
SchemaTracker::record_version(&tx, version, name)?;
tx.commit()?;
```

### 7.2 失败处理

- 迁移失败自动回滚
- 版本记录不会写入
- 下次启动时重新尝试

---

## 八、扩展指南

### 8.1 添加新迁移

1. 在对应目录创建 SQL 文件：

   ```
   migrations/global/002_add_new_feature.sql
   ```

2. 编写 SQL（使用 `CREATE TABLE IF NOT EXISTS` 等安全语句）：

   ```sql
   CREATE TABLE IF NOT EXISTS new_feature (
       id TEXT PRIMARY KEY,
       name TEXT NOT NULL
   );
   ```

3. 无需修改 Rust 代码，重启应用自动执行

### 8.2 修改已有迁移

- **不推荐**修改已发布的迁移文件
- 如需修改，创建新版本迁移文件
- 使用 `ALTER TABLE` 等语句增量修改

### 8.3 回滚支持

当前版本不支持自动回滚，如需回滚：

1. 手动执行反向 SQL
2. 从 `schema_version` 表删除对应版本
3. 或重建数据库并重新执行迁移

**手动回滚示例（以撤销版本 011 为例）**：

```sql
-- 1. 执行反向 SQL（如移除索引）
-- global/011_add_config_indexes 的反向操作：
DROP INDEX IF EXISTS idx_auth_configs_type;
DROP INDEX IF EXISTS idx_network_configs_type;

-- 2. 从迁移记录表移除
DELETE FROM schema_version WHERE version = 11;

-- 3. 验证当前版本
SELECT MAX(version) FROM schema_version;
```

**安全注意事项**：

- 回滚前务必备份数据库文件
- 仅在生产前或测试环境执行回滚
- 级联回滚：如果回滚 v5，则 v6+ 也必须同步回滚

---

## 九、调试技巧

### 9.1 查看当前版本

```rust
let version = manager.get_current_version(&db_path)?;
println!("Current version: {}", version);
```

### 9.2 查看已应用版本

```rust
let versions = manager.get_applied_versions(&db_path)?;
for v in versions {
    println!("v{} - {} (applied at {})", v.version, v.name, v.applied_at);
}
```

### 9.3 查看待执行迁移

```rust
let pending = manager.validate(MigrationType::Global)?;
for m in pending {
    println!("Pending: v{} - {}", m.version, m.name);
}
```

---

## 十、最佳实践

1. **版本号连续**：不要跳号，便于追踪
2. **描述清晰**：文件名能表达迁移内容
3. **幂等性**：使用 `IF NOT EXISTS` 等安全语句
4. **小步迁移**：每次迁移只做一件事
5. **测试 SQL**：在对应数据库手动执行验证
6. **备份数据**：升级前备份数据库文件
7. **文档同步**：重大迁移更新本文档

---

## 十一、连接表字段演进历史

### 11.1 项目连接表 (connections)

| 版本 | 迁移文件                       | 变更内容            | 说明                                              |
| ---- | ------------------------------ | ------------------- | ------------------------------------------------- |
| v1.0 | 001_init.sql                   | 初始表结构          | 包含基础连接字段                                  |
| v1.1 | 002_refactor_query_history.sql | 重构 SQL 历史       | 将 sql_history 重构为 query_history，添加兼容视图 |
| v1.2 | 003_add_schema_name.sql        | 添加 schema_name    | 支持 PostgreSQL/Oracle 等多 Schema 数据库         |
| v1.3 | 004_add_duckdb_fed.sql         | 添加 use_duckdb_fed | 标记是否启用 DuckDB 联邦分析功能                  |
| v1.4 | 005_add_metadata_path.sql      | 添加 metadata_path  | 记录元数据缓存文件路径（相对于项目根目录）        |

### 11.2 全局连接表 (global_connections)

| 版本 | 迁移文件                  | 变更内容            | 说明                                         |
| ---- | ------------------------- | ------------------- | -------------------------------------------- |
| v1.0 | 001_init.sql              | 初始表结构          | 包含基础连接字段                             |
| v1.1 | 002_add_schema_name.sql   | 添加 schema_name    | 支持 PostgreSQL/Oracle 等多 Schema 数据库    |
| v1.2 | 003_add_duckdb_fed.sql    | 添加 use_duckdb_fed | 标记是否启用 DuckDB 联邦分析功能             |
| v1.3 | 004_add_metadata_path.sql | 添加 metadata_path  | 记录元数据缓存文件路径（相对于全局数据目录） |

### 11.3 路径语义说明

`metadata_path` 字段存储的是**相对路径**，但基准目录不同：

| 连接类型     | 存储路径示例                    | 相对基准                                      |
| ------------ | ------------------------------- | --------------------------------------------- |
| **项目连接** | `project_metadata/{conn_id}.db` | 相对于项目根目录 `.RSMETA/`                   |
| **全局连接** | `metadata/{conn_id}.db`         | 相对于全局数据目录 `{data_dir}/RdataStation/` |

### 11.4 关键字段说明

#### schema_name

- **用途**：指定默认 Schema 名
- **适用数据库**：PostgreSQL、Oracle、SQL Server 等多 Schema 数据库
- **示例值**：`public`、`dbo`、`hr`

#### use_duckdb_fed

- **用途**：标记是否启用 DuckDB 联邦分析功能
- **类型**：BOOLEAN (0/1)
- **默认值**：0 (不启用)
- **说明**：启用后，该连接的数据可通过 DuckDB 进行联邦查询和分析

#### metadata_path

- **用途**：记录元数据缓存文件路径
- **类型**：TEXT (可选)
- **说明**：用于缓存数据库对象的元数据（表结构、列信息等），提升加载速度

---

## 十二、迁移系统工作流程

### 12.1 新项目创建流程

```
1. 创建项目目录结构
   ↓
2. 初始化项目 SQLite (project_meta/001_init.sql)
   → 创建 connections 表（包含所有最新字段）
   → 创建 query_history 表
   → 创建 sql_history 兼容视图
   ↓
3. 初始化项目 DuckDB (project_analysis/001_init.sql)
   ↓
4. 记录初始版本到 schema_version 表
```

### 12.2 旧项目升级流程

```
1. 打开项目时，读取 schema_version 表获取当前版本
   ↓
2. 对比嵌入的迁移文件，找出未执行的版本
   ↓
3. 按版本号升序执行增量迁移
   → 002: 重构 SQL 历史表
   → 003: 添加 schema_name 字段
   → 004: 添加 use_duckdb_fed 字段
   → 005: 添加 metadata_path 字段
   ↓
4. 每步迁移在事务中执行，失败自动回滚
   ↓
5. 更新 schema_version 表记录新版本
```

### 12.3 迁移执行示例

```rust
use crate::core::migration::{MigrationManager, MigrationType};

let manager = MigrationManager::new();

// 执行项目元数据迁移
let applied = manager.migrate(
    &project_meta_db_path,
    MigrationType::ProjectMeta
)?;

if !applied.is_empty() {
    tracing::info!("Applied {} migrations", applied.len());
    for m in &applied {
        tracing::info!("  - v{}: {}", m.version, m.name);
    }
}
```

### 12.4 迁移文件命名规范

```
{三位版本号}_{描述性名称}.sql

示例：
001_init.sql                    # 初始表结构
002_refactor_query_history.sql  # 重构查询历史
003_add_schema_name.sql         # 添加 schema_name 字段
004_add_duckdb_fed.sql          # 添加 use_duckdb_fed 字段
005_add_metadata_path.sql       # 添加 metadata_path 字段
```

### 12.5 迁移文件编写规范

1. **使用幂等语句**：

   ```sql
   CREATE TABLE IF NOT EXISTS ...
   CREATE INDEX IF NOT EXISTS ...
   ```

2. **添加字段使用 ALTER TABLE**：

   ```sql
   ALTER TABLE connections ADD COLUMN schema_name TEXT;
   ALTER TABLE connections ADD COLUMN use_duckdb_fed BOOLEAN DEFAULT 0;
   ```

3. **添加索引优化查询**：

   ```sql
   CREATE INDEX IF NOT EXISTS idx_connections_duckdb_fed ON connections(use_duckdb_fed);
   ```

4. **数据迁移（如需要）**：

   ```sql
   -- 迁移旧数据到新表
   INSERT OR IGNORE INTO new_table (...)
   SELECT ... FROM old_table;
   ```

5. **创建兼容视图（保持向后兼容）**：
   ```sql
   CREATE VIEW IF NOT EXISTS sql_history AS
   SELECT id, connection_id, sql AS sql_text, ...
   FROM query_history;
   ```

---

## 十三、连接级元数据缓存版本历史

### 13.1 版本演进

| 版本 | 迁移文件                                  | 变更内容             | 说明                                                   |
| ---- | ----------------------------------------- | -------------------- | ------------------------------------------------------ |
| v1.0 | 001_init.sql                              | 初始统一表结构       | schemata/tables/columns/views/routines/indexes         |
| v2.0 | 002_add_cache_version_and_compression.sql | 版本控制与压缩       | 添加版本跟踪和 gzip 压缩支持                           |
| v3.0 | 003_add_fts_search.sql                    | FTS5 全文搜索        | 添加 fts_schemata/fts_tables/fts_columns 虚拟表        |
| v4.0 | 004_refactor_to_normalized.sql            | 规范化表结构         | 独立表：schemata/tables/columns/indexes/views/routines |
| v5.0 | 005_normalized_fts_and_cascade.sql        | FTS 同步与级联删除   | FTS 索引同步，删除 Schema 自动级联清理                 |
| v6.0 | 006_add_metadata_index.sql                | 索引表支持分页懒加载 | metadata_index 索引表，introspect_level 分级加载       |
| v7.0 | 007_incremental_sync.sql                  | 增量同步支持         | object_hash 字段，sync_snapshot 表，sync_operations 表 |

### 13.2 V7 增量同步架构

**设计目标**：

- 首次同步：全量预热
- 后续同步：仅同步变化对象
- 预热时间减少 90%+

**核心表结构**：

```sql
-- 同步快照表（记录上次同步状态）
CREATE TABLE IF NOT EXISTS sync_snapshot (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    connection_id TEXT NOT NULL,
    snapshot_type TEXT NOT NULL,      -- schema/table/column/index/view/routine/full
    object_type TEXT NOT NULL,
    object_name TEXT NOT NULL,
    parent_name TEXT,
    object_hash TEXT,
    snapshot_at INTEGER NOT NULL,
    UNIQUE (connection_id, object_type, object_name, parent_name)
);

-- 同步操作表（记录待同步操作）
CREATE TABLE IF NOT EXISTS sync_operations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    connection_id TEXT NOT NULL,
    operation_type TEXT NOT NULL,     -- create/update/delete/no_change
    object_type TEXT NOT NULL,
    object_name TEXT NOT NULL,
    parent_name TEXT,
    old_hash TEXT,
    new_hash TEXT,
    detected_at INTEGER NOT NULL,
    processed_at INTEGER,
    status TEXT DEFAULT 'pending',     -- pending/running/completed/failed
    priority INTEGER DEFAULT 5,
    error_message TEXT
);

-- 变更检测视图
CREATE VIEW IF NOT EXISTS v_schema_changes AS ...;
CREATE VIEW IF NOT EXISTS v_table_changes AS ...;
CREATE VIEW IF NOT EXISTS v_column_changes AS ...;
```

**Hash 计算**：

- 算法：SHA-256
- 输入：`object_type | name | parent | extra_data`
- 用途：快速检测对象是否发生变化

### 13.3 性能对比

| 指标     | V6 优化后 | V7 增量（首次） | V7 增量（后续） |
| -------- | --------- | --------------- | --------------- |
| 预热时间 | 150ms     | 150ms           | 15ms            |
| 相对提升 | -70%      | -70%            | -97%            |

### 13.4 与竞品对比

| 特性     | RdataStation V7 | DBeaver 24.x | DataGrip 2024.2 |
| -------- | --------------- | ------------ | --------------- |
| 增量同步 | ✅ 完整支持     | ⚠️ 部分支持  | ⚠️ 部分支持     |
| 首次预热 | 150ms 🏆        | 400ms        | 300ms           |
| 增量预热 | 15ms 🏆         | 350ms        | 250ms           |
| 内存占用 | 80MB 🏆         | 350MB        | 500MB           |

详见 [COMPARISON.md](../COMPARISON.md)
