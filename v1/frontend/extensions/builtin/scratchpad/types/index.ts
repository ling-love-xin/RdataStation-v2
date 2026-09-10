export type ScratchpadEntryKind = 'file' | 'folder'

export interface ScratchpadEntry {
  name: string
  path: string
  kind: ScratchpadEntryKind
  size: number
  modified_at: string | null
  children?: ScratchpadEntry[]
}

export interface SearchMatch {
  file: string
  line_number: number
  line_content: string
  before_context: string[]
  after_context: string[]
}

export interface SearchResult {
  matches: SearchMatch[]
  total_files_scanned: number
  total_files_skipped: number
  skipped_files: string[]
  truncated: boolean
}

export interface ExternalReference {
  alias: string
  path: string
  created_at: string
}

export interface FileMeta {
  last_connection_id?: string
  last_executed_at?: string
}

export interface AnalyzableFile {
  name: string
  relative_path: string
  file_type: string
  size_bytes: number
  duckdb_query_hint: string
}

export interface ScratchpadResponse {
  local_entries: ScratchpadEntry[]
  external_references: ExternalReference[]
  scratchpad_path: string
  file_meta: Record<string, FileMeta>
}

export interface PromoteResult {
  resource: AnalyticsResourceBrief
  removed: boolean
}

export interface AnalyticsResourceBrief {
  id: string
  resource_type: string
  name: string
  scope: string
}

export interface ScratchpadChangeEntry {
  path: string
  kind: string
}

export interface ScratchpadChangeEvent {
  changes: ScratchpadChangeEntry[]
}

export interface ReplaceResult {
  replaced: number
  file_path: string
}

export type DiffLineKind = 'unchanged' | 'added' | 'removed'

export interface DiffLine {
  line_number_left: number | null
  line_number_right: number | null
  kind: DiffLineKind
  content: string
}

export interface DiffResult {
  lines: DiffLine[]
  left_label: string
  right_label: string
}
