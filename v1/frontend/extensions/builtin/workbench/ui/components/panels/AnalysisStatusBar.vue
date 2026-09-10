<template>
  <div class="analysis-statusbar">
    <span class="statusbar-item">{{ connectionName }}</span>
    <span class="statusbar-separator">|</span>
    <span class="statusbar-item">{{ cursorPosition }}</span>
    <span v-if="federationSources.length > 0" class="statusbar-separator">|</span>
    <span v-if="federationSources.length > 0" class="statusbar-item statusbar-federation">
      联邦: {{ federationSources.length }} 数据源
    </span>
    <span v-if="lastExecutionTime !== null" class="statusbar-separator">|</span>
    <span v-if="lastExecutionTime !== null" class="statusbar-item">{{ lastExecutionTime }}ms</span>
    <span class="statusbar-spacer" />
    <span class="statusbar-item">DuckDB</span>
  </div>
</template>

<script setup lang="ts">
defineProps<{
  connectionName: string
  cursorPosition: string
  lastExecutionTime: number | null
  federationSources: string[]
}>()
</script>

<style scoped>
.analysis-statusbar {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 2px 10px;
  background: var(--statusbar-bg, #f0c040);
  color: var(--statusbar-fg, #333333);
  font-size: 12px;
  flex-shrink: 0;
  min-height: 22px;
  user-select: none;
}
.statusbar-item {
  white-space: nowrap;
  opacity: 0.9;
}
.statusbar-separator {
  opacity: 0.4;
}
.statusbar-spacer {
  flex: 1;
}
.statusbar-federation {
  color: #0066b8;
  font-weight: 500;
}
</style>
