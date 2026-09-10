<template>
  <div class="plugins-panel">
    <div class="plugins-header">
      <h3>{{ t('workbench.pluginManagement') }}</h3>
    </div>
    <div class="plugins-content">
      <div class="empty-plugins">
        <p>{{ t('workbench.noPlugins') }}</p>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'

const { t } = useI18n()
</script>

<style scoped>
.plugins-panel {
  height: 100%;
  display: flex;
  flex-direction: column;
  background: var(--bg-primary, #1e1e1e);
}

.plugins-header {
  padding: 12px 16px;
  border-bottom: 1px solid var(--border-color, #3e3e42);
  background: var(--bg-secondary, #252526);
  flex-shrink: 0;
}

.plugins-header h3 {
  margin: 0;
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary, #cccccc);
}

.plugins-content {
  flex: 1;
  overflow-y: auto;
  padding: 24px 16px;
}

.empty-plugins {
  color: var(--text-secondary, #858585);
  text-align: center;
}
</style>
