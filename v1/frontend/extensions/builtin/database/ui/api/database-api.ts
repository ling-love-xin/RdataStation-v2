/**
 * 数据库导航 API 层
 *
 * 统一处理与 Tauri 后端的通信
 * 使用 tauri-specta 生成的 typed commands
 * 遵循规范：所有数据交互必须通过 tauri.invoke 调用 Rust 核心接口
 */

import { commands } from '@/generated/specta/bindings'
import type {
  ColumnMeta,
  IndexMeta,
  ConstraintMeta,
  ProcedureMeta,
  FunctionMeta,
  SequenceMeta,
  TriggerMeta,
  RoutineSourceMeta,
} from '@/generated/specta/bindings'
import { typed } from '@/shared/api'

/**
 * Catalog 元数据 — 树形导航根节点
 */
export interface ICatalogMeta {
  /** Catalog 名称 */
  name: string
  /** Schema 列表 */
  schemas?: ISchemaMeta[]
}

/**
 * Schema 元数据
 */
export interface ISchemaMeta {
  /** Schema 名称 */
  name: string
  /** 表列表 */
  tables?: ITableMeta[]
  /** 视图列表 */
  views?: IViewMeta[]
}

/**
 * 表元数据
 */
export interface ITableMeta {
  /** 表名 */
  name: string
  /** 表类型（TABLE / VIEW） */
  type: string
  /** 列列表 */
  columns?: IColumnMeta[]
  /** 索引列表 */
  indexes?: IIndexMeta[]
  /** 约束列表 */
  constraints?: IConstraintMeta[]
}

/**
 * 索引元数据
 */
export interface IIndexMeta {
  name: string
  columns: string[]
  unique: boolean
}

/**
 * 约束元数据
 */
export interface IConstraintMeta {
  name: string
  type: string
  columns: string[]
}

/**
 * 视图元数据
 */
export interface IViewMeta {
  /** 视图名 */
  name: string
  /** 列列表 */
  columns?: IColumnMeta[]
}

/**
 * 列元数据（对齐后端 ColumnMeta）
 *
 * @see ColumnMeta (Rust metadata_commands.rs)
 */
export interface IColumnMeta {
  /** 列名 */
  name: string
  /** 数据类型 */
  dataType: string
  /** 是否可空 */
  isNullable: boolean
  /** 默认值 */
  defaultValue?: string
  /** 是否主键 */
  isPrimaryKey?: boolean
}

/**
 * 加载 Catalog 列表（ANSI SQL 标准三层结构：Catalog → Schema → Table）
 *
 * @param connectionId - 连接标识符
 * @param connectionType - 连接类型（global / project），影响 L2 磁盘缓存路径
 * @param projectPath - 项目路径（project 连接需要）
 * @returns Catalog 元数据数组
 */
export async function loadCatalogs(
  connectionId: string,
  connectionType?: string,
  projectPath?: string
): Promise<ICatalogMeta[]> {
  return typed(
    commands.loadCatalogs(connectionId, connectionType ?? null, projectPath ?? null)
  ) as unknown as ICatalogMeta[]
}

/**
 * 加载 Schema 列表
 *
 * @param connectionId - 连接标识符
 * @param catalogName - Catalog 名称
 * @param connectionType - 连接类型（global / project）
 * @param projectPath - 项目路径（project 连接需要）
 * @returns Schema 元数据数组
 */
export async function loadSchemas(
  connectionId: string,
  catalogName: string,
  connectionType?: string,
  projectPath?: string
): Promise<ISchemaMeta[]> {
  return typed(
    commands.loadSchemas(connectionId, catalogName, connectionType ?? null, projectPath ?? null)
  ) as unknown as ISchemaMeta[]
}

/**
 * 加载表列表
 *
 * @param connectionId - 连接标识符
 * @param catalogName - Catalog 名称
 * @param schemaName - Schema 名称
 * @param connectionType - 连接类型（global / project）
 * @param projectPath - 项目路径（project 连接需要）
 * @returns 表元数据数组
 */
export async function loadTables(
  connectionId: string,
  catalogName: string,
  schemaName: string,
  connectionType?: string,
  projectPath?: string
): Promise<ITableMeta[]> {
  return typed(
    commands.loadTables(
      connectionId,
      catalogName,
      schemaName,
      connectionType ?? null,
      projectPath ?? null
    )
  ) as unknown as ITableMeta[]
}

/**
 * 加载视图列表
 *
 * @param connectionId - 连接标识符
 * @param catalogName - Catalog 名称
 * @param schemaName - Schema 名称
 * @param connectionType - 连接类型（global / project）
 * @param projectPath - 项目路径（project 连接需要）
 * @returns 视图元数据数组
 */
export async function loadViews(
  connectionId: string,
  catalogName: string,
  schemaName: string,
  connectionType?: string,
  projectPath?: string
): Promise<IViewMeta[]> {
  return typed(
    commands.loadViews(
      connectionId,
      catalogName,
      schemaName,
      connectionType ?? null,
      projectPath ?? null
    )
  ) as unknown as IViewMeta[]
}

/**
 * 加载列信息
 *
 * @param connectionId - 连接标识符
 * @param catalogName - Catalog 名称
 * @param schemaName - Schema 名称
 * @param tableName - 表名或视图名
 * @param connectionType - 连接类型（global / project）
 * @param projectPath - 项目路径（project 连接需要）
 * @returns 列元数据数组
 */
export async function loadColumns(
  connectionId: string,
  catalogName: string,
  schemaName: string,
  tableName: string,
  connectionType?: string,
  projectPath?: string
): Promise<IColumnMeta[]> {
  return typed(
    commands.loadColumns(
      connectionId,
      catalogName,
      schemaName,
      tableName,
      connectionType ?? null,
      projectPath ?? null
    )
  ) as unknown as IColumnMeta[]
}

/**
 * 加载索引列表
 */
export async function loadIndexes(
  connectionId: string,
  catalogName: string,
  schemaName: string,
  tableName: string,
  connectionType?: string,
  projectPath?: string
): Promise<IndexMeta[]> {
  return typed(
    commands.loadIndexes(
      connectionId,
      catalogName,
      schemaName,
      tableName,
      connectionType ?? null,
      projectPath ?? null
    )
  )
}

/**
 * 加载约束列表
 */
export async function loadConstraints(
  connectionId: string,
  catalogName: string,
  schemaName: string,
  tableName: string,
  connectionType?: string,
  projectPath?: string
): Promise<ConstraintMeta[]> {
  return typed(
    commands.loadConstraints(
      connectionId,
      catalogName,
      schemaName,
      tableName,
      connectionType ?? null,
      projectPath ?? null
    )
  )
}

/**
 * 加载序列列表
 *
 * @param connectionId - 连接标识符
 * @param catalogName - Catalog 名称
 * @param schemaName - Schema 名称
 * @param connectionType - 连接类型（global / project）
 * @param projectPath - 项目路径（project 连接需要）
 * @returns 序列元数据数组
 */
export async function loadSequences(
  connectionId: string,
  catalogName: string,
  schemaName: string,
  connectionType?: string,
  projectPath?: string
): Promise<SequenceMeta[]> {
  return typed(
    commands.loadSequences(
      connectionId,
      catalogName,
      schemaName,
      connectionType ?? null,
      projectPath ?? null
    )
  )
}

/**
 * 加载触发器列表
 *
 * @param connectionId - 连接标识符
 * @param catalogName - Catalog 名称
 * @param schemaName - Schema 名称
 * @param connectionType - 连接类型（global / project）
 * @param projectPath - 项目路径（project 连接需要）
 * @returns 触发器元数据数组
 */
export async function loadTriggers(
  connectionId: string,
  catalogName: string,
  schemaName: string,
  connectionType?: string,
  projectPath?: string
): Promise<TriggerMeta[]> {
  return typed(
    commands.loadTriggers(
      connectionId,
      catalogName,
      schemaName,
      connectionType ?? null,
      projectPath ?? null
    )
  )
}

/**
 * 加载存储过程列表
 *
 * @param connectionId - 连接标识符
 * @param catalogName - Catalog 名称
 * @param schemaName - Schema 名称
 * @param connectionType - 连接类型（global / project）
 * @param projectPath - 项目路径（project 连接需要）
 * @returns 存储过程名数组
 */
export async function loadProcedures(
  connectionId: string,
  catalogName: string,
  schemaName: string,
  connectionType?: string,
  projectPath?: string
): Promise<ProcedureMeta[]> {
  return typed(
    commands.loadProcedures(
      connectionId,
      catalogName,
      schemaName,
      connectionType ?? null,
      projectPath ?? null
    )
  )
}

/**
 * 加载函数列表
 *
 * @param connectionId - 连接标识符
 * @param catalogName - Catalog 名称
 * @param schemaName - Schema 名称
 * @param connectionType - 连接类型（global / project）
 * @param projectPath - 项目路径（project 连接需要）
 * @returns 函数名数组
 */
export async function loadFunctions(
  connectionId: string,
  catalogName: string,
  schemaName: string,
  connectionType?: string,
  projectPath?: string
): Promise<FunctionMeta[]> {
  return typed(
    commands.loadFunctions(
      connectionId,
      catalogName,
      schemaName,
      connectionType ?? null,
      projectPath ?? null
    )
  )
}

/**
 * 加载过程/函数的 DDL 源码 (DBeaver-style Source Tab)
 * R15 新增 — get_routine_source() trait 方法 + L1 缓存
 *
 * @param connectionId - 连接标识符
 * @param catalogName - Catalog 名称
 * @param schemaName - Schema 名称
 * @param routineName - 例程名（过程或函数名）
 * @param routineKind - 例程类型（PROCEDURE / FUNCTION）
 * @returns 例程源码元数据
 */
export type { RoutineSourceMeta }

export async function loadRoutineSource(
  connectionId: string,
  catalogName: string,
  schemaName: string,
  routineName: string,
  routineKind: string
): Promise<RoutineSourceMeta> {
  return typed(
    commands.loadRoutineSource(connectionId, catalogName, schemaName, routineName, routineKind)
  )
}

/**
 * API 版本信息
 */
export type ApiVersionResponse = {
  version: string
  major: number
  minor: number
  patch: number
  codename: string
}

export async function getApiVersion(): Promise<ApiVersionResponse> {
  return commands.getApiVersion()
}

/**
 * 连接健康检查（ping）
 */
export async function pingConnection(connId?: string): Promise<boolean> {
  return typed(commands.pingConnection(connId ?? null))
}

/**
 * SQL 审计日志记录
 */
export interface SqlAuditRecord {
  id: string
  sql: string
  connId: string | null
  dbType: string | null
  executedAt: string
  durationMs: number | null
  success: boolean | null
  errorMessage: string | null
  rowsAffected: number | null
  rowsReturned: number | null
}

/**
 * 内省级别
 *
 * Level 1: 仅名称
 * Level 2: 名称 + 列元数据（不含源码）
 * Level 3: 全部（含例程源码）
 */
export type IntrospectionLevel = 'level1' | 'level2' | 'level3'

/**
 * 设置连接的内省级别
 */
export async function setIntrospectionLevel(
  connectionId: string,
  level: IntrospectionLevel
): Promise<void> {
  await typed(commands.setIntrospectionLevel(connectionId, level))
}

/**
 * 获取连接的内省级别
 */
export async function getIntrospectionLevel(connectionId: string): Promise<IntrospectionLevel> {
  return typed(commands.getIntrospectionLevel(connectionId)) as unknown as IntrospectionLevel
}

/**
 * 重置连接的内省级别为默认 Level 3
 */
export async function removeIntrospectionLevel(connectionId: string): Promise<void> {
  await typed(commands.removeIntrospectionLevel(connectionId))
}
