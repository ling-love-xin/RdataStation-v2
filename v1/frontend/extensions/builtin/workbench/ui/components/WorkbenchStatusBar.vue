<template>
  <div v-if="statusBarSettings.visible" class="status-bar">
    <div class="status-left">
      <!-- 连接状态 -->
      <span
        v-if="statusBarSettings.showConnectionStatus"
        class="status-item connection-status"
        :class="connectionStatusClass"
      >
        <span class="status-dot" :class="connectionStatusClass" />
        <span class="connection-label">{{ connectionLabel }}</span>
      </span>

      <!-- DuckDB 加速指示器 -->
      <span v-if="statusBarSettings.showDuckDBIndicator" class="status-item">
        <span class="status-dot builtin" />
        {{ t('workbench.duckdbAccelerated') }}
      </span>

      <!-- 执行时间 -->
      <span v-if="statusBarSettings.showExecutionTime && executionTime > 0" class="status-item">
        {{ t('workbench.duration') }}: {{ executionTime }}ms
      </span>

      <!-- 行数 -->
      <span v-if="statusBarSettings.showRowCount && rowCount !== undefined" class="status-item">
        {{ t('workbench.rowCount') }}: {{ rowCount }}
      </span>

      <CacheWarmingStatus />
    </div>

    <div class="status-right">
      <!-- 编码格式 -->
      <span v-if="statusBarSettings.showEncoding" class="status-item">{{
        t('workbench.encoding')
      }}</span>

      <!-- 设置按钮 -->
      <span class="status-item clickable" :title="t('settings.title')" @click="handleOpenSettings">
        <Settings :size="14" />
      </span>

      <!-- 版本信息 -->
      <span v-if="statusBarSettings.showVersion" class="status-item version-info">
        RdataStation • {{ t('workbench.wasmPluginVersion') }}
      </span>
    </div>
  </div>
</template>

<script setup lang="ts">
import { Settings } from 'lucide-vue-next'
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'

import { useConnectionStore } from '@/extensions/builtin/connection/ui/stores/connection-store'
import CacheWarmingStatus from '@/extensions/builtin/database/ui/components/cache-warming-status.vue'
import { useAppStore } from '@/stores/useAppStore'

import { WorkbenchEvent, dispatchWorkbenchEvent } from '../constants/workbench-events'

const { t } = useI18n()
const appStore = useAppStore()
const connectionStore = useConnectionStore()

interface Props {
  executionTime?: number
  rowCount?: number
}

withDefaults(defineProps<Props>(), {
  executionTime: 0,
  rowCount: undefined,
})

// 从配置系统读取状态栏设置
const statusBarSettings = computed(() => appStore.effectiveStatusBarSettings)

// 连接状态计算
const connectionStatusClass = computed(() => {
  if (connectionStore.isConnected) return 'connected'
  return 'disconnected'
})

const connectionLabel = computed(() => {
  if (connectionStore.isConnected && connectionStore.currentConnection) {
    return connectionStore.currentConnection.name
  }
  return t('workbench.noConnection')
})

function handleOpenSettings() {
  dispatchWorkbenchEvent(WorkbenchEvent.OpenSettings)
}
</script>

<style scoped>
.status-bar {
  height: 24px;
  background: var(--status-bar-bg, var(--bg-secondary));
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 var(--spacing-md);
  font-size: var(--font-size-sm);
  color: var(--status-bar-text, var(--text-secondary));
  flex-shrink: 0;
  border-top: 1px solid var(--border-color);
}

.status-left,
.status-right {
  display: flex;
  align-items: center;
  gap: var(--spacing-lg);
}

.status-item {
  display: flex;
  align-items: center;
  gap: 6px;
}

.status-item.clickable {
  cursor: pointer;
  opacity: 0.8;
  transition: opacity 0.15s;
}

.status-item.clickable:hover {
  opacity: 1;
  color: var(--text-primary);
}

/* 连接状态样式 */
.connection-status {
  padding: 2px 8px;
  border-radius: var(--border-radius-sm);
  font-weight: 500;
}

.connection-status.connected {
  background: var(--status-bar-connected-bg, rgba(0, 184, 148, 0.15));
  color: var(--status-bar-connected-text, var(--brand-success));
}

.connection-status.disconnected {
  background: var(--status-bar-disconnected-bg, rgba(107, 114, 128, 0.15));
  color: var(--status-bar-disconnected-text, var(--text-muted));
}

.status-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  flex-shrink: 0;
}

.status-dot.connected {
  background: var(--brand-success);
}

.status-dot.disconnected {
  background: var(--text-muted);
}

.status-dot.builtin {
  background: var(--warning-color, var(--brand-warning));
}

.version-info {
  opacity: 0.7;
}
</style>
