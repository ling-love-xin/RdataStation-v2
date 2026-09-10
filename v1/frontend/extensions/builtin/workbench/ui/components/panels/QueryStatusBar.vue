<template>
  <div class="query-statusbar">
    <span class="statusbar-item">{{ connectionName || '无连接' }}</span>
    <span class="statusbar-separator">|</span>
    <span class="statusbar-item">{{ cursorPosition }}</span>
    <span class="statusbar-separator">|</span>
    <span class="statusbar-item">{{ statementCount }} 条SQL</span>
    <span v-if="inTransaction" class="statusbar-separator">|</span>
    <span v-if="inTransaction" class="statusbar-item statusbar-transaction">事务中</span>
    <span v-if="lastExecutionTime !== null" class="statusbar-separator">|</span>
    <span v-if="lastExecutionTime !== null" class="statusbar-item">{{ lastExecutionTime }}ms</span>
    <span class="statusbar-spacer" />
    <span class="statusbar-item">{{ editorMode }}</span>
  </div>
</template>

<script setup lang="ts">
interface Props {
  connectionName: string
  cursorPosition: string
  statementCount: number
  inTransaction: boolean
  lastExecutionTime: number | null
  editorMode: string
}

defineProps<Props>()
</script>

<style scoped>
.query-statusbar {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 2px 10px;
  background: var(--statusbar-bg, #007acc);
  color: var(--statusbar-fg, #ffffff);
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

.statusbar-transaction {
  color: #ffd700;
  font-weight: 500;
}
</style>
