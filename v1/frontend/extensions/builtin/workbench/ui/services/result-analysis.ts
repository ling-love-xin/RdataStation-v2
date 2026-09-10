/**
 * 结果集分析服务 + 洞察 API
 *
 * 前后端对接层：所有与结果分析相关的 Tauri invoke 封装
 */
import { invoke } from '@tauri-apps/api/core'

import type { MultiRuleResult } from '../types/result'

export type { MultiRuleResult }

// ==================== 基础接口 ====================

export interface ResultSet {
  columns: string[]
  rows: unknown[][]
  row_count: number
  elapsed_ms: number
  temp_table: string
}

// ==================== 洞察体系类型 ====================

export interface ColumnInsightFull {
  stats: ColumnStats
  sample: unknown[]
  histogram: DistributionBin[] | null
}

export interface ColumnStats {
  column_name: string
  data_type: string
  total_count: number
  null_count: number
  null_rate: number
  unique_count: number | null
  stats_detail: ColumnStatsDetail
}

export type ColumnStatsDetail =
  | NumericStatsDetail
  | TextStatsDetail
  | DateTimeStatsDetail
  | BooleanStatsDetail
  | UnknownStatsDetail

export interface NumericStatsDetail {
  kind: 'Numeric'
  min: number
  max: number
  avg: number
  median: number
  p25: number
  p75: number
  sum: number
  stddev: number | null
  skewness: number | null
  kurtosis: number | null
  is_extreme: ExtremeValue[]
}

export interface ExtremeValue {
  value: number
  kind: string
}

export interface TextStatsDetail {
  kind: 'Text'
  min_length: number
  max_length: number
  top_values: TextFrequency[]
}

export interface TextFrequency {
  value: string
  count: number
  ratio: number
}

export interface DateTimeStatsDetail {
  kind: 'DateTime'
  earliest: string
  latest: string
  span_days: number
  monthly_distribution: TextFrequency[]
}

export interface BooleanStatsDetail {
  kind: 'Boolean'
  true_count: number
  false_count: number
  true_ratio: number
}

export interface UnknownStatsDetail {
  kind: 'Unknown'
}

export interface DistributionBin {
  label: string
  count: number
  ratio: number
}

// ==================== API 方法 ====================

export async function reExecuteWithFilter(
  connId: string,
  originalSql: string,
  whereClause?: string,
  orderClause?: string
): Promise<ResultSet> {
  return invoke<ResultSet>('re_execute_with_filter', {
    input: {
      conn_id: connId,
      original_sql: originalSql,
      where_clause: whereClause || null,
      order_clause: orderClause || null,
    },
  })
}

export async function executeDuckdbAnalysis(
  tempTable: string,
  sql: string,
  columns?: string[],
  rows?: unknown[][]
): Promise<ResultSet> {
  return invoke<ResultSet>('execute_duckdb_analysis', {
    input: {
      temp_table: tempTable,
      sql,
      columns: columns || null,
      rows: rows || null,
    },
  })
}

export async function getColumnInsights(
  tempTable: string,
  columnName: string
): Promise<ColumnStats> {
  return invoke<ColumnStats>('get_column_insights', {
    input: {
      temp_table: tempTable,
      column_name: columnName,
    },
  })
}

export async function getColumnInsightFull(
  tempTable: string,
  columnName: string
): Promise<ColumnInsightFull> {
  return invoke<ColumnInsightFull>('get_column_insight_full', {
    input: {
      temp_table: tempTable,
      column_name: columnName,
    },
  })
}

export async function createDuckdbTempTable(columns: string[], rows: unknown[][]): Promise<string> {
  return invoke<string>('create_duckdb_temp_table', {
    input: {
      columns,
      rows,
    },
  })
}

// ═══════════════════ 持久化 API ═══════════════════

export interface SaveInsightSnapshotInput {
  temp_table: string
  column_name: string
  conn_id?: string
  db_name?: string
  schema_name?: string
  table_name?: string
}

export interface InsightVersionEntry {
  snapshot_id: string
  column_name: string
  data_type: string | null
  stats_json: string
  version_id: string
  parent_version_id: string | null
  checksum: string
  created_at: string
}

export interface InsightStorageStats {
  total_snapshots: number
  unique_columns: number
  total_size_bytes: number
  total_size_display: string
}

export interface CleanupResult {
  duckdb_deleted: number
  sqlite_deleted: number
}

export async function saveColumnInsightSnapshot(input: SaveInsightSnapshotInput): Promise<string> {
  return invoke<string>('save_column_insight_snapshot', {
    input,
  })
}

export async function getColumnInsightHistory(columnName: string): Promise<InsightVersionEntry[]> {
  return invoke<InsightVersionEntry[]>('get_column_insight_history', {
    input: {
      column_name: columnName,
    },
  })
}

export async function cleanupInsightSnapshots(days: number): Promise<CleanupResult> {
  return invoke<CleanupResult>('cleanup_insight_snapshots', {
    input: {
      days,
    },
  })
}

export async function getInsightStorageStats(): Promise<InsightStorageStats> {
  return invoke<InsightStorageStats>('get_insight_storage_stats')
}

// ═══════════════════ 规则引擎 API ═══════════════════

export interface RenderHint {
  component: string | null
  display_order: number | null
}

export interface RuleMeta {
  id: string
  name: string
  description: string
  version: string
  category: string
  applies_to: string[]
  builtin: boolean
  parameters: string[]
  result_type: string | null
  render: RenderHint | null
}

export interface ExecuteRuleInput {
  rule_id: string
  params: Record<string, string>
  temp_table: string
}

export async function executeInsightRule(input: ExecuteRuleInput): Promise<MultiRuleResult> {
  return invoke<MultiRuleResult>('execute_insight_rule', { input })
}

export async function listInsightRules(category?: string): Promise<RuleMeta[]> {
  return invoke<RuleMeta[]>('list_insight_rules', { category: category ?? null })
}

export async function reloadInsightRules(projectPath: string): Promise<number> {
  return invoke<number>('reload_insight_rules', {
    input: { project_path: projectPath },
  })
}

export async function listRulesForColumn(columnType: string): Promise<RuleMeta[]> {
  return invoke<RuleMeta[]>('list_rules_for_column', {
    input: { column_type: columnType },
  })
}

// ═══════════════════ 表探查 API ═══════════════════

export interface TableProfile {
  table_name: string
  db_type: string
  columns: TableColumnMeta[]
  row_count: number | null
  schema_name: string | null
}

export interface TableColumnMeta {
  column_name: string
  data_type: string
  is_nullable: boolean
  is_primary_key: boolean
  ordinal_position: number
}

// ═══════════════════ 质量评分 ═══════════════════

export interface QualityScore {
  column_name: string
  overall_score: number
  level: string
  dimensions: QualityDimension[]
  summary: string
}

export interface QualityDimension {
  name: string
  score: number
  weight: number
  detail: string
}

export interface GetColumnQualityInput {
  column_name: string
  temp_table: string
}

export async function getColumnQuality(input: GetColumnQualityInput): Promise<QualityScore> {
  return invoke<QualityScore>('get_column_quality', { input })
}

// ═══════════════════ 表质量评估 ═══════════════════

export interface TableQuality {
  table_name: string
  overall_score: number
  level: string
  column_scores: ColumnQualityEntry[]
  summary: string
  scored_count: number
  total_columns: number
}

export interface ColumnQualityEntry {
  column_name: string
  quality_score: number
  level: string
  null_rate: number
}

export interface BatchEvaluateInput {
  conn_id: string
  database: string
  schema: string
  table: string
}

export async function batchEvaluateColumns(input: BatchEvaluateInput): Promise<TableQuality> {
  return invoke<TableQuality>('batch_evaluate_columns', { input })
}

// ═══════════════════ Schema 洞察 ═══════════════════

export interface SchemaInsightReport {
  schema_name: string
  table_count: number
  total_columns: number
  fk_candidates: ForeignKeyCandidate[]
  type_mismatches: TypeMismatch[]
  orphan_tables: OrphanTable[]
  redundant_columns: RedundantColumn[]
  summary: string
  health_score: number
  health_level: string
}

export interface ForeignKeyCandidate {
  source_table: string
  source_column: string
  target_table: string
  target_column: string
  confidence: string
  naming_pattern: string
}

export interface TypeMismatch {
  column_name: string
  tables: TypeMismatchEntry[]
  severity: string
}

export interface TypeMismatchEntry {
  table_name: string
  data_type: string
}

export interface OrphanTable {
  table_name: string
  column_count: number
  reason: string
}

export interface RedundantColumn {
  column_name: string
  table_count: number
  tables: string[]
  suggestion: string
}

export interface SchemaInsightInput {
  conn_id: string
  database: string
  schema: string
}

export async function getSchemaInsight(input: SchemaInsightInput): Promise<SchemaInsightReport> {
  return invoke<SchemaInsightReport>('get_schema_insight', { input })
}

// ═══════════════════ 表探查 ═══════════════════

export interface GetTableProfileInput {
  conn_id: string
  db_type: string
  database: string
  schema: string
  table: string
}

export async function getTableProfile(input: GetTableProfileInput): Promise<TableProfile> {
  return invoke<TableProfile>('get_table_profile', { input })
}

// ═══════════════════ 版本详情 API ═══════════════════

export async function getInsightVersionDetail(
  versionId: string
): Promise<ColumnInsightFull | null> {
  return invoke<ColumnInsightFull | null>('get_insight_version_detail', {
    input: { version_id: versionId },
  })
}

// ═══════════════════ 表列探查 API ═══════════════════

export interface ProfileColumnFromTableInput {
  conn_id: string
  database: string
  schema: string
  table: string
  column_name: string
}

export async function profileColumnFromTable(
  input: ProfileColumnFromTableInput
): Promise<ColumnInsightFull> {
  return invoke<ColumnInsightFull>('profile_column_from_table', { input })
}

// ═══════════════════ 单元格编辑持久化 ═══════════════════

export interface CellUpdateInput {
  conn_id: string
  table_name: string
  column_name: string
  new_value: unknown
  row_identity: Record<string, unknown>
}

export interface CellUpdateResult {
  success: boolean
  affected_rows: number
  message: string
}

export async function saveCellUpdate(input: CellUpdateInput): Promise<CellUpdateResult> {
  return invoke<CellUpdateResult>('save_cell_update', { input })
}

// ═══════════════════ 数据导出 ═══════════════════

export interface ExportResultInput {
  temp_table: string
  file_path: string
  format: string
}

export async function exportResultToFile(input: ExportResultInput): Promise<string> {
  return invoke<string>('export_result_to_file', { input })
}
