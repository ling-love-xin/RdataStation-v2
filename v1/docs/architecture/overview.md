# RdataStation 项目级架构设计

> 最后更新：2026-04-24

## 核心架构原则

### 1. 四层数据库架构

RdataStation 采用四层数据库架构，确保系统级、项目级、连接级数据严格隔离：

```
┌─────────────────────────────────────────────────────────┐
│  系统级数据库 (System Level)                              │
│  位置：{data_dir}/RdataStation/system/                   │
│  ├─ global.db (SQLite 连接池)                            │
│  │   ├─ 全局连接信息 (不跟随项目)                          │
│  │   ├─ 全局设置 (主题、快捷键等)                          │
│  │   └─ 最近连接记录                                      │
│  ├─ global_metadata/ (每个连接独立文件)                   │
│  │   ├─ conn_oracle_001.sqlite (可能 500MB)              │
│  │   └─ conn_mysql_002.sqlite (可能 50MB)                │
│  └─ analytics.duckdb (DuckDB 长连接)                      │
│      ├─ 查询缓存                                         │
│      └─ 全局分析数据                                      │
├─────────────────────────────────────────────────────────┤
│  项目级数据库 (Project Level)                             │
│  位置：{project_path}/                                   │
│  ├─ meta/                                                │
│  │   ├─ project.db (SQLite 连接池)                       │
│  │   │   ├─ 项目连接信息 (跟随项目)                        │
│  │   │   ├─ SQL 历史                                     │
│  │   │   └─ 项目设置                                     │
│  │   └─ connection_metadata/ (每个连接独立文件)           │
│  │       ├─ conn_pg_001.sqlite                           │
│  │       └─ conn_sqlite_002.sqlite                       │
│  └─ analytics/                                           │
│      └─ data.duckdb (DuckDB 长连接)                       │
│          ├─ 项目分析数据                                  │
│          └─ 持久化结果集                                  │
├─────────────────────────────────────────────────────────┤
│  连接级数据库 (Connection Level)                          │
│  位置：用户指定的数据库服务器                              │
│  ├─ MySQL/PostgreSQL/Oracle 等                           │
│  └─ 通过驱动连接，不存储本地                              │
└─────────────────────────────────────────────────────────┘
```

### 2. 项目作为核心组织单元

```
Project（项目）
├── meta/
│   ├── project.db              # SQLite 事务数据库
│   └── connection_metadata/    # 连接元数据缓存
│       └── conn_{id}.sqlite    # 每个连接独立文件
├── analytics/
│   └── data.duckdb             # DuckDB 分析数据库
└── config/
    ├── connections.json        # 连接配置
    ├── settings.json           # 项目设置
    └── layout.json             # 工作台布局
```

### 3. 数据分层

| 类型           | 存储                | 用途                          | 示例                     |
| -------------- | ------------------- | ----------------------------- | ------------------------ |
| **事务性**     | SQLite              | 连接信息、SQL历史、项目元数据 | connections, sql_history |
| **分析性**     | DuckDB              | 查询结果、数据分析、报表      | query_results, analytics |
| **配置性**     | JSON文件            | 用户偏好、编辑器设置          | settings.json            |
| **元数据缓存** | SQLite (每连接独立) | 表结构、列信息、索引          | tables, columns, indexes |

### 4. 动态渲染架构

```
Workbench（工作台）
├── LeftSidebar（动态导航）
│   └── DatabaseNavigator（基于当前连接动态渲染）
├── CenterArea（动态面板）
│   ├── SQLEditorPanel（SQL编辑器）
│   ├── TableDataPanel（表数据）
│   └── QueryResultPanel（查询结果）
└── RightSidebar（动态工具）
    └── PropertiesPanel（属性面板）
```

### 5. 状态管理架构

```
ProjectStore（项目级）
├── currentProject: Project
├── connections: Connection[]
├── activeConnection: Connection | null
└── workbenchState: WorkbenchState

ConnectionStore（连接级）
├── connections: Map<connId, Connection>
├── activeConnectionId: string | null
└── connectionStates: Map<connId, ConnectionState>

WorkbenchStore（工作台级）
├── panels: Panel[]
├── activePanelId: string | null
├── editorState: EditorState
└── queryResults: QueryResult[]
```

### 6. 本地持久化策略

#### 系统级 SQLite 数据库结构

```sql
-- 全局连接信息表
CREATE TABLE global_connections (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    driver TEXT NOT NULL,
    host TEXT,
    port INTEGER,
    database TEXT,
    username TEXT,
    password_encrypted TEXT,
    options TEXT,
    is_active BOOLEAN DEFAULT 1,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- 全局设置表
CREATE TABLE global_settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- 最近连接表
CREATE TABLE recent_connections (
    id TEXT PRIMARY KEY,
    connection_id TEXT NOT NULL,
    last_used TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    usage_count INTEGER DEFAULT 1
);
```

#### 项目级 SQLite 数据库结构

```sql
-- 连接信息表
CREATE TABLE connections (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    driver TEXT NOT NULL,
    host TEXT,
    port INTEGER,
    database TEXT,
    username TEXT,
    password_encrypted TEXT,
    options JSON,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- SQL历史表
CREATE TABLE sql_history (
    id TEXT PRIMARY KEY,
    connection_id TEXT,
    sql_text TEXT NOT NULL,
    execution_time_ms INTEGER,
    rows_affected INTEGER,
    error_message TEXT,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (connection_id) REFERENCES connections(id)
);

-- 项目设置表
CREATE TABLE project_settings (
    key TEXT PRIMARY KEY,
    value JSON NOT NULL,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
```

#### 连接级元数据缓存结构

```sql
-- 表信息
CREATE TABLE tables (
    id TEXT PRIMARY KEY,
    conn_id TEXT NOT NULL,
    schema_name TEXT,
    table_name TEXT NOT NULL,
    table_type TEXT,
    comment TEXT,
    row_count INTEGER,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- 列信息
CREATE TABLE columns (
    id TEXT PRIMARY KEY,
    conn_id TEXT NOT NULL,
    schema_name TEXT,
    table_name TEXT NOT NULL,
    column_name TEXT NOT NULL,
    data_type TEXT,
    is_nullable BOOLEAN,
    column_default TEXT,
    comment TEXT,
    ordinal_position INTEGER,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- FTS5 全文搜索索引
CREATE VIRTUAL TABLE metadata_fts USING fts5(
    schema_name,
    table_name,
    column_name,
    comment,
    content='tables'
);
```

#### DuckDB 数据库结构（分析性）

```sql
-- 查询结果缓存表
CREATE TABLE query_results (
    id TEXT PRIMARY KEY,
    query_id TEXT,
    sql_hash TEXT,
    result_data BLOB,
    row_count INTEGER,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- 数据分析表
CREATE TABLE analytics (
    id TEXT PRIMARY KEY,
    analysis_type TEXT,
    source_connection TEXT,
    source_table TEXT,
    result_data BLOB,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
```

### 7. 动态渲染流程

```
1. 用户选择项目
   ↓
2. 加载项目配置（SQLite）
   ↓
3. 渲染项目工作台
   ↓
4. 用户选择连接
   ↓
5. 动态加载数据库导航（基于连接类型）
   ↓
6. 用户选择表/执行SQL
   ↓
7. 动态渲染对应面板（表数据/查询结果）
   ↓
8. 分析数据存储到 DuckDB
```

### 8. 组件动态渲染设计

#### DatabaseNavigator（动态导航）

```typescript
interface NavigatorNode {
  id: string
  type: 'connection' | 'database' | 'schema' | 'table' | 'view' | 'column'
  name: string
  icon: string
  children?: NavigatorNode[]
  metadata?: Record<string, any>
  actions?: NavigatorAction[]
}

// 根据连接类型动态渲染不同结构
// MySQL: connection -> database -> table -> column
// PostgreSQL: connection -> database -> schema -> table -> column
// SQLite: connection -> table -> column
```

#### WorkbenchPanel（动态面板）

```typescript
interface Panel {
  id: string
  type: 'sql-editor' | 'table-data' | 'query-result' | 'properties'
  title: string
  component: Component
  props: Record<string, any>
  state: PanelState
}

// 根据用户操作动态添加/移除面板
// 双击表 -> 添加 TableDataPanel
// 执行SQL -> 添加 QueryResultPanel
// 点击属性 -> 添加 PropertiesPanel
```

### 9. 文件组织

```
系统级 (system/)
├── global.db              # SQLite 连接池
├── global_metadata/       # 全局连接元数据缓存
│   └── conn_{id}.sqlite   # 每个连接独立文件
└── analytics.duckdb       # DuckDB 长连接

项目级 (project/)
├── meta/
│   ├── project.db         # 项目 SQLite 连接池
│   └── connection_metadata/
│       └── conn_{id}.sqlite  # 项目连接元数据缓存
├── analytics/
│   └── data.duckdb        # 项目 DuckDB 长连接
└── config/
    ├── connections.json        # 连接配置
    ├── settings.json           # 项目设置
    └── layout.json             # 工作台布局
```

### 10. 数据流设计

```
User Action
    ↓
Vue Component
    ↓
Pinia Store
    ↓
Tauri Command
    ↓
Rust Core
    ↓
SQLite/DuckDB
    ↓
File System
```

### 11. 关键设计决策

1. **四层架构隔离**: 系统级、项目级、连接级数据严格分离
2. **元数据跟随连接信息**: 项目迁移时只需复制项目目录
3. **每个连接独立元数据文件**: 大型数据库元数据不影响其他连接
4. **懒加载**: 数据库导航按需加载，不一次性加载所有元数据
5. **状态同步**: 前端状态与本地数据库保持同步
6. **分析分离**: 事务性数据和分析性数据物理分离
7. **动态渲染**: 所有功能面板根据上下文动态创建和销毁
8. **连接池 + 长连接**: SQLite 用连接池支持并发，DuckDB 用长连接保持分析状态
