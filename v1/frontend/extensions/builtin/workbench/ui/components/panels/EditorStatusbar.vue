<template>
  <div :class="['editor-statusbar', `mode-${modeClass}`]">
    <!-- 左侧：连接 + 执行状态 -->
    <div class="status-left">
      <span class="status-item status-connection" :title="connectionStatusText">
        <span :class="['conn-dot', connectionStatus]" />
        <span class="conn-name">{{ connectionDisplayText }}</span>
      </span>
      <span class="status-divider" />
      <span v-if="statementCount > 0" class="status-item status-statements">
        {{ statementCount }} {{ statementCount === 1 ? 'stmt' : 'stmts' }}
      </span>
      <span v-if="executing" class="status-item status-executing">
        <span class="executing-dot" />
        {{ $t('sqlEditor.executing') }}
        <NButton
          v-if="canCancel"
          quaternary
          size="tiny"
          class="cancel-btn"
          @click="$emit('cancel')"
        >
          <X :size="12" />
        </NButton>
      </span>
      <span v-else-if="lastExecutionTime !== null" class="status-item status-time">
        <Clock :size="10" />
        {{ formatTime(lastExecutionTime) }}
      </span>
    </div>

    <!-- 中间：光标位置 + 选中信息 -->
    <div class="status-center">
      <span class="status-item status-location">
        {{ $t('sqlEditor.statusLine') }} {{ cursorPosition }}
      </span>
      <span v-if="selectedTextInfo" class="status-item status-selection">
        {{ selectedTextInfo }}
      </span>
    </div>

    <div class="status-spacer" />

    <!-- 通知 -->
    <div class="status-notifications">
      <span
        v-if="(warningCount ?? 0) > 0"
        class="status-item status-notif warn"
        title="查看警告"
        @click="$emit('showWarnings')"
      >
        <AlertTriangle :size="10" />
        {{ warningCount }}
      </span>
      <span
        v-if="(errorCount ?? 0) > 0"
        class="status-item status-notif error"
        title="查看错误"
        @click="$emit('showErrors')"
      >
        <AlertCircle :size="10" />
        {{ errorCount }}
      </span>
    </div>

    <span class="status-divider" />

    <!-- 右侧：模式 + 文件信息 + 事务 -->
    <div class="status-right">
      <span class="status-item status-mode">{{ editorMode }}</span>
      <span v-if="isDirty" class="status-item status-dirty">● {{ $t('sqlEditor.unsaved') }}</span>
      <span class="status-divider" />
      <span class="status-item clickable" title="选择编码">UTF-8</span>
      <span class="status-divider" />
      <span class="status-item clickable" title="换行符">LF</span>
      <span class="status-divider" />
      <span class="status-item" title="缩进">Spaces: 2</span>
      <span class="status-divider" />
      <span class="status-item clickable git-branch" title="Git 分支">
        <GitBranch :size="10" />
        main
      </span>

      <template v-if="inTransaction">
        <span class="status-divider" />
        <span class="status-item status-transaction">
          <span class="tx-dot" />
          TX
        </span>
        <NButton quaternary size="tiny" class="tx-btn tx-commit" @click="$emit('commit')">
          {{ $t('sqlEditor.commit') }}
        </NButton>
        <NButton quaternary size="tiny" class="tx-btn tx-rollback" @click="$emit('rollback')">
          {{ $t('sqlEditor.rollback') }}
        </NButton>
      </template>

      <span class="status-divider" />
      <NPopselect
        v-if="popselectOptions.length > 0"
        :options="popselectOptions"
        :value="selectedConnection"
        size="small"
        :render-label="renderConnectionLabel"
        trigger="click"
        @update:value="$emit('connectionChange', $event)"
      >
        <NButton quaternary size="tiny" class="status-connection-btn">
          <Database :size="11" />
          {{ connectionDisplayText || $t('sqlEditor.statusNoConnection') }}
        </NButton>
      </NPopselect>
      <slot name="right" />
    </div>
  </div>
</template>

<script setup lang="ts">
import { X, Clock, AlertTriangle, AlertCircle, GitBranch, Database } from 'lucide-vue-next'
import { NButton, NPopselect } from 'naive-ui'
import { computed } from 'vue'

interface Props {
  cursorPosition: string
  selectedTextInfo: string
  editorMode: string
  executing: boolean
  canCancel: boolean
  lastExecutionTime: number | null
  connectionInfoText: string
  connectionStatus?: 'connected' | 'disconnected' | 'connecting'
  popselectOptions: Array<{ label: string; value: string }>
  selectedConnection: string
  inTransaction: boolean
  statementCount: number
  isDirty?: boolean
  warningCount?: number
  errorCount?: number
}

const props = defineProps<Props>()

interface Emits {
  (e: 'connectionChange', connId: string): void
  (e: 'cancel'): void
  (e: 'commit'): void
  (e: 'rollback'): void
  (e: 'showWarnings'): void
  (e: 'showErrors'): void
}

defineEmits<Emits>()

const modeClass = computed(() => {
  const mode = props.editorMode.toLowerCase()
  if (mode.includes('sql')) return 'sql'
  if (mode.includes('analysis')) return 'analysis'
  if (mode.includes('code') || mode.includes('plain text')) return 'code'
  return 'default'
})

const connectionStatus = computed(() => props.connectionStatus || 'disconnected')

const connectionStatusText = computed(() => {
  switch (connectionStatus.value) {
    case 'connected': return '已连接'
    case 'connecting': return '连接中...'
    default: return '未连接'
  }
})

const connectionDisplayText = computed(() => {
  if (!props.connectionInfoText) return '未连接'
  const parts = props.connectionInfoText.split('/')
  return parts.length > 1 ? parts[parts.length - 1].trim() : props.connectionInfoText
})

function formatTime(ms: number): string {
  if (ms < 1000) return `${ms}ms`
  if (ms < 60000) return `${(ms / 1000).toFixed(1)}s`
  return `${(ms / 60000).toFixed(1)}m`
}

function renderConnectionLabel(option: { label: string; value: string }): string {
  return option.label
}
</script>

<style scoped>
.editor-statusbar {
  display: flex;
  align-items: center;
  height: 24px;
  padding: 0 8px;
  background: #252526;
  border-top: 1px solid #3e3e42;
  font-size: 12px;
  color: #858585;
  gap: 0;
  transition: background 0.3s ease;
  user-select: none;
}

/* 模式配色 */
.editor-statusbar.mode-sql { background: #1a2633; }
.editor-statusbar.mode-analysis { background: #1a2633; }
.editor-statusbar.mode-code { background: #1f1a2e; }
.editor-statusbar.mode-default { background: #252526; }

.status-left {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
}

.status-center {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
  margin: 0 12px;
}

.status-spacer {
  flex: 1;
}

.status-notifications {
  display: flex;
  align-items: center;
  gap: 4px;
}

.status-right {
  display: flex;
  align-items: center;
  gap: 2px;
}

.status-item {
  white-space: nowrap;
  display: flex;
  align-items: center;
  gap: 3px;
}

.status-item.clickable {
  cursor: pointer;
  padding: 0 3px;
  border-radius: 2px;
}
.status-item.clickable:hover {
  background: rgba(255, 255, 255, 0.08);
  color: #ccc;
}

.status-divider {
  width: 1px;
  height: 14px;
  background: #3e3e42;
  margin: 0 4px;
}

/* 连接状态 */
.status-connection {
  display: flex;
  align-items: center;
  gap: 5px;
  cursor: pointer;
}
.conn-dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  flex-shrink: 0;
}
.conn-dot.connected { background: #4caf50; }
.conn-dot.connecting { background: #cca700; animation: pulse 1.5s ease-in-out infinite; }
.conn-dot.disconnected { background: #f44747; }
.conn-name {
  max-width: 120px;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* 通知计数 */
.status-notif {
  display: flex;
  align-items: center;
  gap: 3px;
  font-size: 10px;
  cursor: pointer;
  padding: 0 4px;
  border-radius: 2px;
  font-weight: 600;
}
.status-notif:hover { background: rgba(255, 255, 255, 0.08); }
.status-notif.warn { color: #cca700; }
.status-notif.error { color: #f44747; }

/* Git 分支 */
.git-branch {
  font-family: 'Consolas', monospace;
  font-size: 10px;
  color: #858585;
  display: flex;
  align-items: center;
  gap: 3px;
}

.status-executing {
  display: flex;
  align-items: center;
  gap: 4px;
  color: #e17055;
}

.executing-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: #e17055;
  animation: pulse 1.5s ease-in-out infinite;
}

.status-time {
  display: flex;
  align-items: center;
  gap: 3px;
  font-family: 'Consolas', monospace;
  font-size: 10px;
}

@keyframes pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.3; }
}

.status-connection-btn {
  font-size: 12px;
  color: #858585;
  display: flex;
  align-items: center;
  gap: 3px;
}

.status-connection-btn:hover {
  color: #ccc;
}

.status-transaction {
  display: flex;
  align-items: center;
  gap: 4px;
  color: #00b894;
  font-weight: 600;
}

.tx-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: #00b894;
}

.tx-btn {
  font-size: 11px;
  padding: 0 4px;
  height: 18px;
}

.tx-commit { color: #00b894; }
.tx-rollback { color: #e17055; }

.status-dirty {
  color: #f0c040;
  font-weight: 600;
}

.status-mode {
  font-weight: 600;
  color: #ccc;
}

.status-selection {
  color: #6a6a6a;
  font-size: 10px;
}
</style>
