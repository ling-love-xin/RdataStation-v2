<template>
  <NModal :show="show" :mask-closable="false" @update:show="$emit('close')">
    <div class="test-modal">
      <!-- Header -->
      <div :class="['test-header', result.success ? 'success' : 'fail']">
        <CheckCircle v-if="result.success" :size="24" />
        <XCircle v-else :size="24" />
        <span class="test-title">{{
          result.success ? $t('navigator.testSuccess') : $t('navigator.testFailed')
        }}</span>
      </div>

      <!-- Error detail (only on failure) -->
      <div v-if="!result.success" class="test-section">
        <div class="section-title">{{ $t('navigator.errorDetail') }}</div>
        <div class="section-body error-detail">
          {{ result.message || $t('navigator.connectionFailedGeneric') }}
        </div>
      </div>

      <!-- Remote Server -->
      <div class="test-section">
        <div class="section-title">{{ $t('navigator.remoteServer') }}</div>
        <div class="section-body kv-grid">
          <div class="kv-row">
            <span class="kv-key">{{ $t('navigator.hostPort') }}</span>
            <span class="kv-val">{{ hostPort }}</span>
          </div>
          <div v-if="result.serverVersion" class="kv-row">
            <span class="kv-key">{{ $t('navigator.serverVersion') }}</span>
            <span class="kv-val mono">{{ result.serverVersion }}</span>
          </div>
          <div v-if="database" class="kv-row">
            <span class="kv-key">{{ $t('navigator.database') }}</span>
            <span class="kv-val mono">{{ database }}</span>
          </div>
          <div v-if="user" class="kv-row">
            <span class="kv-key">{{ $t('navigator.username') }}</span>
            <span class="kv-val">{{ user }}</span>
          </div>
        </div>
      </div>

      <!-- Local Client -->
      <div class="test-section">
        <div class="section-title">{{ $t('navigator.localClient') }}</div>
        <div class="section-body kv-grid">
          <div class="kv-row">
            <span class="kv-key">{{ $t('navigator.application') }}</span>
            <span class="kv-val">RdataStation</span>
          </div>
          <div v-if="driverName" class="kv-row">
            <span class="kv-key">{{ $t('navigator.driver') }}</span>
            <span class="kv-val">{{ driverName }}</span>
          </div>
          <div v-if="networkInfo" class="kv-row">
            <span class="kv-key">{{ $t('navigator.network') }}</span>
            <span class="kv-val">{{ networkInfo }}</span>
          </div>
        </div>
      </div>

      <!-- Connection Info -->
      <div class="test-section">
        <div class="section-title">{{ $t('navigator.connectionInfo') }}</div>
        <div class="section-body kv-grid">
          <div v-if="url" class="kv-row url-row">
            <span class="kv-key">URL</span>
            <span class="kv-val mono url">{{ url }}</span>
          </div>
          <div v-if="result.responseTimeMs != null" class="kv-row">
            <span class="kv-key">{{ $t('navigator.responseTime') }}</span>
            <span class="kv-val mono">{{ result.responseTimeMs }}ms</span>
          </div>
          <div v-if="result.success" class="kv-row">
            <span class="kv-key">{{ $t('navigator.status') }}</span>
            <span class="kv-val status-ok">● {{ $t('navigator.connected') }}</span>
          </div>
        </div>
      </div>

      <!-- Footer -->
      <div class="test-footer">
        <NButton type="primary" @click="$emit('close')">{{ $t('navigator.confirm') }}</NButton>
      </div>
    </div>
  </NModal>
</template>

<script setup lang="ts">
import { CheckCircle, XCircle } from 'lucide-vue-next'
import { NButton, NModal } from 'naive-ui'
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'

const { t: $t } = useI18n()

interface TestResult {
  success: boolean
  message: string
  serverVersion?: string
  responseTimeMs?: number
}

interface Props {
  show: boolean
  result: TestResult
  host?: string
  port?: string
  database?: string
  user?: string
  url?: string
  driverName?: string
  networkInfo?: string
}

const props = withDefaults(defineProps<Props>(), {
  show: false,
  host: '',
  port: '',
  database: '',
  user: '',
  url: '',
  driverName: '',
  networkInfo: '',
})

defineEmits<{ close: [] }>()

const hostPort = computed(() => {
  if (!props.host) return '-'
  const port = props.port ? `:${props.port}` : ''
  return `${props.host}${port}`
})
</script>

<style scoped>
.test-modal {
  width: 480px;
  max-height: 88vh;
  background: var(--color-bg-primary);
  border: 1px solid var(--color-border);
  border-radius: var(--border-radius-md);
  overflow: hidden;
  display: flex;
  flex-direction: column;
}

/* Header */
.test-header {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 14px 20px;
  font-size: var(--font-size-md);
  font-weight: 600;
}
.test-header.success {
  color: var(--brand-success);
  border-bottom: 1px solid var(--color-border-subtle);
}
.test-header.fail {
  color: var(--brand-danger);
  border-bottom: 1px solid var(--color-border-subtle);
}

/* Sections */
.test-section {
  padding: 12px 20px;
  border-bottom: 1px solid var(--color-border-subtle);
}
.test-section:last-of-type {
  border-bottom: none;
}

.section-title {
  font-size: var(--font-size-xs);
  font-weight: 700;
  text-transform: uppercase;
  color: var(--color-text-muted);
  margin-bottom: 8px;
  letter-spacing: 0.5px;
}

.section-body {
  padding-left: 2px;
}

.error-detail {
  font-size: var(--font-size-sm);
  color: var(--brand-danger);
  line-height: 1.5;
  word-break: break-all;
}

/* KV Grid */
.kv-grid {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.kv-row {
  display: flex;
  align-items: baseline;
  gap: 12px;
  font-size: var(--font-size-sm);
}
.kv-key {
  flex-shrink: 0;
  width: 72px;
  color: var(--color-text-muted);
  text-align: right;
}
.kv-val {
  color: var(--color-text-primary);
  flex: 1;
  min-width: 0;
  word-break: break-all;
}
.kv-val.mono {
  font-family: var(--font-mono);
  font-size: var(--font-size-xs);
}
.kv-val.url {
  font-size: var(--font-size-xs);
  color: var(--color-text-secondary);
}
.kv-val.status-ok {
  color: var(--brand-success);
  font-weight: 500;
}
.url-row {
  align-items: flex-start;
}

/* Footer */
.test-footer {
  display: flex;
  justify-content: flex-end;
  padding: 12px 20px;
  border-top: 1px solid var(--color-border-subtle);
}
</style>
