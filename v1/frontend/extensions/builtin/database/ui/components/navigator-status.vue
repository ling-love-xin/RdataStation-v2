<template>
  <div class="navigator-status">
    <span v-if="isInTransaction" class="transaction-indicator">
      <span class="transaction-dot"></span>
      <span class="transaction-text">{{ t('navigator.transactionInProgress') }}</span>
      <span class="transaction-duration">{{ formatDuration(transactionDuration) }}</span>
    </span>
    <span class="status-text">{{ text }}</span>
  </div>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'

const { t } = useI18n()

defineProps<{
  text: string
  isInTransaction?: boolean
  transactionDuration?: number
}>()

function formatDuration(ms: number = 0): string {
  const seconds = Math.floor(ms / 1000)
  const minutes = Math.floor(seconds / 60)
  const remainingSeconds = seconds % 60

  if (minutes > 0) {
    return t('navigator.minutesSeconds', { minutes, seconds: remainingSeconds })
  }
  return t('navigator.seconds', { seconds })
}
</script>

<style scoped>
.navigator-status {
  padding: 4px 8px;
  background: var(--bg-tertiary);
  border-top: 1px solid var(--border-color);
  font-size: 11px;
  color: var(--text-secondary);
}

.status-text {
  white-space: nowrap;
}

.transaction-indicator {
  display: flex;
  align-items: center;
  gap: 4px;
  margin-right: 12px;
  padding: 2px 6px;
  background: var(--success-light);
  border-radius: 4px;
}

.transaction-dot {
  width: 8px;
  height: 8px;
  background: var(--success-color);
  border-radius: 50%;
  animation: pulse 1.5s ease-in-out infinite;
}

.transaction-text {
  color: var(--success-color);
  font-weight: 500;
}

.transaction-duration {
  color: var(--text-secondary);
  font-size: 10px;
}

@keyframes pulse {
  0%,
  100% {
    opacity: 1;
  }
  50% {
    opacity: 0.5;
  }
}
</style>
