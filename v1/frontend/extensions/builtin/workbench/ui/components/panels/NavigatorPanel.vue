<template>
  <div class="navigator-panel">
    <DatabaseNavigator />
  </div>
</template>

<script setup lang="ts">
import DatabaseNavigator from '@/extensions/builtin/database/ui/components/database-navigator.vue'
</script>

<style scoped>
.navigator-panel {
  height: 100%;
  overflow: auto;
}
</style>
