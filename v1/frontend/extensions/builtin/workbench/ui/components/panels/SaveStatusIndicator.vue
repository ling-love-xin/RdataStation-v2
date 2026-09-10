<template>
  <div class="save-status-indicator" :class="statusClass" :title="statusTooltip">
    <span class="save-status-icon">{{ statusIcon }}</span>
    <span v-if="showLabel" class="save-status-label">{{ statusLabel }}</span>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'

import type { SaveStatus } from '@/extensions/builtin/workbench/ui/composables/useFileSave'

interface Props {
  status: SaveStatus
  lastSaveTime?: number | null
  showLabel?: boolean
}

const props = withDefaults(defineProps<Props>(), {
  status: 'idle',
  lastSaveTime: null,
  showLabel: false,
})

const statusIcon = computed(() => {
  switch (props.status) {
    case 'saving':
      return '⟳'
    case 'saved':
      return '✓'
    case 'unsaved':
      return '●'
    case 'error':
      return '✗'
    case 'idle':
    default:
      return '✓'
  }
})

const statusLabel = computed(() => {
  switch (props.status) {
    case 'saving':
      return 'Saving...'
    case 'saved':
      return 'Saved'
    case 'unsaved':
      return 'Unsaved'
    case 'error':
      return 'Save Error'
    case 'idle':
    default:
      return ''
  }
})

const statusClass = computed(() => `save-status-${props.status}`)

const statusTooltip = computed(() => {
  if (props.status === 'saved' && props.lastSaveTime) {
    const date = new Date(props.lastSaveTime)
    return `Last saved: ${date.toLocaleTimeString()}`
  }
  switch (props.status) {
    case 'saving':
      return 'Saving file...'
    case 'unsaved':
      return 'File has unsaved changes'
    case 'error':
      return 'Save failed - click to retry'
    default:
      return ''
  }
})
</script>

<style scoped>
.save-status-indicator {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 0 4px;
  border-radius: 2px;
  font-size: 12px;
  white-space: nowrap;
  opacity: 0.85;
  transition: opacity 0.2s ease;
}

.save-status-indicator:hover {
  opacity: 1;
}

.save-status-saved .save-status-icon {
  color: inherit;
}

.save-status-unsaved .save-status-icon {
  color: #fdcb6e;
}

.save-status-saving .save-status-icon {
  animation: save-spin 1s linear infinite;
  color: inherit;
}

.save-status-error .save-status-icon {
  color: #d63031;
}

.save-status-idle .save-status-icon {
  color: inherit;
  opacity: 0.6;
}

.save-status-label {
  opacity: 0.8;
}

@keyframes save-spin {
  from {
    transform: rotate(0deg);
  }
  to {
    transform: rotate(360deg);
  }
}
</style>
