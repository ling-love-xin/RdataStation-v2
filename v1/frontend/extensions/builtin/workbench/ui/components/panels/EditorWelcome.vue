<template>
  <div v-if="visible" class="editor-watermark">
    <div class="watermark-content">
      <div class="watermark-text">
        <div class="watermark-title">{{ $t('sqlEditor.title') }}</div>
        <div class="watermark-shortcuts">
          <span class="shortcut-hint"
            ><kbd>Ctrl</kbd>+<kbd>Enter</kbd> {{ $t('sqlEditor.shortcutExecute') }}</span
          >
          <span class="shortcut-hint"
            ><kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>F</kbd>
            {{ $t('sqlEditor.shortcutFormat') }}</span
          >
          <span class="shortcut-hint"
            ><kbd>Ctrl</kbd>+<kbd>/</kbd> {{ $t('sqlEditor.shortcutComment') }}</span
          >
          <span class="shortcut-hint"><kbd>F5</kbd> {{ $t('sqlEditor.shortcutExecuteAll') }}</span>
        </div>
      </div>

      <div v-if="recentConnections.length > 0" class="welcome-recent">
        <div class="recent-title">{{ $t('sqlEditor.recentConnections') }}</div>
        <div class="recent-list">
          <div
            v-for="conn in recentConnections.slice(0, 5)"
            :key="conn.connId"
            class="recent-item"
            @click="handleConnect(conn.connId)"
          >
            <Database :size="14" class="recent-icon" />
            <span class="recent-name">{{ conn.name || conn.connId }}</span>
            <span class="recent-type">{{ conn.dbType.toUpperCase() }}</span>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { Database } from 'lucide-vue-next'
import { computed } from 'vue'

import { useConnectionStore } from '@/extensions/builtin/connection/ui/stores/connection-store'

interface Props {
  visible: boolean
}

const _props = defineProps<Props>()

const emit = defineEmits<{
  (e: 'connect', connId: string): void
}>()

const connectionStore = useConnectionStore()

const recentConnections = computed(() => {
  return connectionStore.connections.slice(0, 10)
})

function handleConnect(connId: string): void {
  emit('connect', connId)
}
</script>

<style scoped>
.editor-watermark {
  position: absolute;
  top: 40px;
  left: 0;
  right: 0;
  bottom: 24px;
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1;
}

.watermark-content {
  text-align: center;
}

.watermark-text {
  opacity: 0.35;
  user-select: none;
}

.watermark-title {
  font-size: 20px;
  font-weight: 600;
  color: var(--text-primary, #cccccc);
  margin-bottom: 16px;
}

.watermark-shortcuts {
  display: flex;
  flex-direction: column;
  gap: 6px;
  align-items: center;
}

.shortcut-hint {
  font-size: 12px;
  color: var(--text-secondary, #858585);
  line-height: 1.6;
}

.shortcut-hint kbd {
  display: inline-block;
  padding: 1px 5px;
  margin: 0 2px;
  background: var(--bg-tertiary, #2d2d30);
  border: 1px solid var(--border-color, #3e3e42);
  border-radius: 3px;
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--text-secondary, #858585);
}

.welcome-recent {
  margin-top: 24px;
  pointer-events: auto;
}

.recent-title {
  font-size: 12px;
  font-weight: 500;
  color: var(--text-secondary, #858585);
  margin-bottom: 8px;
  text-transform: uppercase;
  letter-spacing: 0.5px;
}

.recent-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.recent-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 12px;
  border-radius: 4px;
  cursor: pointer;
  background: var(--bg-secondary, #2b2d30);
  border: 1px solid var(--border-color, #3e3e42);
  transition:
    background 0.15s,
    border-color 0.15s;
}

.recent-item:hover {
  background: var(--bg-tertiary, #3d4446);
  border-color: var(--brand-accent, #e17055);
}

.recent-icon {
  color: var(--brand-accent, #e17055);
  flex-shrink: 0;
}

.recent-name {
  font-size: 13px;
  font-weight: 500;
  color: var(--text-primary, #cccccc);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.recent-type {
  font-size: 11px;
  color: var(--text-muted, #6b7280);
  text-transform: uppercase;
  margin-left: auto;
}
</style>
