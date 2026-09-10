<template>
  <div class="minimal-editor-tab">
    <span class="tab-label">{{ params?.title ?? '' }}</span>
  </div>
</template>

<script setup lang="ts">
defineProps<{ params?: Record<string, unknown> }>()
</script>

<style>
.dv-group-view:has(.minimal-editor-tab) .tabs-and-actions-container {
  height: 24px !important;
  min-height: 24px !important;
  background: var(--tab-bg, #1e1e1e) !important;
}
.dv-group-view:has(.minimal-editor-tab) .void-container {
  height: 24px !important;
}
</style>

<style scoped>
.minimal-editor-tab {
  display: flex;
  align-items: center;
  height: 24px;
  padding: 0 8px;
  font-size: 11px;
  color: var(--dv-tab-inactive-foreground, #858585);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  max-width: 100px;
}
.tab-label {
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>
