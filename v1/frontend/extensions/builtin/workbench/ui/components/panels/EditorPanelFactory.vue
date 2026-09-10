<template>
  <div class="editor-panel-factory">
    <QueryEditorPanel v-if="currentEditorType === 'query'" :params="props.params" />
    <AnalysisEditorPanel v-if="currentEditorType === 'analysis'" :params="props.params" />
    <CodeEditorPanel v-if="currentEditorType === 'code'" :params="props.params" />
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'

import { EditorModeResolver } from '@/extensions/builtin/workbench/manager/EditorModeResolver'
import type {
  EditorType,
  EditorPanelParams,
} from '@/extensions/builtin/workbench/types/editor-types'

import AnalysisEditorPanel from './AnalysisEditorPanel.vue'
import CodeEditorPanel from './CodeEditorPanel.vue'
import QueryEditorPanel from './QueryEditorPanel.vue'

const props = defineProps<{
  params: EditorPanelParams
}>()

const currentEditorType = computed<EditorType>(() =>
  EditorModeResolver.resolve(props.params.filePath, props.params.language)
)
</script>

<style scoped>
.editor-panel-factory {
  display: flex;
  flex-direction: column;
  height: 100%;
  overflow: hidden;
}
</style>
