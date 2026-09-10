<template>
  <div ref="containerRef" class="main-content-area">
    <!-- SQL Editor Area -->
    <div
      class="sql-editor-area"
      :class="uiStore.isDark ? 'dockview-theme-dark' : 'dockview-theme-light'"
    >
      <DockviewVue ref="dockviewRef" class="dockview" @ready="onReady" />
    </div>
  </div>
</template>

<script setup lang="ts">
import { DockviewVue, type DockviewReadyEvent } from 'dockview-vue'
import { ref } from 'vue'

import { panelRegistry } from '@/core/panel-registry'
import { useUiStore } from '@/shared/stores/ui'

const uiStore = useUiStore()

const containerRef = ref<HTMLElement | null>(null)
const dockviewRef = ref<InstanceType<typeof DockviewVue> | null>(null)

// Event handlers
function onReady(event: DockviewReadyEvent) {
  const api = event.api

  // Get all registered panels
  const panels = panelRegistry.getAll()
  // eslint-disable-next-line no-console
  console.debug(`[MainContent] Creating ${panels.length} panels from registry`)

  // Filter panels by location (center/bottom)
  const centerPanels = panels.filter(p => p.location === 'center')

  // Create center panels (SQL Editor)
  let centerPanelId: string | null = null
  centerPanels.forEach((panel, index) => {
    const panelConfig = {
      id: `panel_${panel.id}`,
      component: panel.id,
      title: panel.name,
    } as const

    if (index === 0) {
      api.addPanel(panelConfig)
      centerPanelId = `panel_${panel.id}`
    } else if (centerPanelId) {
      ;(panelConfig as Record<string, unknown>).position = {
        referencePanel: centerPanelId,
        direction: 'within',
      }
      api.addPanel(panelConfig)
    }
  })
}
</script>

<style scoped>
.main-content-area {
  display: flex;
  flex-direction: column;
  width: 100%;
  height: 100%;
  overflow: hidden;
  background-color: var(--bg-primary, #1e1e1e);
}

.sql-editor-area {
  flex: 1;
  overflow: hidden;
}

.dockview {
  height: 100%;
  width: 100%;
}
</style>
