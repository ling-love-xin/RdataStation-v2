<template>
  <div
    class="icon-tab"
    :class="{ 'icon-tab--edge': isEdgeGroup }"
    :title="isEdgeGroup ? titleText : undefined"
  >
    <component :is="iconComponent" :size="14" class="icon-tab-icon" />
    <span v-if="tabDirty && !isEdgeGroup" class="icon-tab-dirty" />
    <span v-if="!isEdgeGroup" class="icon-tab-title">{{ titleText }}</span>
  </div>
</template>

<script setup lang="ts">
import {
  Database,
  BarChart3,
  Puzzle,
  FileText,
  FileCode,
  Sparkles,
  StickyNote,
  Dices,
  Layout,
} from 'lucide-vue-next'
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'

import { panelRegistry } from '@/core/panel-registry'
import { useTabDirtyState } from '@/extensions/builtin/workbench/ui/composables/useTabDirtyState'

import type { Component } from 'vue'

interface TabApi {
  id: string
  title?: string
  onDidTitleChange?: (listener: (e: { title: string }) => void) => { dispose: () => void }
}

const props = defineProps<{
  params: {
    api: TabApi
    title?: string
    params?: Record<string, unknown>
    containerApi?: unknown
    groupApi?: unknown
    tabLocation?: string
  }
}>()

const PANEL_ICONS: Record<string, Component> = {
  scratchpad: StickyNote,
  databaseNavigator: Database,
  'analytics-resource-manager': BarChart3,
  plugins: Puzzle,
  sqlHistory: FileText,
  mockPanel: Dices,
  columnInsights: Sparkles,
  emptyWorkbench: Layout,
  sqlEditor: Database,
  codeEditor: FileCode,
  queryResult: BarChart3,
  multiTabResult: BarChart3,
  dynamicObjectProperties: FileText,
}

const componentId = computed(() => {
  const panelId = props.params.api?.id || ''
  return panelId.replace(/^panel_/, '').replace(/_\d+$/, '')
})

const iconComponent = computed(() => {
  return PANEL_ICONS[componentId.value] || Layout
})

const isEdgeGroup = computed(() => {
  const desc = panelRegistry.get(componentId.value)
  return desc?.location === 'left' || desc?.location === 'right'
})

const titleText = ref('')

const { isDirty } = useTabDirtyState()

const tabDirty = computed(() => isDirty(props.params.api?.id || ''))

let titleDisposable: { dispose: () => void } | null = null

onMounted(() => {
  const api = props.params.api
  titleText.value = api?.title || ''

  if (api?.onDidTitleChange) {
    titleDisposable = api.onDidTitleChange((e: { title: string }) => {
      titleText.value = e.title
    })
  }
})

onBeforeUnmount(() => {
  titleDisposable?.dispose()
  titleDisposable = null
})
</script>

<style scoped>
.icon-tab {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 0 4px;
  min-width: 0;
  overflow: hidden;
}

.icon-tab-icon {
  flex-shrink: 0;
  opacity: 0.8;
}

.icon-tab-title {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
  color: inherit;
}

.icon-tab-dirty {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background-color: currentColor;
  opacity: 0.7;
  flex-shrink: 0;
}

.icon-tab--edge {
  justify-content: center;
  gap: 0;
}

.icon-tab--edge .icon-tab-icon {
  opacity: 0.9;
}
</style>
