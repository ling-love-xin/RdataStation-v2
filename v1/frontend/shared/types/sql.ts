/**
 * SQL 编辑器共享类型定义
 */

export type SqlDialect =
  | 'generic'
  | 'mysql'
  | 'postgres'
  | 'sqlite'
  | 'duckdb'
  | 'mssql'
  | 'oracle'
  | 'snowflake'
  | 'bigquery'
  | 'redshift'

export type DatabaseType = SqlDialect

export interface SqlEditorParams {
  connectionId?: string
  databaseName?: string
  initialSql?: string
  panelId?: string
  schema?: string
  scratchpadRelativePath?: string
  scratchpadFileName?: string
  language?: string
  initialLine?: number
}
