<template>
  <div class="output-panel">
    <div class="output-header">
      <h3>{{ t('workbench.output') }}</h3>
    </div>
    <div class="output-content">
      <div v-if="logs.length === 0" class="empty-output">
        <p>{{ t('workbench.noOutput') }}</p>
      </div>
      <div v-for="(log, index) in logs" :key="index" :class="['log-item', log.type]">
        <span class="log-time">{{ log.time }}</span>
        <span class="log-message">{{ log.message }}</span>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()

const logs = ref([
  { type: 'info', time: '10:30:00', message: 'RdataStation started' },
  { type: 'info', time: '10:30:01', message: 'Project config loaded' },
  { type: 'success', time: '10:30:02', message: 'DuckDB acceleration enabled' },
])
</script>

<style scoped>
.output-panel {
  height: 100%;
  display: flex;
  flex-direction: column;
  background: var(--bg-primary, #1e1e1e);
}

.output-header {
  padding: 12px 16px;
  border-bottom: 1px solid var(--border-color, #3e3e42);
  background: var(--bg-secondary, #252526);
  flex-shrink: 0;
}

.output-header h3 {
  margin: 0;
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary, #cccccc);
}

.output-content {
  flex: 1;
  overflow-y: auto;
  padding: 12px 16px;
}

.empty-output {
  color: var(--text-secondary, #858585);
  text-align: center;
  padding: 24px 0;
}

.log-item {
  display: flex;
  gap: 12px;
  padding: 8px 0;
  border-bottom: 1px solid var(--border-color, #3e3e42);
}

.log-time {
  color: var(--text-secondary, #858585);
  font-size: 12px;
  flex-shrink: 0;
  font-family: var(--font-mono);
}

.log-message {
  color: var(--text-primary, #cccccc);
  font-size: 13px;
}

.log-item.success .log-message {
  color: var(--text-success, #4ec9b0);
}
</style>
