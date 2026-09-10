<template>
  <div class="right-sidebar">
    <component :is="currentComponent" />
  </div>
</template>

<script setup lang="ts">
import { computed, defineAsyncComponent } from 'vue'

import { useLayoutStore } from '@/extensions/builtin/workbench/ui/stores/layout-store'

const layoutStore = useLayoutStore()

const SqlHistoryComponent = defineAsyncComponent(
  () => import('@/extensions/builtin/workbench/ui/components/panels/SqlHistoryPanel.vue')
)

const OutputComponent = defineAsyncComponent(
  () => import('@/extensions/builtin/workbench/ui/components/panels/EmptyWorkbenchPanel.vue')
)

const ColumnInsightsComponent = defineAsyncComponent(
  () => import('@/extensions/builtin/workbench/ui/components/panels/ColumnInsightsPanel.vue')
)

const RightSidebarPlaceholder = defineAsyncComponent(
  () => import('@/extensions/builtin/workbench/ui/components/panels/EmptyWorkbenchPanel.vue')
)

const currentComponent = computed(() => {
  switch (layoutStore.selectedRightItem) {
    case 'sql-history':
      return SqlHistoryComponent
    case 'output':
      return OutputComponent
    case 'column-insights':
      return ColumnInsightsComponent
    default:
      return RightSidebarPlaceholder
  }
})
</script>

<style scoped>
.right-sidebar {
  width: 100%;
  height: 100%;
  overflow: hidden;
  background-color: var(--bg-secondary, #252526);
}
</style>
