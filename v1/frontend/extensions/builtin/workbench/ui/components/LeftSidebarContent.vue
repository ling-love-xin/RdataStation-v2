<template>
  <div class="left-sidebar">
    <component :is="currentComponent" />
  </div>
</template>

<script setup lang="ts">
import { computed, defineAsyncComponent } from 'vue'

import { useLayoutStore } from '@/extensions/builtin/workbench/ui/stores/layout-store'

const layoutStore = useLayoutStore()

const DatabaseNavComponent = defineAsyncComponent(
  () => import('@/extensions/builtin/database/ui/components/database-navigator.vue')
)

const AnalyticsResourceComponent = defineAsyncComponent(
  () => import('@/extensions/builtin/analytics-resource/ui/components/AnalyticsResourceManager.vue')
)

const currentComponent = computed(() => {
  switch (layoutStore.selectedLeftItem) {
    case 'database':
      return DatabaseNavComponent
    case 'analytics':
      return AnalyticsResourceComponent
    case 'plugins':
      return { template: '<div class="placeholder-message">Plugins — Coming soon</div>' }
    case 'settings':
      return { template: '<div class="placeholder-message">Settings — Coming soon</div>' }
    default:
      return DatabaseNavComponent
  }
})
</script>

<style scoped>
.left-sidebar {
  width: 100%;
  height: 100%;
  overflow: hidden;
  background-color: var(--bg-secondary, #252526);
}
</style>
