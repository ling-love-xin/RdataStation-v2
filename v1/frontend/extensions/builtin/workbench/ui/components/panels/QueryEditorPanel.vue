<template>
  <div class="query-editor-panel">
    <QueryToolbar
      :is-executing="isExecuting"
      :execution-mode="executionMode"
      :connection-name="connectionName"
      :in-transaction="inTransaction"
      @execute="handleExecute"
      @execute-new="handleExecuteNew"
      @accelerate="handleAccelerate"
      @explain="handleExplain"
      @format="handleFormat"
      @transpile="handleTranspile"
      @mode-change="handleModeChange"
      @begin-transaction="handleBeginTransaction"
      @commit-transaction="handleCommitTransaction"
      @rollback-transaction="handleRollbackTransaction"
    />
    <EditorBody ref="editorBodyRef" :params="props.params" :extra-extensions="extraExtensions" />
    <QueryStatusBar
      :connection-name="connectionName"
      :cursor-position="cursorPosition"
      :statement-count="statementCount"
      :in-transaction="inTransaction"
      :last-execution-time="lastExecutionTime"
      :editor-mode="editorMode"
    />
  </div>
</template>

<script setup lang="ts">
import { autocompletion } from '@codemirror/autocomplete'
import { useMessage } from 'naive-ui'
import { ref, computed, onMounted, watch } from 'vue'

import { EditorManager } from '@/extensions/builtin/workbench/manager/EditorManager'
import { createSchemaCompletionSource } from '@/extensions/builtin/workbench/services/schema-completion-service'
import type { EditorPanelParams } from '@/extensions/builtin/workbench/types/editor-types'
import { useSqlExecution } from '@/extensions/builtin/workbench/ui/composables/useSqlExecution'
import { tauriInvoke } from '@/shared/api'
import type { SqlDialect } from '@/shared/types/sql'

import EditorBody from './EditorBody.vue'
import QueryStatusBar from './QueryStatusBar.vue'
import QueryToolbar from './QueryToolbar.vue'

import type { Extension } from '@codemirror/state'

const props = defineProps<{
  params: EditorPanelParams
}>()

const message = useMessage()
const editorBodyRef = ref<InstanceType<typeof EditorBody> | null>(null)

const runtimeConnId = ref<string>('')
const connectionName = computed(() => EditorManager.activeFileInfo?.connectionId ?? '')

const {
  executing: isExecuting,
  lastExecutionTime,
  inTransaction,
  statementCount,
  scheduleParse,
  executeSingleStatement,
  executeNewTab,
  executeDuckDBAccelerated,
  beginTransaction,
  commitTransaction,
  rollbackTransaction,
} = useSqlExecution({
  panelId: props.params.filePath,
  editorId: props.params.filePath,
  getEditorValue: () => editorBodyRef.value?.getEditorValue?.() ?? '',
  getSelectedText: () => editorBodyRef.value?.getSelectedText?.() ?? '',
  runtimeConnId,
  currentConnectionName: connectionName,
})

const cursorPosition = ref('Ln 1, Col 1')
const editorMode = computed(() => 'SQL')
const executionMode = ref<'normal' | 'analysis' | 'smart'>('normal')

/** Schema 感知自动补全扩展 */
const extraExtensions = computed<Extension[]>(() => {
  const connId = runtimeConnId.value
  if (!connId) return []

  const completionSource = createSchemaCompletionSource(() => {
    if (!runtimeConnId.value) return null
    // 默认使用 public schema，后续可扩展为从上下文获取
    return {
      connId: runtimeConnId.value,
      dbName: 'default',
      schemaName: 'public',
      connectionType: 'global',
    }
  })

  return [autocompletion({ override: [completionSource] })]
})

// 从 EditorBody 同步光标位置到状态栏
watch(
  () => editorBodyRef.value?.cursorPosition,
  (pos) => {
    if (pos) {
      cursorPosition.value = pos
      const selText = editorBodyRef.value?.selectionText
      if (selText) {
        cursorPosition.value = `${pos} ${selText}`
      }
    }
  }
)

function handleExecute() {
  executeSingleStatement()
}

function handleExecuteNew() {
  executeNewTab()
}

function handleAccelerate() {
  executeDuckDBAccelerated()
}

function handleFormat() {
  const sql = editorBodyRef.value?.getEditorValue?.() || ''
  if (!sql.trim()) {
    message.warning('No SQL to format')
    return
  }
  EditorManager.formatSQL()
}

/** 方言选择器值 → SqlDialect 映射 */
const DIALECT_MAP: Record<string, SqlDialect> = {
  mysql: 'mysql',
  postgresql: 'postgres',
  postgres: 'postgres',
  sqlite: 'sqlite',
  tsql: 'mssql',
  mssql: 'mssql',
  duckdb: 'duckdb',
  oracle: 'oracle',
  bigquery: 'bigquery',
  redshift: 'redshift',
  generic: 'generic',
}

function handleExplain() {
  const sql = editorBodyRef.value?.getSelectedText?.() || editorBodyRef.value?.getEditorValue?.() || ''
  if (!sql.trim()) {
    message.warning('No SQL to explain')
    return
  }
  const explainSql = `EXPLAIN ${sql}`
  // 设置编辑器内容为 EXPLAIN SQL 并执行
  editorBodyRef.value?.setEditorValue?.(explainSql)
  executeSingleStatement()
}

async function handleTranspile(dialect: string) {
  const sql = editorBodyRef.value?.getEditorValue?.() || ''
  if (!sql.trim()) {
    message.warning('No SQL to transpile')
    return
  }
  const targetDialect = DIALECT_MAP[dialect] || 'generic'
  try {
    const response = await tauriInvoke<{ transpiled_sql: string; success: boolean; error: string | null }>(
      'transpile_sql',
      {
        input: {
          sql,
          source_dialect: 'generic',
          target_dialect: targetDialect,
        },
      }
    )
    if (response.success && response.transpiled_sql && response.transpiled_sql !== sql) {
      editorBodyRef.value?.setEditorValue?.(response.transpiled_sql)
      message.success(`已转换为 ${dialect} 方言`)
    } else if (response.success) {
      message.info('转换结果与原 SQL 相同')
    } else {
      message.warning(response.error || '转换失败，请检查 SQL 语法')
    }
  } catch (e) {
    message.error(`转换失败: ${String(e)}`)
  }
}

function handleModeChange(mode: 'normal' | 'analysis' | 'smart') {
  executionMode.value = mode
}

function handleBeginTransaction() {
  beginTransaction()
}

function handleCommitTransaction() {
  commitTransaction()
}

function handleRollbackTransaction() {
  rollbackTransaction()
}

onMounted(() => {
  const info = EditorManager.activeFileInfo
  if (info?.connectionId) {
    runtimeConnId.value = info.connectionId
  }
  scheduleParse()
})
</script>

<style scoped>
.query-editor-panel {
  display: flex;
  flex-direction: column;
  height: 100%;
  overflow: hidden;
}
</style>