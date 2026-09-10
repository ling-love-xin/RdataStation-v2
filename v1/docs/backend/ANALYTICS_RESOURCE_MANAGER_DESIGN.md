# 分析资源管理器 设计方案

## 一、整体架构

```
┌─────────────────────────────────────────────────────────┐
│                    前端 UI 层                            │
│  ┌───────────────────────────────────────────────────┐ │
│  │ AnalyticsResourceManager.vue                     │ │
│  ├── ResourceSearchBar.vue                         │ │
│  ├── ResourceTagFilter.vue (标签矩阵)             │ │
│  ├── ResourceTree.vue (文件夹 + 列表)            │ │
│  ├── ResourceDetailPanel.vue (详情面板)           │ │
│  └── BottomActionBar.vue (底部操作栏)             │ │
│  └───────────────────────────────────────────────────┘ │
└──────────────────────┬──────────────────────────────────┘
                       │ Tauri Command
┌──────────────────────▼──────────────────────────────────┐
│              Rust 核心层 (Core)                          │
│  ┌───────────────────────────────────────────────────┐ │
│  │ AnalyticsResourceService                        │ │
│  │  - 查询资源（支持标签过滤）                      │ │
│  │  - 创建文件夹                                    │ │
│  │  - 资源生命周期管理（持久化/提升/删除）         │ │
│  │  - 导入/导出文件                                │ │
│  │  - 回收站管理                                   │ │
│  │  - 依赖关系追踪                                 │ │
│  └───────────────────────────────────────────────────┘ │
│  ┌───────────────────────────────────────────────────┐ │
│  │ DuckDBEngine (扩展)                             │ │
│  │  - 临时表管理                                    │ │
│  │  - 持久表管理                                    │ │
│  │  - 全局表管理（含版本控制）                     │ │
│  │  - 懒加载支持                                    │ │
│  └───────────────────────────────────────────────────┘ │
└──────────────────────┬──────────────────────────────────┘
                       │
┌──────────────────────▼──────────────────────────────────┐
│              存储层 (Persistence)                       │
│  ┌──────────────────┐  ┌─────────────────────────────┐ │
│  │ project.db       │  │ global.db                   │ │
│  │ (项目级数据)     │  │ (应用级数据)                │ │
│  │ - analytics_    │  │ - analytics_               │ │
│  │   resources      │  │   resources                 │ │
│  │ - folders        │  │ - folders                   │ │
│  │ - folder_items   │  │ - folder_items              │ │
│  │ - resource_      │  │ - resource_                 │ │
│  │   references     │  │   references                │ │
│  │ - recycle_bin    │  │ - recycle_bin               │ │
│  └──────────────────┘  └─────────────────────────────┘ │
│  ┌──────────────────┐  ┌─────────────────────────────┐ │
│  │ project.duckdb   │  │ global.duckdb               │ │
│  │ (项目DuckDB)     │  │ (应用DuckDB)                │ │
│  │ - persist.*      │  │ - global.*                   │ │
│  └──────────────────┘  └─────────────────────────────┘ │
│  ┌──────────────────┐  ┌─────────────────────────────┐ │
│  │ project_files/   │  │ ~/.rdatastation/            │ │
│  │ (项目文件)       │  │ shared-files/               │ │
│  └──────────────────┘  └─────────────────────────────┘ │
└─────────────────────────────────────────────────────────┘
```

---

## 二、数据库模型设计

### 2.1 project.db（项目级数据库）

```sql
-- 分析资源表
CREATE TABLE analytics_resources (
    id TEXT PRIMARY KEY,
    resource_type TEXT NOT NULL,  -- 'connection' | 'table' | 'file'
    name TEXT NOT NULL,
    alias TEXT,                     -- 用户自定义别名
    -- 资源特定字段（JSON 存储）
    config TEXT NOT NULL,  -- JSON: {connection_id, table_name, file_path, ...}
    -- 层级标签
    scope TEXT NOT NULL,  -- 'application' | 'project' | 'session'
    -- 统计信息
    row_count INTEGER,
    column_count INTEGER,
    file_size INTEGER,
    -- 版本信息（用于全局表）
    version INTEGER DEFAULT 1,
    parent_version_id TEXT,        -- 上一版本 ID
    -- 元数据
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    created_by TEXT,
    deleted_at TIMESTAMP,          -- 软删除时间
    -- 依赖信息
    parent_resource_id TEXT,
    source_query TEXT,             -- 创建时使用的查询
    FOREIGN KEY (parent_resource_id) REFERENCES analytics_resources(id)
);

-- 文件夹表
CREATE TABLE folders (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    parent_folder_id TEXT,
    description TEXT,
    sort_order INTEGER DEFAULT 0,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (parent_folder_id) REFERENCES folders(id)
);

-- 文件夹-资源关联表（多对多）
CREATE TABLE folder_items (
    folder_id TEXT NOT NULL,
    resource_id TEXT NOT NULL,
    sort_order INTEGER DEFAULT 0,
    added_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (folder_id, resource_id),
    FOREIGN KEY (folder_id) REFERENCES folders(id) ON DELETE CASCADE,
    FOREIGN KEY (resource_id) REFERENCES analytics_resources(id) ON DELETE CASCADE
);

-- 资源引用历史（用于依赖追踪）
CREATE TABLE resource_references (
    id TEXT PRIMARY KEY,
    from_resource_id TEXT NOT NULL,
    to_resource_id TEXT NOT NULL,
    reference_type TEXT NOT NULL,  -- 'read_csv' | 'insert_into' | 'join' | 'create_table_as'
    reference_context TEXT,         -- SQL 片段上下文
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (from_resource_id) REFERENCES analytics_resources(id) ON DELETE CASCADE,
    FOREIGN KEY (to_resource_id) REFERENCES analytics_resources(id) ON DELETE CASCADE
);

-- 回收站表
CREATE TABLE recycle_bin (
    id TEXT PRIMARY KEY,
    resource_id TEXT NOT NULL,
    resource_data TEXT NOT NULL,    -- 完整的资源 JSON
    deleted_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    expires_at TIMESTAMP NOT NULL,  -- 过期时间（30天后）
    FOREIGN KEY (resource_id) REFERENCES analytics_resources(id) ON DELETE CASCADE
);

-- 操作审计表
CREATE TABLE audit_log (
    id TEXT PRIMARY KEY,
    resource_id TEXT NOT NULL,
    action TEXT NOT NULL,  -- 'create' | 'update' | 'delete' | 'promote' | 'persist'
    actor TEXT,
    details TEXT,          -- JSON: 变更详情
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (resource_id) REFERENCES analytics_resources(id)
);
```

### 2.2 global.db（应用级数据库）

结构与 project.db 相同，主要存储：

- 全局连接模板
- DuckDB 全局表
- 全局共享文件
- 全局文件夹

---

## 三、资源类型定义

```rust
/// 资源类型
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ResourceType {
    Connection(ConnectionConfig),
    Table(TableConfig),
    File(FileConfig),
}

/// 连接资源
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionConfig {
    pub connection_id: String,
    pub db_type: String,
}

/// 表资源
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableConfig {
    pub table_name: String,
    pub duckdb_schema: String,  // "temp" | "persist" | "global"
    pub row_count: Option<usize>,
    pub column_count: Option<usize>,
    pub columns: Option<Vec<ColumnInfo>>,
    pub parent_resource_id: Option<String>,
    pub source_query: Option<String>,
}

/// 文件资源
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileConfig {
    pub file_path: String,
    pub file_name: String,
    pub file_size: Option<usize>,
    pub file_type: FileType,
    pub storage_location: StorageLocation,
    pub encoding: Option<String>,
}

/// 资源作用域
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ResourceScope {
    Application,
    Project { project_id: String },
    Session { session_id: String },
}

/// 资源完整结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsResource {
    pub id: String,
    pub name: String,
    pub alias: Option<String>,
    pub resource_type: ResourceType,
    pub scope: ResourceScope,
    pub version: i32,
    pub parent_version_id: Option<String>,
    pub stats: ResourceStats,
    pub folder_ids: Vec<String>,          // 所属文件夹
    pub reference_count: usize,           // 被引用次数
    pub created_at: DateTime,
    pub updated_at: DateTime,
    pub deleted_at: Option<DateTime>,
}

/// 资源统计
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceStats {
    pub row_count: Option<usize>,
    pub column_count: Option<usize>,
    pub file_size: Option<usize>,
}
```

---

## 四、Rust 核心接口设计

### 4.1 AnalyticsResourceService trait

```rust
#[async_trait]
pub trait AnalyticsResourceService: Send + Sync {
    // ========== 资源查询 ==========

    /// 获取资源列表（支持标签过滤）
    async fn get_resources(&self, filters: ResourceFilters) -> Result<Vec<AnalyticsResource>, CoreError>;

    /// 获取资源详情
    async fn get_resource(&self, resource_id: &str) -> Result<AnalyticsResource, CoreError>;

    /// 获取文件夹下的资源
    async fn get_folder_resources(&self, folder_id: &str) -> Result<Vec<AnalyticsResource>, CoreError>;

    /// 搜索资源
    async fn search_resources(&self, query: &str, filters: ResourceFilters) -> Result<Vec<AnalyticsResource>, CoreError>;

    // ========== 资源创建 ==========

    /// 从结果集创建临时表
    async fn create_temp_table(
        &self,
        result: QueryResult,
        name: &str,
        session_id: &str,
    ) -> Result<AnalyticsResource, CoreError>;

    /// 从连接提取表到分析区
    async fn extract_table(
        &self,
        connection_id: &str,
        schema: &str,
        table: &str,
        target_scope: ResourceScope,
    ) -> Result<AnalyticsResource, CoreError>;

    /// 导入文件
    async fn import_file(
        &self,
        source_path: &str,
        import_mode: FileImportMode,
    ) -> Result<AnalyticsResource, CoreError>;

    // ========== 资源生命周期管理 ==========

    /// 持久化临时表
    async fn persist_temp_table(
        &self,
        temp_resource_id: &str,
        name: &str,
    ) -> Result<AnalyticsResource, CoreError>;

    /// 提升为全局（自动创建新版本）
    async fn promote_to_global(&self, resource_id: &str) -> Result<AnalyticsResource, CoreError>;

    /// 获取资源版本历史
    async fn get_resource_versions(&self, resource_id: &str) -> Result<Vec<AnalyticsResource>, CoreError>;

    /// 重命名资源
    async fn rename_resource(&self, resource_id: &str, new_name: &str) -> Result<AnalyticsResource, CoreError>;

    /// 删除资源（放入回收站）
    async fn delete_resource(&self, resource_id: &str) -> Result<(), CoreError>;

    /// 清理会话临时资源
    async fn cleanup_session_resources(&self, session_id: &str) -> Result<usize, CoreError>;

    // ========== 文件夹管理 ==========

    /// 创建文件夹
    async fn create_folder(&self, name: &str, parent_id: Option<&str>) -> Result<Folder, CoreError>;

    /// 重命名文件夹
    async fn rename_folder(&self, folder_id: &str, new_name: &str) -> Result<Folder, CoreError>;

    /// 删除文件夹
    async fn delete_folder(&self, folder_id: &str) -> Result<(), CoreError>;

    /// 获取文件夹列表
    async fn get_folders(&self) -> Result<Vec<Folder>, CoreError>;

    /// 添加资源到文件夹
    async fn add_to_folder(&self, folder_id: &str, resource_id: &str) -> Result<(), CoreError>;

    /// 从文件夹移除资源
    async fn remove_from_folder(&self, folder_id: &str, resource_id: &str) -> Result<(), CoreError>;

    // ========== 回收站 ==========

    /// 获取回收站资源
    async fn get_recycle_bin(&self) -> Result<Vec<RecycleBinItem>, CoreError>;

    /// 恢复资源
    async fn restore_resource(&self, recycle_id: &str) -> Result<AnalyticsResource, CoreError>;

    /// 彻底删除
    async fn permanent_delete(&self, recycle_id: &str) -> Result<(), CoreError>;

    /// 清空回收站
    async fn empty_recycle_bin(&self) -> Result<usize, CoreError>;

    /// 清理过期资源（定时任务）
    async fn cleanup_expired(&self) -> Result<usize, CoreError>;

    // ========== 依赖追踪 ==========

    /// 获取资源依赖图
    async fn get_dependency_graph(&self, resource_id: &str) -> Result<DependencyGraph, CoreError>;

    /// 检查删除是否安全
    async fn check_delete_safe(&self, resource_id: &str) -> Result<DeleteCheckResult, CoreError>;

    // ========== SQL 引用生成 ==========

    /// 生成 SQL 引用片段
    fn generate_sql_reference(&self, resource: &AnalyticsResource) -> String;
}
```

### 4.2 DuckDBEngine 扩展

```rust
/// DuckDB 资源管理能力
#[async_trait]
pub trait DuckDBResourceManager: Send + Sync {
    // 临时表管理
    async fn create_temp_table(&self, result: QueryResult, name: &str) -> Result<String, CoreError>;
    async fn list_temp_tables(&self) -> Result<Vec<TempTableInfo>, CoreError>;
    async fn drop_temp_table(&self, name: &str) -> Result<(), CoreError>;
    async fn cleanup_temp_tables(&self) -> Result<usize, CoreError>;

    // 持久表管理
    async fn create_persistent_table(&self, source: &str, name: &str) -> Result<String, CoreError>;
    async fn list_persistent_tables(&self) -> Result<Vec<TableInfo>, CoreError>;

    // 全局表管理（含版本）
    async fn create_global_table(&self, source: &str, name: &str) -> Result<GlobalTableResult, CoreError>;
    async fn list_global_tables(&self) -> Result<Vec<GlobalTableInfo>, CoreError>;
    async fn get_global_table_version(&self, name: &str, version: i32) -> Result<Option<TableInfo>, CoreError>;

    // 懒加载支持
    async fn get_table_stats(&self, schema: &str, table: &str) -> Result<TableStats, CoreError>;
}

/// 全局表创建结果
pub struct GlobalTableResult {
    pub table_name: String,
    pub version: i32,
    pub full_name: String,  // global.supplier_v3
}
```

---

## 五、资源状态机

```
┌─────────────────────────────────────────────────────────────────┐
│                        资源生命周期                                 │
└─────────────────────────────────────────────────────────────────┘

[查询结果集]
        │
        ▼
┌─────────────────┐   persist 操作   ┌─────────────────┐
│  会话临时表      │ ───────────────▶│  项目持久表      │
│  temp.result_xx │                 │  persist.xxx    │
└─────────────────┘                 └─────────────────┘
        │                                   │
        │ 会话结束 / 用户清理                │ promote 操作
        ▼                                   ▼
┌─────────────────┐                 ┌─────────────────┐
│ 自动清理         │                 │  全局表（新版本）│
│ (DROP TEMP)     │                 │  global.xxx_vN  │
└─────────────────┘                 └─────────────────┘
                                              │
                                              │ 旧版本保留
                                              ▼
                                    ┌─────────────────┐
                                    │  版本历史        │
                                    │  global.xxx_v1  │
                                    │  global.xxx_v2  │
                                    │  global.xxx_v3  │ ← 当前
                                    └─────────────────┘

[文件导入流程]
        │
        ├─ 导入到项目 → project_files/ → persist scope
        │
        ├─ 提升为全局 → ~/.rdatastation/shared-files/ → application scope
        │
        └─ 仅引用   → 记录原路径 → external scope（不复制文件）
```

---

## 六、SQL 引用协议

| 资源类型 | 作用域 | SQL 前缀                               | 示例                                       |
| -------- | ------ | -------------------------------------- | ------------------------------------------ |
| 远程表   | 项目   | `{conn_id}.{schema}.{table}`           | `mysql_conn1.public.users`                 |
| 临时表   | 会话   | `temp.{table_name}`                    | `temp.result_123`                          |
| 持久表   | 项目   | `persist.{table_name}`                 | `persist.sales_summary`                    |
| 全局表   | 应用   | `global.{table_name}_v{version}`       | `global.region_dim_v3`                     |
| 项目文件 | 项目   | `read_csv_auto('{path}')`              | `read_csv_auto('project_files/data.csv')`  |
| 全局文件 | 应用   | `read_csv_auto('shared://{filename}')` | `read_csv_auto('shared://dim_region.csv')` |

---

## 七、关键技术决策

| 决策项         | 方案                          | 理由                     |
| -------------- | ----------------------------- | ------------------------ |
| 文件夹存储     | project.db + global.db        | 跟随项目/应用迁移        |
| 临时表生命周期 | SQL Tab 关闭                  | 每个分析上下文独立       |
| 资源引用       | 多对多关联表                  | 同一资源可属于多个文件夹 |
| 依赖追踪       | resource_references 表        | 显示引用链，防止误删     |
| 全局文件存储   | ~/.rdatastation/shared-files/ | 统一路径，跨项目共享     |
| 回收站         | 软删除 + 30天过期             | 数据安全，恢复机制       |
| 全局表版本     | 自动创建新版本                | 不破坏旧引用，平稳升级   |
| 懒加载         | 文件夹展开时加载              | 性能优化                 |

---

## 八、优化空间

### 8.1 性能优化

- 虚拟滚动 + 懒加载（ag-Grid）
- 资源元数据缓存（复用 metadata_cache）
- 预加载策略（用户浏览时预加载）

### 8.2 用户体验优化

- 资源搜索与模糊匹配
- 智能推荐（最近使用/最常用）
- 批量操作

### 8.3 架构灵活性优化

- 资源类型可扩展（插件机制）
- 自定义资源标签
- 资源模板

### 8.4 数据安全优化

- 回收站（已实现）
- 操作审计
- 删除前依赖检查（已实现）

### 8.5 团队协作优化（未来）

- 资源评论
- 资源变更通知
- 权限控制

---

## 九、API 设计（Tauri Commands）

```rust
// 资源管理
#[tauri::command]
async fn get_analytics_resources(filters: ResourceFilters) -> Result<Vec<AnalyticsResource>, CoreError>;

#[tauri::command]
async fn get_resource(resource_id: String) -> Result<AnalyticsResource, CoreError>;

#[tauri::command]
async fn search_analytics_resources(query: String, filters: ResourceFilters) -> Result<Vec<AnalyticsResource>, CoreError>;

// 资源操作
#[tauri::command]
async fn create_temp_table(result: QueryResult, name: String, session_id: String) -> Result<AnalyticsResource, CoreError>;

#[tauri::command]
async fn import_file(source_path: String, import_mode: FileImportMode) -> Result<AnalyticsResource, CoreError>;

#[tauri::command]
async fn persist_temp_table(temp_resource_id: String, name: String) -> Result<AnalyticsResource, CoreError>;

#[tauri::command]
async fn promote_to_global(resource_id: String) -> Result<AnalyticsResource, CoreError>;

#[tauri::command]
async fn delete_resource(resource_id: String) -> Result<(), CoreError>;

// 文件夹管理
#[tauri::command]
async fn get_folders() -> Result<Vec<Folder>, CoreError>;

#[tauri::command]
async fn create_folder(name: String, parent_id: Option<String>) -> Result<Folder, CoreError>;

#[tauri::command]
async fn add_resource_to_folder(folder_id: String, resource_id: String) -> Result<(), CoreError>;

// 回收站
#[tauri::command]
async fn get_recycle_bin() -> Result<Vec<RecycleBinItem>, CoreError>;

#[tauri::command]
async fn restore_resource(recycle_id: String) -> Result<AnalyticsResource, CoreError>;

#[tauri::command]
async fn permanent_delete(recycle_id: String) -> Result<(), CoreError>;

// 依赖追踪
#[tauri::command]
async fn get_resource_dependencies(resource_id: String) -> Result<DependencyGraph, CoreError>;

#[tauri::command]
async fn check_delete_safe(resource_id: String) -> Result<DeleteCheckResult, CoreError>;

// SQL 引用
#[tauri::command]
async fn generate_sql_reference(resource_id: String) -> Result<String, CoreError>;
```

---

## 十、核心定位

分析资源管理器 = 数据资源的「统一视图 + 快速引用 + 灵活组织 + 生命周期管理」

定位是 **"数据兵器库"**，而非简单的文件浏览器。
