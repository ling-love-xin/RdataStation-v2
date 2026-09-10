<template>
  <div class="query-toolbar">
    <div class="toolbar-group">
      <NTooltip trigger="hover">
        <template #trigger>
          <NButton
            size="small"
            quaternary
            class="toolbar-btn"
            :disabled="isExecuting"
            @click="$emit('execute')"
          >
            <template #icon><Play :size="15" /></template>
            执行
          </NButton>
        </template>
        执行 (Ctrl+Enter)
      </NTooltip>

      <NTooltip trigger="hover">
        <template #trigger>
          <NButton
            size="small"
            quaternary
            class="toolbar-btn"
            :disabled="isExecuting"
            @click="$emit('executeNew')"
          >
            <template #icon><ExternalLink :size="15" /></template>
            新标签
          </NButton>
        </template>
        新标签页执行 (Ctrl+Shift+Enter)
      </NTooltip>

      <NTooltip trigger="hover">
        <template #trigger>
          <NButton
            size="small"
            quaternary
            class="toolbar-btn"
            :disabled="isExecuting"
            @click="$emit('accelerate')"
          >
            <template #icon><Zap :size="15" /></template>
            本地加速
          </NButton>
        </template>
        本地 DuckDB 加速执行
      </NTooltip>

      <NTooltip trigger="hover">
        <template #trigger>
          <NButton
            size="small"
            quaternary
            class="toolbar-btn"
            :disabled="isExecuting"
            @click="$emit('explain')"
          >
            <template #icon><FileSearch :size="15" /></template>
            执行计划
          </NButton>
        </template>
        查看执行计划
      </NTooltip>
    </div>

    <div class="toolbar-divider" />

    <div class="toolbar-group">
      <NTooltip trigger="hover">
        <template #trigger>
          <NButton size="small" quaternary class="toolbar-btn" @click="$emit('format')">
            <template #icon><AlignLeft :size="15" /></template>
            格式化
          </NButton>
        </template>
        格式化 SQL
      </NTooltip>

      <NTooltip trigger="hover">
        <template #trigger>
          <NButton size="small" quaternary class="toolbar-btn" @click="showTranspile = true">
            <template #icon><Languages :size="15" /></template>
            方言转换
          </NButton>
        </template>
        方言转换
      </NTooltip>
    </div>

    <div class="toolbar-divider" />

    <div class="toolbar-group">
      <NButtonGroup size="small">
        <NButton
          :type="executionMode === 'normal' ? 'primary' : 'default'"
          @click="$emit('modeChange', 'normal')"
          >普通</NButton
        >
        <NButton
          :type="executionMode === 'analysis' ? 'primary' : 'default'"
          @click="$emit('modeChange', 'analysis')"
          >分析</NButton
        >
        <NButton
          :type="executionMode === 'smart' ? 'primary' : 'default'"
          @click="$emit('modeChange', 'smart')"
          >智能</NButton
        >
      </NButtonGroup>
    </div>

    <div class="toolbar-divider" />

    <div class="toolbar-group">
      <NTooltip trigger="hover">
        <template #trigger>
          <NButton
            size="small"
            quaternary
            class="toolbar-btn"
            :disabled="inTransaction"
            @click="$emit('beginTransaction')"
          >
            <template #icon><PlayCircle :size="15" /></template>
            开始事务
          </NButton>
        </template>
        开始事务
      </NTooltip>

      <NTooltip trigger="hover">
        <template #trigger>
          <NButton
            size="small"
            quaternary
            class="toolbar-btn"
            :disabled="!inTransaction"
            @click="$emit('commitTransaction')"
          >
            <template #icon><CheckCircle :size="15" /></template>
            提交
          </NButton>
        </template>
        提交事务
      </NTooltip>

      <NTooltip trigger="hover">
        <template #trigger>
          <NButton
            size="small"
            quaternary
            class="toolbar-btn"
            :disabled="!inTransaction"
            @click="$emit('rollbackTransaction')"
          >
            <template #icon><XCircle :size="15" /></template>
            回滚
          </NButton>
        </template>
        回滚事务
      </NTooltip>
    </div>

    <div class="toolbar-spacer" />

    <div class="toolbar-group">
      <NTooltip trigger="hover">
        <template #trigger>
          <NButton size="small" quaternary class="toolbar-btn">
            <template #icon><Clock :size="15" /></template>
            历史
          </NButton>
        </template>
        SQL 历史
      </NTooltip>
    </div>

    <NModal v-model:show="showTranspile" title="方言转换" :mask-closable="true">
      <div class="transpile-modal">
        <NSelect
          v-model:value="targetDialect"
          :options="dialectOptions"
          placeholder="选择目标方言"
        />
        <NButton type="primary" size="small" class="transpile-btn" @click="doTranspile"
          >转换</NButton
        >
      </div>
    </NModal>
  </div>
</template>

<script setup lang="ts">
import {
  Play,
  ExternalLink,
  Zap,
  FileSearch,
  AlignLeft,
  Languages,
  PlayCircle,
  CheckCircle,
  XCircle,
  Clock,
} from 'lucide-vue-next'
import { NButton, NButtonGroup, NModal, NSelect, NTooltip } from 'naive-ui'
import { ref } from 'vue'

interface Props {
  isExecuting: boolean
  executionMode: 'normal' | 'analysis' | 'smart'
  connectionName: string
  inTransaction?: boolean
}

withDefaults(defineProps<Props>(), {
  inTransaction: false,
})

interface Emits {
  (e: 'execute'): void
  (e: 'executeNew'): void
  (e: 'accelerate'): void
  (e: 'explain'): void
  (e: 'format'): void
  (e: 'transpile', dialect: string): void
  (e: 'modeChange', mode: 'normal' | 'analysis' | 'smart'): void
  (e: 'beginTransaction'): void
  (e: 'commitTransaction'): void
  (e: 'rollbackTransaction'): void
}

const emit = defineEmits<Emits>()

const showTranspile = ref(false)
const targetDialect = ref('postgresql')

const dialectOptions = [
  { label: 'MySQL', value: 'mysql' },
  { label: 'PostgreSQL', value: 'postgresql' },
  { label: 'SQLite', value: 'sqlite' },
  { label: 'SQL Server', value: 'tsql' },
  { label: 'DuckDB', value: 'duckdb' },
  { label: 'Oracle', value: 'oracle' },
  { label: 'BigQuery', value: 'bigquery' },
]

function doTranspile() {
  showTranspile.value = false
  emit('transpile', targetDialect.value)
}
</script>

<style scoped>
.query-toolbar {
  display: flex;
  align-items: center;
  gap: 2px;
  padding: 4px 8px;
  background: var(--bg-secondary, #252526);
  border-bottom: 1px solid var(--border-color, #3e3e42);
  flex-shrink: 0;
  min-height: 32px;
  overflow-x: auto;
}

.toolbar-group {
  display: flex;
  align-items: center;
  gap: 2px;
}

.toolbar-btn {
  color: var(--text-secondary, #858585);
}

.toolbar-btn:hover {
  color: var(--text-primary, #cccccc);
  background: var(--bg-hover, #2d2d30);
}

.toolbar-divider {
  width: 1px;
  height: 16px;
  background: var(--border-color, #3e3e42);
  margin: 0 6px;
}

.toolbar-spacer {
  flex: 1;
}

.transpile-modal {
  padding: 16px;
  display: flex;
  flex-direction: column;
}

.transpile-btn {
  margin-top: 12px;
}
</style>
