<template>
  <div class="analysis-editor-panel">
    <AnalysisToolbar
      :is-executing="isExecuting"
      @execute="handleExecute"
      @execute-new="handleExecuteNew"
      @format="handleFormat"
      @federation-change="handleFederationChange"
    />
    <EditorBody ref="editorBodyRef" :params="props.params" />
    <AnalysisStatusBar
      :connection-name="'DuckDB (本地分析引擎)'"
      :cursor-position="cursorPosition"
      :last-execution-time="lastExecutionTime"
      :federation-sources="federationSources"
    />
  </div>
</template>

<script setup lang="ts">
import { ref, computed } from 'vue'

import { EditorManager } from '@/extensions/builtin/workbench/manager/EditorManager'
import type { EditorPanelParams } from '@/extensions/builtin/workbench/types/editor-types'
import { useSqlExecution } from '@/extensions/builtin/workbench/ui/composables/useSqlExecution'

import AnalysisStatusBar from './AnalysisStatusBar.vue'
import AnalysisToolbar from './AnalysisToolbar.vue'
import EditorBody from './EditorBody.vue'

const props = defineProps<{
  params: EditorPanelParams
}>()

const editorBodyRef = ref<InstanceType<typeof EditorBody> | null>(null)
const federationSources = ref<string[]>([])

const runtimeConnId = ref('__duckdb_local__')
const connectionName = computed(() => 'DuckDB (本地分析引擎)')

const {
  executing: isExecuting,
  lastExecutionTime,
  executeNewTab,
  executeDuckDBAccelerated,
} = useSqlExecution({
  panelId: props.params.filePath,
  editorId: props.params.filePath,
  getEditorValue: () => editorBodyRef.value?.getEditorValue?.() ?? '',
  getSelectedText: () => editorBodyRef.value?.getSelectedText?.() ?? '',
  runtimeConnId,
  currentConnectionName: connectionName,
})

const cursorPosition = ref('Ln 1, Col 1')

function handleExecute(): void {
  executeDuckDBAccelerated()
}

function handleExecuteNew(): void {
  executeNewTab()
}

function handleFormat(): void {
  EditorManager.formatSQL()
}

function handleFederationChange(sources: string[]): void {
  federationSources.value = sources
}
</script>

<style scoped>
.analysis-editor-panel {
  display: flex;
  flex-direction: column;
  height: 100%;
  overflow: hidden;
}
</style>