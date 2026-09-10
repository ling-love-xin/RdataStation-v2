-- 迁移版本：006
-- 数据库类型：SQLite
-- 作用：索引表支持分页懒加载、预热状态跟踪
-- 更新：支持百万级表数据库的快速加载
-- 更新时间：2026-05-04

-- ===========================================================================
-- ======================== 第一阶段：索引表 ==================================
-- ===========================================================================

-- 1. 创建元数据索引表
-- 用于快速定位对象，避免全量加载
CREATE TABLE IF NOT EXISTS metadata_index (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    -- 索引键
    connection_id TEXT NOT NULL,
    schema_id INTEGER,                      -- 关联 schemata 表
    object_type TEXT NOT NULL CHECK (object_type IN (
        'schema', 'table', 'view', 'column', 'index', 'routine', 'routine_param'
    )),
    object_name TEXT NOT NULL,
    parent_name TEXT,                        -- 表的列：parent_name=表名
    -- 层级路径（用于虚拟树）
    path TEXT NOT NULL,                      -- 格式：schema/table/column
    -- 索引状态
    introspect_level INTEGER DEFAULT 1,      -- 1=仅索引, 2=已加载概要, 3=完整详情
    is_loaded INTEGER DEFAULT 0,
    last_sync INTEGER,
    -- 统计信息（可选，用于排序）
    row_count_estimate INTEGER,
    data_length INTEGER,
    -- 排序权重（用于搜索结果排序）
    sort_weight INTEGER DEFAULT 0,
    UNIQUE (connection_id, object_type, object_name, parent_name)
);

-- 2. 创建快速查询索引
CREATE INDEX IF NOT EXISTS idx_metadata_index_lookup ON metadata_index(
    connection_id, object_type, object_name
);

CREATE INDEX IF NOT EXISTS idx_metadata_index_schema ON metadata_index(
    connection_id, schema_id, object_type
);

CREATE INDEX IF NOT EXISTS idx_metadata_index_path ON metadata_index(
    connection_id, path
);

CREATE INDEX IF NOT EXISTS idx_metadata_index_level ON metadata_index(
    connection_id, introspect_level
);

-- ===========================================================================
-- ======================== 第二阶段：连接同步状态 =============================
-- ===========================================================================

-- 3. 连接同步状态表
CREATE TABLE IF NOT EXISTS connection_sync_status (
    connection_id TEXT PRIMARY KEY,
    -- 同步状态
    status TEXT DEFAULT 'idle' CHECK (status IN (
        'idle', 'indexing', 'syncing', 'completed', 'error', 'cancelled'
    )),
    -- 进度信息
    progress INTEGER DEFAULT 0,              -- 0-100
    total_objects INTEGER DEFAULT 0,         -- 总对象数
    synced_objects INTEGER DEFAULT 0,       -- 已同步对象数
    current_object TEXT,                    -- 当前同步对象
    -- 时间戳
    started_at INTEGER,
    completed_at INTEGER,
    last_error TEXT,
    -- 同步配置
    sync_mode TEXT DEFAULT 'lazy',           -- lazy=懒加载, full=全量, smart=智能
    max_batch_size INTEGER DEFAULT 1000,     -- 最大批量大小
    UNIQUE (connection_id)
);

-- ===========================================================================
-- ======================== 第三阶段：增量同步日志 =============================
-- ===========================================================================

-- 4. 增量同步日志
CREATE TABLE IF NOT EXISTS sync_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    connection_id TEXT NOT NULL,
    sync_type TEXT NOT NULL,                -- index/table/column/routine/full
    started_at INTEGER NOT NULL,
    ended_at INTEGER,
    duration_ms INTEGER,
    objects_count INTEGER DEFAULT 0,
    success INTEGER DEFAULT 1,
    error_message TEXT,
    sync_mode TEXT                           -- lazy/full/smart
);

CREATE INDEX IF NOT EXISTS idx_sync_log_connection ON sync_log(
    connection_id, started_at DESC
);

-- ===========================================================================
-- ======================== 第四阶段：后台同步任务 =============================
-- ===========================================================================

-- 5. 待同步任务队列（用于后台增量同步）
CREATE TABLE IF NOT EXISTS sync_tasks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    connection_id TEXT NOT NULL,
    task_type TEXT NOT NULL CHECK (task_type IN (
        'schema', 'table', 'column', 'index', 'view', 'routine'
    )),
    object_name TEXT NOT NULL,
    parent_name TEXT,                        -- schema 名或表名
    priority INTEGER DEFAULT 5,             -- 1-10, 1 最高
    status TEXT DEFAULT 'pending' CHECK (status IN (
        'pending', 'running', 'completed', 'failed', 'cancelled'
    )),
    created_at INTEGER DEFAULT (strftime('%s', 'now')),
    started_at INTEGER,
    completed_at INTEGER,
    retry_count INTEGER DEFAULT 0,
    error_message TEXT
);

CREATE INDEX IF NOT EXISTS idx_sync_tasks_pending ON sync_tasks(
    connection_id, status, priority, created_at
);

-- ===========================================================================
-- ======================== 第五阶段：数据迁移 =================================
-- ===========================================================================

-- 6. 从规范化表迁移数据到索引表
-- 注意：这是可选的，用于兼容旧数据
-- 新数据应直接写入 metadata_index

-- 迁移 schemata 到索引表
INSERT OR IGNORE INTO metadata_index (
    connection_id, schema_id, object_type, object_name, parent_name, path,
    introspect_level, is_loaded, last_sync, sort_weight
)
SELECT 
    'legacy' as connection_id,
    s.id as schema_id,
    'schema' as object_type,
    s.schema_name as object_name,
    '' as parent_name,
    s.schema_name as path,
    COALESCE(s.introspect_level, 3) as introspect_level,
    COALESCE(s.is_loaded, 1) as is_loaded,
    s.last_sync,
    100 as sort_weight
FROM schemata s;

-- 迁移 tables 到索引表
INSERT OR IGNORE INTO metadata_index (
    connection_id, schema_id, object_type, object_name, parent_name, path,
    introspect_level, is_loaded, last_sync, row_count_estimate, sort_weight
)
SELECT 
    'legacy' as connection_id,
    t.schema_id,
    'table' as object_type,
    t.table_name as object_name,
    s.schema_name as parent_name,
    s.schema_name || '/' || t.table_name as path,
    COALESCE(t.introspect_level, 3) as introspect_level,
    COALESCE(t.is_loaded, 1) as is_loaded,
    t.last_sync,
    t.row_count_estimate,
    CASE t.table_type 
        WHEN 'TABLE' THEN 90 
        WHEN 'VIEW' THEN 80 
        ELSE 70 
    END as sort_weight
FROM tables t
INNER JOIN schemata s ON t.schema_id = s.id;

-- 迁移 views 到索引表
INSERT OR IGNORE INTO metadata_index (
    connection_id, schema_id, object_type, object_name, parent_name, path,
    introspect_level, is_loaded, last_sync, sort_weight
)
SELECT 
    'legacy' as connection_id,
    v.schema_id,
    'view' as object_type,
    v.view_name as object_name,
    s.schema_name as parent_name,
    s.schema_name || '/' || v.view_name as path,
    3 as introspect_level,
    1 as is_loaded,
    v.last_sync,
    80 as sort_weight
FROM views v
INNER JOIN schemata s ON v.schema_id = s.id;

-- ===========================================================================
-- ======================== 第六阶段：版本更新 ================================
-- ===========================================================================

-- 7. 更新缓存版本
UPDATE cache_version 
SET version = 6, 
    upgraded_at = strftime('%s', 'now'),
    updated_at = strftime('%s', 'now')
WHERE id = 1;

-- 8. 记录迁移历史
INSERT INTO cache_migration_history (version, migrated_at, duration_ms, success, error_message)
SELECT 
    6,
    strftime('%s', 'now'),
    0,
    1,
    'V6: 索引表支持分页懒加载、预热状态跟踪'
WHERE NOT EXISTS (
    SELECT 1 FROM cache_migration_history WHERE version = 6
);

-- ===========================================================================
-- ======================== 注意事项 =========================================
-- ===========================================================================
-- 
-- 1. metadata_index 表是懒加载的核心：
--    - Level 1: 只插入索引（schema/table 名）
--    - Level 2: 更新概要信息（row_count 等）
--    - Level 3: 完整详情（从 columns/indexes 等表获取）
--
-- 2. connection_sync_status 表跟踪同步进度：
--    - idle: 空闲
--    - indexing: 正在构建索引
--    - syncing: 正在同步详情
--    - completed: 完成
--    - error: 错误
--    - cancelled: 取消
--
-- 3. sync_tasks 表支持后台增量同步：
--    - 用户展开节点时才插入详细同步任务
--    - 后台任务按优先级排队执行
--
-- 4. 向后兼容：
--    - 旧的规范化表仍然可用
--    - 新数据同时写入索引表和规范化表
--    - 查询时优先使用索引表加速
--