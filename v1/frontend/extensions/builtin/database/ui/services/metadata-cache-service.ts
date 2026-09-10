/**
 * 元数据缓存服务
 *
 * 提供数据库元数据缓存的读取、刷新、保存等操作
 * 优先使用缓存，缓存失效时回退到实时查询
 *
 * 注意：类型已与 Rust 后端通过 ts-rs 保持同步，
 * 类型定义在 src/generated/ 目录自动生成
 */

import { invoke } from '@tauri-apps/api/core'

// 导入自动生成的类型（待类型生成完成后启用）
// import type {
//   DatabaseMeta,
//   CatalogMeta,
//   SchemaMeta,
//   TableMeta,
//   ViewMeta,
//   ColumnMeta,
//   ProcedureMeta,
//   FunctionMeta,
//   RoutineSourceMeta,
//   IndexMeta,
//   ConstraintMeta,
//   CacheStats,
//   CacheStatusResponse,
//   CacheStatsResponse,
//   RefreshCacheInput,
//   ClearCacheInput,
//   TableInput,
//   ColumnInput,
//   DDLEventInput,
//   SyncStatusInfo,
//   WarmingProgressResponse,
//   WarmCacheInput,
//   CancelWarmingInput,
//   MigrationResponse,
//   BuildCacheIndexInput,
//   IndexBuildResponse,
//   SchemaObjectCountsResponse,
// } from '../../../generated';

// 临时类型定义（待自动生成启用）
export interface DatabaseMeta {
  name: string
}
export interface CatalogMeta {
  name: string
}
export interface SchemaMeta {
  name: string
  // V10: 企业级 schema 聚合统计
  totalTables?: number
  totalViews?: number
  totalProcedures?: number
  totalFunctions?: number
  totalSizeBytes?: number
  rowCountTotal?: number
}
export interface TableMeta {
  name: string
  type: string
  rowCountEstimate?: number | null
  dataLength?: number | null
  indexLength?: number | null
  displayOrder?: number
  hidden?: boolean
  favorite?: boolean
  colorLabel?: string | null
  userComment?: string | null
}
export interface ViewMeta {
  name: string
  type: string
}
export interface ColumnMeta {
  name: string
  dataType: string
  isNullable: boolean
  defaultValue: string | null
  isPrimaryKey: boolean
  isForeignKey: boolean
  comment: string | null
}
export interface ProcedureMeta {
  name: string
}
export interface FunctionMeta {
  name: string
}
export interface RoutineSourceMeta {
  name: string
  routineKind: string
  sourceCode: string | null
}
export interface IndexMeta {
  name: string
  tableName: string
  columnNames: string[]
  isUnique: boolean
  isPrimary: boolean
  indexType: string | null
  comment: string | null
}
export interface ConstraintMeta {
  name: string
  tableName: string
  constraintType: string
  columnNames: string[]
  referencedTable: string | null
  referencedColumns: string[]
  updateRule: string | null
  deleteRule: string | null
}
export interface CacheStats {
  l1_hits: number
  l1_misses: number
  l2_hits: number
  l2_misses: number
  db_queries: number
  l1_hit_avg_us: number
  l2_hit_avg_us: number
  db_query_avg_us: number
  l1_hit_rate: number
  l2_hit_rate: number
  overall_hit_rate: number
}
export interface CacheStatusResponse {
  is_valid: boolean
  last_sync: number | null
  stats: CacheStatsResponse | null
}
export interface CacheStatsResponse {
  table_count: number
  column_count: number
  last_sync: number | null
}
export interface RefreshCacheInput {
  connection_id: string
  connection_type: string
  project_path?: string
  database_name: string
  schema_name?: string
}
export interface ClearCacheInput {
  connection_id: string
  connection_type: string
  project_path?: string
  database_name: string
  schema_name?: string
}
export interface TableInput {
  id: string
  name: string
  comment?: string
}
export interface ColumnInput {
  id: string
  name: string
  data_type: string
  is_nullable: boolean
  is_primary: boolean
  is_unique: boolean
}
export interface DDLEventInput {
  type: string
  connection_id: string
  connection_type?: string
  project_path?: string
  database_name: string
  schema_name?: string
  table_name?: string
  column_name?: string
  executed_at?: number
}
export interface SyncStatusInfo {
  in_progress: boolean
  total_tables: number
  completed_tables: number
  last_sync_time: number | null
}
export interface WarmingProgressResponse {
  connection_id: string
  is_warming: boolean
  current_step: string
  total_steps: number
  completed_steps: number
  progress_percentage: number
  current_database?: string
  current_schema?: string
  current_table?: string
}
export interface WarmCacheInput {
  connection_id: string
  connection_type: string
  project_path?: string
  databases: string[]
}
export interface CancelWarmingInput {
  connection_id: string
}
export interface MigrationResponse {
  from_version: number
  to_version: number
  success: boolean
  duration_ms?: number
  message: string
}
export interface BuildCacheIndexInput {
  connection_id: string
  connection_type: string
  project_path?: string
  source_connection_id: string
  database: string
  schema?: string
  incremental?: boolean
}
export interface IndexBuildResponse {
  success: boolean
  schema_count: number
  table_count: number
  column_count: number
  total_entries: number
  message: string
  incremental?: boolean
  create_count?: number
  update_count?: number
  delete_count?: number
}
export interface SchemaObjectCountsResponse {
  table_count: number
  view_count: number
  column_count: number
  routine_count: number
  total: number
}

// ============================================================================
// 元数据加载 API (metadata_commands.rs)
// ============================================================================

/**
 * 获取数据库列表（Catalogs）
 */
export async function loadDatabases(
  connId: string,
  connectionType?: 'global' | 'project',
  projectPath?: string
): Promise<DatabaseMeta[]> {
  return invoke<DatabaseMeta[]>('load_databases', {
    connId,
    connectionType,
    projectPath,
  })
}

/**
 * 获取数据库列表（Catalogs，别名）
 */
export async function loadCatalogs(
  connId: string,
  connectionType?: 'global' | 'project',
  projectPath?: string
): Promise<CatalogMeta[]> {
  return invoke<CatalogMeta[]>('load_catalogs', {
    connId,
    connectionType,
    projectPath,
  })
}

/**
 * 获取 Schema 列表
 */
export async function loadSchemas(
  connId: string,
  dbName: string,
  connectionType?: 'global' | 'project',
  projectPath?: string
): Promise<SchemaMeta[]> {
  return invoke<SchemaMeta[]>('load_schemas', {
    connId,
    dbName,
    connectionType,
    projectPath,
  })
}

/**
 * 获取表列表
 */
export async function loadTables(
  connId: string,
  dbName: string,
  schemaName: string,
  connectionType?: 'global' | 'project',
  projectPath?: string
): Promise<TableMeta[]> {
  return invoke<TableMeta[]>('load_tables', {
    connId,
    dbName,
    schemaName,
    connectionType,
    projectPath,
  })
}

/**
 * 获取视图列表
 */
export async function loadViews(
  connId: string,
  dbName: string,
  schemaName: string,
  connectionType?: 'global' | 'project',
  projectPath?: string
): Promise<ViewMeta[]> {
  return invoke<ViewMeta[]>('load_views', {
    connId,
    dbName,
    schemaName,
    connectionType,
    projectPath,
  })
}

/**
 * 获取列列表
 */
export async function loadColumns(
  connId: string,
  dbName: string,
  schemaName: string,
  tableName: string,
  connectionType?: 'global' | 'project',
  projectPath?: string
): Promise<ColumnMeta[]> {
  return invoke<ColumnMeta[]>('load_columns', {
    connId,
    dbName,
    schemaName,
    tableName,
    connectionType,
    projectPath,
  })
}

/**
 * 获取存储过程列表
 */
export async function loadProcedures(
  connId: string,
  dbName: string,
  schemaName: string
): Promise<ProcedureMeta[]> {
  return invoke<ProcedureMeta[]>('load_procedures', {
    connId,
    dbName,
    schemaName,
  })
}

/**
 * 获取函数列表
 */
export async function loadFunctions(
  connId: string,
  dbName: string,
  schemaName: string
): Promise<FunctionMeta[]> {
  return invoke<FunctionMeta[]>('load_functions', {
    connId,
    dbName,
    schemaName,
  })
}

/**
 * 获取例程源码
 */
export async function loadRoutineSource(
  connId: string,
  dbName: string,
  schemaName: string,
  routineName: string,
  routineKind: string
): Promise<RoutineSourceMeta> {
  return invoke<RoutineSourceMeta>('load_routine_source', {
    connId,
    dbName,
    schemaName,
    routineName,
    routineKind,
  })
}

/**
 * 获取索引列表
 */
export async function loadIndexes(
  connId: string,
  dbName: string,
  schemaName: string,
  tableName: string,
  connectionType?: 'global' | 'project',
  projectPath?: string
): Promise<IndexMeta[]> {
  return invoke<IndexMeta[]>('load_indexes', {
    connId,
    dbName,
    schemaName,
    tableName,
    connectionType,
    projectPath,
  })
}

/**
 * 获取约束列表
 */
export async function loadConstraints(
  connId: string,
  dbName: string,
  schemaName: string,
  tableName: string,
  connectionType?: 'global' | 'project',
  projectPath?: string
): Promise<ConstraintMeta[]> {
  return invoke<ConstraintMeta[]>('load_constraints', {
    connId,
    dbName,
    schemaName,
    tableName,
    connectionType,
    projectPath,
  })
}

/**
 * 使元数据缓存失效
 */
export async function invalidateMetadataCache(connId: string): Promise<void> {
  return invoke('invalidate_metadata_cache', {
    connId,
  })
}

/**
 * 获取缓存统计信息
 */
export async function getCacheStats(): Promise<CacheStats> {
  return invoke<CacheStats>('get_cache_stats')
}

/**
 * 重置缓存统计信息
 */
export async function resetCacheStats(): Promise<void> {
  return invoke('reset_cache_stats')
}

/**
 * 设置内省级别
 * @param level 'level1' | 'level2' | 'level3'
 */
export async function setIntrospectionLevel(connId: string, level: string): Promise<void> {
  return invoke('set_introspection_level', {
    connId,
    level,
  })
}

/**
 * 获取内省级别
 */
export async function getIntrospectionLevel(connId: string): Promise<string> {
  return invoke<string>('get_introspection_level', {
    connId,
  })
}

/**
 * 移除内省级别
 */
export async function removeIntrospectionLevel(connId: string): Promise<void> {
  return invoke('remove_introspection_level', {
    connId,
  })
}

// ============================================================================
// 元数据缓存管理 API (metadata_cache_commands.rs)
// ============================================================================

/**
 * 获取缓存状态
 */
export async function getMetadataCacheStatus(
  connectionId: string,
  connectionType: 'global' | 'project',
  databaseName: string,
  schemaName?: string,
  projectPath?: string
): Promise<CacheStatusResponse> {
  return invoke<CacheStatusResponse>('get_metadata_cache_status', {
    connectionId,
    connectionType,
    projectPath,
    databaseName,
    schemaName,
  })
}

/**
 * 刷新元数据缓存
 */
export async function refreshMetadataCache(input: RefreshCacheInput): Promise<void> {
  return invoke('refresh_metadata_cache', input as unknown as Record<string, unknown>)
}

/**
 * 清除元数据缓存（同时删除缓存文件）
 */
export async function clearMetadataCache(input: ClearCacheInput): Promise<number> {
  return invoke<number>('clear_metadata_cache', input as unknown as Record<string, unknown>)
}

/**
 * 保存表元数据到缓存
 */
export async function saveTableMetadataToCache(
  connectionId: string,
  connectionType: 'global' | 'project',
  projectPath: string | undefined,
  id: string,
  databaseName: string,
  schemaName: string,
  tableName: string,
  comment: string | undefined
): Promise<void> {
  return invoke('save_table_metadata_to_cache', {
    connectionId,
    connectionType,
    projectPath,
    id,
    databaseName,
    schemaName,
    tableName,
    comment,
  })
}

/**
 * 批量保存表元数据到缓存
 */
export async function saveTablesBatchToCache(
  connectionId: string,
  connectionType: 'global' | 'project',
  projectPath: string | undefined,
  databaseName: string,
  schemaName: string,
  tables: TableInput[]
): Promise<number> {
  return invoke<number>('save_tables_batch_to_cache', {
    connectionId,
    connectionType,
    projectPath,
    databaseName,
    schemaName,
    tables,
  })
}

/**
 * 保存列元数据到缓存
 */
export async function saveColumnMetadataToCache(
  connectionId: string,
  connectionType: 'global' | 'project',
  projectPath: string | undefined,
  id: string,
  databaseName: string,
  schemaName: string,
  tableName: string,
  columnName: string,
  dataType: string,
  isNullable: boolean,
  isPrimary: boolean,
  isUnique: boolean
): Promise<void> {
  return invoke('save_column_metadata_to_cache', {
    connectionId,
    connectionType,
    projectPath,
    id,
    databaseName,
    schemaName,
    tableName,
    columnName,
    dataType,
    isNullable,
    isPrimary,
    isUnique,
  })
}

/**
 * 批量保存列元数据到缓存
 */
export async function saveColumnsBatchToCache(
  connectionId: string,
  connectionType: 'global' | 'project',
  projectPath: string | undefined,
  databaseName: string,
  schemaName: string,
  tableName: string,
  columns: ColumnInput[]
): Promise<number> {
  return invoke<number>('save_columns_batch_to_cache', {
    connectionId,
    connectionType,
    projectPath,
    databaseName,
    schemaName,
    tableName,
    columns,
  })
}

interface CachedTableRow {
  name: string
  schema_name?: string
  rowCountEstimate?: number
  dataLength?: number
  indexLength?: number
}

interface CachedColumnRow {
  name: string
  data_type: string
  is_nullable: boolean
  is_primary: boolean
}

/**
 * 从缓存获取表列表
 */
export async function getTablesFromCache(
  connectionId: string,
  connectionType: 'global' | 'project',
  databaseName: string,
  schemaName?: string,
  projectPath?: string
): Promise<CachedTableRow[]> {
  return invoke<CachedTableRow[]>('get_tables_from_cache', {
    connectionId,
    connectionType,
    projectPath,
    databaseName,
    schemaName,
  })
}

/**
 * 从缓存获取列列表
 */
export async function getColumnsFromCache(
  connectionId: string,
  connectionType: 'global' | 'project',
  databaseName: string,
  schemaName: string,
  tableName: string,
  projectPath?: string
): Promise<CachedColumnRow[]> {
  return invoke<CachedColumnRow[]>('get_columns_from_cache', {
    connectionId,
    connectionType,
    projectPath,
    databaseName,
    schemaName,
    tableName,
  })
}

/**
 * 通知后端 DDL 事件（缓存失效）
 */
export async function notifyDDLEvent(event: DDLEventInput): Promise<void> {
  return invoke('notify_ddl_event', event as unknown as Record<string, unknown>)
}

/**
 * 获取同步状态
 */
export async function getSyncStatus(connectionId: string): Promise<SyncStatusInfo | null> {
  return invoke<SyncStatusInfo | null>('get_sync_status', {
    connectionId,
  })
}

/**
 * 取消同步任务
 */
export async function cancelSync(
  connectionId: string,
  connectionType: 'global' | 'project',
  projectPath?: string
): Promise<void> {
  return invoke('cancel_sync', {
    connectionId,
    connectionType,
    projectPath,
  })
}

// ============================================================================
// 缓存预热 API (cache_warming_commands.rs)
// ============================================================================

/**
 * V7: 构建缓存索引（支持增量模式）
 */
export async function buildCacheIndex(input: BuildCacheIndexInput): Promise<IndexBuildResponse> {
  return invoke<IndexBuildResponse>(
    'build_cache_index',
    input as unknown as Record<string, unknown>
  )
}

/**
 * 启动缓存预热
 */
export async function startCacheWarming(input: WarmCacheInput): Promise<WarmingProgressResponse> {
  return invoke<WarmingProgressResponse>(
    'start_cache_warming',
    input as unknown as Record<string, unknown>
  )
}

/**
 * 取消缓存预热
 */
export async function cancelCacheWarming(input: CancelWarmingInput): Promise<void> {
  return invoke('cancel_cache_warming', input as unknown as Record<string, unknown>)
}

/**
 * 获取预热进度
 */
export async function getWarmingProgress(connectionId: string): Promise<WarmingProgressResponse> {
  return invoke<WarmingProgressResponse>('get_warming_progress', {
    connectionId,
  })
}

/**
 * 检查缓存版本
 */
export async function checkCacheVersion(
  connectionId: string,
  connectionType: 'global' | 'project',
  projectPath?: string
): Promise<number> {
  return invoke<number>('check_cache_version', {
    connectionId,
    connectionType,
    projectPath,
  })
}

/**
 * 执行缓存版本迁移
 */
export async function executeCacheMigration(
  connectionId: string,
  connectionType: 'global' | 'project',
  projectPath?: string
): Promise<MigrationResponse> {
  return invoke<MigrationResponse>('execute_cache_migration', {
    connectionId,
    connectionType,
    projectPath,
  })
}

/**
 * 获取缓存版本迁移历史
 */
export async function getCacheMigrationHistory(
  connectionId: string,
  connectionType: 'global' | 'project',
  projectPath?: string
): Promise<Record<string, unknown>[]> {
  return invoke<Record<string, unknown>[]>('get_cache_migration_history', {
    connectionId,
    connectionType,
    projectPath,
  })
}

/**
 * V7: 获取内省级别建议（DataGrip 风格）
 */
export async function getIntrospectLevelSuggestion(
  connectionId: string,
  connectionType: 'global' | 'project',
  projectPath: string | undefined,
  schemaId: number,
  isCurrentSchema: boolean
): Promise<number> {
  return invoke<number>('get_introspect_level_suggestion', {
    connectionId,
    connectionType,
    projectPath,
    schemaId,
    isCurrentSchema,
  })
}

/**
 * V7: 获取 Schema 对象数量统计
 */
export async function getSchemaObjectCounts(
  connectionId: string,
  connectionType: 'global' | 'project',
  projectPath: string | undefined,
  schemaId: number
): Promise<SchemaObjectCountsResponse> {
  return invoke<SchemaObjectCountsResponse>('get_schema_object_counts', {
    connectionId,
    connectionType,
    projectPath,
    schemaId,
  })
}

// ============================================================================
// 便捷工具函数
// ============================================================================

/**
 * 完整加载单个表的所有信息（列、索引、约束）
 */
export async function loadTableFullMetadata(
  connId: string,
  dbName: string,
  schemaName: string,
  tableName: string,
  connectionType?: 'global' | 'project',
  projectPath?: string
) {
  const [columns, indexes, constraints] = await Promise.all([
    loadColumns(connId, dbName, schemaName, tableName, connectionType, projectPath),
    loadIndexes(connId, dbName, schemaName, tableName, connectionType, projectPath),
    loadConstraints(connId, dbName, schemaName, tableName, connectionType, projectPath),
  ])

  return { columns, indexes, constraints }
}

/**
 * 完整加载一个 Schema 的所有表信息
 */
export async function loadSchemaFullMetadata(
  connId: string,
  dbName: string,
  schemaName: string,
  connectionType?: 'global' | 'project',
  projectPath?: string
) {
  const [tables, views, procedures, functions] = await Promise.all([
    loadTables(connId, dbName, schemaName, connectionType, projectPath),
    loadViews(connId, dbName, schemaName, connectionType, projectPath),
    loadProcedures(connId, dbName, schemaName),
    loadFunctions(connId, dbName, schemaName),
  ])

  return { tables, views, procedures, functions }
}
