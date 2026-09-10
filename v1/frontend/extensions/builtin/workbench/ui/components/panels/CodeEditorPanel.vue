<template>
  <div class="code-editor-panel">
    <CodeToolbar
      :is-dirty="isDirty"
      @save="handleSave"
      @format="handleFormat"
      @undo="handleUndo"
      @redo="handleRedo"
    />
    <EditorBody ref="editorBodyRef" :params="props.params" />
    <CodeStatusBar
      :language="language"
      :encoding="encoding"
      :indent="indent"
      :cursor-position="cursorPosition"
      :diagnostic-count="diagnosticCount"
    />
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'

import { EditorManager } from '@/extensions/builtin/workbench/manager/EditorManager'
import type { EditorPanelParams } from '@/extensions/builtin/workbench/types/editor-types'
import type { LspDiagnostic } from '@/extensions/builtin/workbench/types/lsp-types'

import CodeStatusBar from './CodeStatusBar.vue'
import CodeToolbar from './CodeToolbar.vue'
import EditorBody from './EditorBody.vue'

const props = defineProps<{
  params: EditorPanelParams
}>()

const editorBodyRef = ref<InstanceType<typeof EditorBody> | null>(null)
const isDirty = ref(false)
const cursorPosition = ref('Ln 1, Col 1')
const diagnostics = ref<LspDiagnostic[]>([])
const language = computed(() => props.params.language || 'plaintext')
const encoding = ref('UTF-8')
const indent = ref('4空格')

const diagnosticCount = computed(() => ({
  errors: diagnostics.value.filter(d => d.severity === 'error').length,
  warnings: diagnostics.value.filter(d => d.severity === 'warning').length,
  infos: diagnostics.value.filter(d => d.severity === 'info' || d.severity === 'hint').length,
}))

function handleSave() {
  EditorManager.saveCurrentFile()
  isDirty.value = false
}

function handleFormat() {
  EditorManager.formatSQL()
}

function handleUndo() {
  const cmView = editorBodyRef.value?.view
  if (cmView) {
    cmView.dispatch({})
  }
}

function handleRedo() {
  const cmView = editorBodyRef.value?.view
  if (cmView) {
    cmView.dispatch({})
  }
}

const lspExtension = {
  async getDiagnostics(_filePath: string, _content: string): Promise<LspDiagnostic[]> {
    return []
  },

  async getCompletions(_filePath: string, _position: { line: number; character: number }) {
    return []
  },

  async getHover(_filePath: string, _position: { line: number; character: number }) {
    return null
  },
}

onMounted(() => {
  void lspExtension
})
</script>

<style scoped>
.code-editor-panel {
  display: flex;
  flex-direction: column;
  height: 100%;
  overflow: hidden;
}
</style>
