# 数据流设计

> 版本：v2.3
> 最后更新：2026-05-28
> 状态：✅ 编辑流程实现 / url_template 支持 / recent_connections 字段补齐

## 概述

本文档描述 RdataStation 后端的数据流，包括请求处理流程、元数据查询流程、错误传播机制等。

## 一、SQL 执行流程（实际）

```
Frontend (Vue3)
    │
    │ invoke('execute_sql', { connId: "conn_123", sql: "SELECT * FROM users" })
    ▼
Tauri Runtime
    │
    ▼
commands/sql_commands.rs
    │ 1. 输入校验（SQL 非空检查）
    │ 2. 获取 ConnectionManager
    │ 3. manager.get_connection(conn_id)
    │    → Option<DynDatabase>
    ▼
ConnectionManager
    │ 返回 Arc<dyn Database>
    ▼
commands/sql_commands.rs
    │ 4. db.query(sql)
    │ 5. 记录 SQL 历史（history_store）
    ▼
driver/native/{mysql,postgres,sqlite,duckdb}.rs
    │ Database::query(sql)
    │ → Pool::acquire() → 执行 SQL
    │ → 行数据 → Vec<Value> → QueryResult
    ▼
commands/sql_commands.rs
    │ 返回 QueryResult（自动序列化为 JSON）
    ▼
Frontend (Vue3)
```

### 实际代码路径

| 步骤     | 文件                                                                                                                                       |
| -------- | ------------------------------------------------------------------------------------------------------------------------------------------ |
| 命令入口 | [commands/sql_commands.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/commands/sql_commands.rs)                  |
| 连接获取 | [services/connection_manager.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/services/connection_manager.rs) |
| SQL 执行 | [sql_service.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/services/sql_service.rs)                        |
| 驱动实现 | [driver/native/](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/driver/native/)                                 |

## 二、连接创建流程（实际）

```
Frontend (Vue3)
    │ invoke('create_connection', { config })
    ▼
commands/connection_commands.rs
    │
    ▼
ConnectionService::connect(config)
    │
    ├── 1. 解析连接 URL（parse_url）
    ├── 2. create_database(db_type, url)
    │       └── match db_type {  ← P0: 硬编码 4 种匹配
    │             "mysql" => MySqlDatabase::new(url)
    │             "postgres" => PostgresDatabase::new(url)
    │             ...
    │           }
    ├── 3. 注册到 ConnectionManager（add_connection）
    ├── 4. 保存连接信息到全局/项目持久化存储
    └── 5. 返回 conn_id + DataSourceMeta
```

### P0 问题：create_database 硬编码

**路径**: [connection_service.rs#L256](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/services/connection_service.rs#L256)

当前 `ConnectionService::create_database()` 绕过了 `DataSourceRouter` 和 `DriverRegistry`，直接硬编码匹配 4 种数据库类型。

**改进后流程**：

```
ConnectionService::connect(config)
    │
    ▼
DataSourceRouter::route(config)
    │ DriverRegistry::get(config.driver)
    ▼
DriverFactory::create(config)
    │ 工厂负责创建具体数据库实例
    ▼
DynDatabase
```

## 三、Schema 浏览流程

```
Frontend (Navigator Panel)
    │ 用户展开树节点
    ▼
Tauri Command
    │ get_connection(conn_id) → DynDatabase
    ▼
Database trait 方法
    │
    ├── list_databases()  → Vec<String>
    ├── list_schemas(db)  → Vec<String>
    ├── list_tables(db, schema) → Vec<SchemaObject { name, kind, children: None }>
    └── list_columns(db, schema, table) → Vec<SchemaObject { name, kind, children: None }>
```

> ⚠️ **P0-4**：`SchemaObject` 只有 `name`/`kind`/`children`，缺少列类型/注释等详情。

### 元数据缓存流程

```
首次请求:
    frontend → list_tables
        → MetadataCacheManager::get_tables(conn_id, db, schema)
        → 缓存未命中 → 执行实际 SQL → 写入缓存 → 返回

后续请求（缓存命中）:
    frontend → list_tables
        → MetadataCacheManager::get_tables(conn_id, db, schema)
        → 缓存命中 → 直接返回（无需查询数据库）

缓存预热:
    frontend → start_cache_warming(conn_id)
        → 后台任务：遍历所有 schema → 预取 tables + columns
        → 前端轮询 get_warming_progress
```

## 四、双层存储读写流程

### 驱动属性读取流程（v0.5.3+）

```
前端 (AddDataSourceDialog)
    │ 打开新增数据源对话框 → 选择驱动类型
    ▼
Tauri Command
    │ get_drivers_by_type(type_id) 或 get_driver(driver_id)
    ▼
persistence::driver_store
    │ SELECT ... driver_properties FROM drivers
    ▼
Driver { driver_properties: Option<String> }
    │ 序列化为 JSON → 返回前端
    ▼
DriverPropsTab
    │ JSON.parse(driver_properties) → 初始填充键值对表单
    │ 用户可修改/添加/删除属性
    ▼
保存时 driver_properties 写入 global_connections 或 connections 表
```

> `driver_properties` 是 JSON 键值对字符串（`{"key":"value"}`），前端 DriverPropsTab 解析后作为连接属性的默认值。

### 连接信息存储（SQLite）

```
创建连接
    │
    ▼
ConnectionService::connect()
    │
    ├── ConnectionStore::save(conn_info)
    │   └── INSERT INTO connections (global.db)
    │
    └── ProjectConnectionStore::save(conn_info)
        └── INSERT INTO connections (project.db)
```

### 编辑已有连接流程（v0.6.1+）

```
DataSourceSidebar (前端)
    │ 点击"编辑连接"按钮 → editSavedConnection(conn)
    ▼
dispatchWorkbenchEvent(NewConnection, { connection: ProjectConnection })
    ▼
WorkbenchView::handleWorkbenchNewConnection(e)
    │ 检测 e.detail.connection → 设置 dialogInitialConnection
    ▼
AddDataSourceDialog
    │ watch modelValue → initFromConnection(conn)
    │   ├── 回填 name, description, driver_id
    │   ├── 回填 formData (host, port, database, username, password)
    │   ├── 回填 authConfigId, authMethod, networkConfigId
    │   ├── 回填 driverProperties, advancedOptions, environmentId
    │   └── 回填 schemaName, options, metadataPath, tags, useDuckdbFed
    ▼
用户修改表单字段 → 点击"应用"
    │ handleApply() 保存更新后的连接配置
    ▼
projectConnectionStore → updateProjectConnection()
    │ invoke('update_project_connection', { projectPath, connection })
    ▼
Rust 后端 → ProjectConnectionStore::update_connection()
    │ UPDATE connections SET ... WHERE id = ?
    ▼
connectDatabase() → 重新建立运行时连接
```

> 编辑流程复用新增数据源对话框，通过 `initialConnection` prop 传递已有 `ProjectConnection` 数据实现表单回填。

### URL 模板支持（url_template）

```
Driver 配置
    │ url_template: "{driver}://{host}:{port}/{database}"
    ▼
useUrlBuilder (前端)
    │ applyTemplate(template, formData)
    │   .replace("{host}", host)
    │   .replace("{port}", port)
    │   .replace("{database}", database)
    │   .replace("{username}", username)
    │   .replace("{password}", password)
    │   .replace("{file_path}", filePath)
    │   .replace("{driver}", driverName)
    ▼
uriPreview (显示用, 密码脱敏为 ****)
buildUrl (连接用, 真实密码)
```

> 驱动表 `drivers` 包含 `url_template` 字段，优先使用模板构建连接 URL，回退到硬编码模式。

### 分析查询（DuckDB 加速）

```
execute_duckdb_accelerated(sql)
    │
    ▼
DuckDB 分析引擎
    │
    ├── ATTACH 'mysql://...' AS mysql_db
    ├── ATTACH 'postgres://...' AS pg_db
    │
    ├── SELECT * FROM mysql_db.orders
    │   JOIN pg_db.users ON ...
    │   (DuckDB 自动优化 + 谓词下推)
    │
    └── 返回 QueryResult
```

## 五、启动流程

```
main.rs → lib.rs::run()
    │
    ├── 1. register_drivers()
    │       AutoDriverRegistrar::auto_register()
    │       → BuiltinDriverDiscovery::builtin_factories()  // 唯一真相源 × 6
    │       → DriverRegistry::register_by_factory(id, factory) × 6
    │
    ├── 2. initialize_global_system()
    │       → 创建 system/global.db (SQLite)
    │       → 创建 system/analytics/global.duckdb (DuckDB)
    │       → 执行 MigrationType::GlobalSqlite
    │       → 执行 MigrationType::GlobalDuckDB
    │
    ├── 3. init_driver_manager()
    │       → BuiltinDriverDiscovery::builtin_factories()  // 同一真相源 × 6
    │       → DriverManager::register_driver(id, factory) × 6
    │
    └── 4. Tauri Builder
            → manage(ProjectState)
            → manage(AnalyticsResourceState)
            → manage(ScratchpadState)
            → invoke_handler(70+ commands)
            → run()
```

## 六、错误传播机制

```
driver/native/{db}.rs
    │ sqlx::Error / rusqlite::Error
    ▼
    ? (自动转换)
    ▼
CoreError
    ├── CoreError::Connection(ConnectionError::Refused { ... })
    ├── CoreError::Database(DatabaseError::QueryError { ... })
    └── CoreError::Common(CommonError::General { ... })
    ▼
Tauri Command
    │ Result<T, CoreError>
    │ Tauri 自动序列化为 JSON ErrorResponse
    ▼
Frontend
    │ { code: "CONN_REFUSED", message: "..." }
```

### 错误类型层次

```
CoreError
├── ConnectionError      # 连接相关错误
│   ├── Refused         # 连接被拒绝
│   ├── Timeout         # 连接超时
│   ├── InvalidConfig   # 配置无效
│   ├── DriverNotFound  # 驱动未找到
│   └── Network         # 网络错误
├── DatabaseError        # 数据库相关错误
│   ├── QueryError      # 查询错误
│   ├── Driver          # 驱动错误
│   ├── ConstraintViolation  # 约束冲突
│   └── SyntaxError     # 语法错误
└── CommonError          # 通用错误
    ├── General         # 一般错误
    ├── NotSupported    # 不支持的操作
    └── ValidationError # 校验错误
```

## 七、最近连接记录（recent_connections）

### 数据模型（v0.6.1+）

```
ConnectionRecord / RecentConnectionResponse
├── name: String                # 连接名称
├── db_type: String             # 数据库类型
├── url: String                 # 连接 URL（密码脱敏）
├── last_used_at: DateTime      # 最后使用时间
├── description: Option<String> # 连接描述
├── driver_id: Option<String>   # 驱动实例 ID
├── environment_id: Option<String>  # 环境 ID
├── auth_config_id: Option<String>  # 认证配置 ID
├── auth_method: Option<String> # 认证方式 (password/ldap/kerberos/...)
├── network_config_id: Option<String>  # 网络配置 ID
├── driver_properties: Option<String>  # 驱动属性 JSON
└── advanced_options: Option<String>   # 高级选项 JSON
```

### 写入流程

```
ConnectionService::connect()
    │ 创建连接成功后
    ▼
connection_store::save_recent_connection(RecentConnectionInput {
    name, db_type, url, conn_id,
    description, driver_id, environment_id,
    auth_config_id, auth_method,    // ← v0.6.1 补齐
    network_config_id,
    driver_properties,               // ← v0.6.1 补齐
    advanced_options,                // ← v0.6.1 补齐
})
    ▼
GLOBAL_STORE (Lazy<Mutex<ConnectionStore>>)
    │ add_connection(ConnectionInfo) → save() → JSON 文件
    ▼
recent_connections.json
```

### 读取流程

```
Frontend invoke('get_recent_connections')
    ▼
connection_store::get_recent_connections()
    │ ConnectionInfo → ConnectionRecord
    │   (auth_method, driver_properties, advanced_options 透传)
    ▼
RecentConnectionResponse → JSON → 前端
```

> `connection_store::ConnectionInfo` 新增 `auth_method` 字段（v0.6.1），避免 `RecentConnectionInput` 中的 `auth_method` 被静默丢弃。

## 八、关键约束

| 约束                | 说明                                                       |
| ------------------- | ---------------------------------------------------------- |
| ❌ 禁止跨层调用     | Command 不能直接调用 driver/native                         |
| ❌ 禁止 unwrap      | 所有生产代码使用 `?` 操作符                                |
| ✅ QueryResult 格式 | 必须包含 `columns: Vec<String>` 和 `rows: Vec<Vec<Value>>` |
| ✅ services 层      | 只调用 connection_manager / driver，不碰 driver/native     |
| ✅ Arrow IPC        | 插件通信使用 Arrow RecordBatch                             |
