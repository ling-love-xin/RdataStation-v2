<template>
  <div class="policy-sections">
    <!-- Security policies (extracted, always visible) -->
    <div class="adv-sec no-gap">
      <SecurityPolicySection
        v-model:readonly="polReadonly"
        v-model:write-confirm="polWriteConfirm"
        v-model:ddl-confirm="polDdlConfirm"
        v-model:autocommit="polAutocommit"
        v-model:drop-policy="polDrop"
        v-model:row-limit="polRowLimit"
        v-model:size-limit="polSizeLimit"
        :title="$t('navigator.advancedSecurity')"
        :summary="securitySummary"
        :readonly-label="$t('navigator.advancedReadOnly')"
        :write-confirm-label="$t('navigator.writeConfirm')"
        :ddl-confirm-label="$t('navigator.advancedDdlConfirm') || 'DDL确认'"
        :drop-op-label="$t('navigator.advancedDropOp') || 'DROP操作'"
        :autocommit-label="$t('navigator.autoCommit') || '自动提交'"
        :row-limit-label="$t('navigator.rowLimit')"
        :size-limit-label="$t('navigator.sizeLimit')"
        :drop-options="dropOpts"
        @override="checkPolicyOverride"
      />
    </div>

    <!-- Collapsible policies -->
    <NCollapse>
      <NCollapseItem name="schema">
        <template #header>
          <span class="coll-title">📋 {{ $t('navigator.advancedSchema') || 'Schema 策略' }}</span>
          <span class="collapse-summary">{{ schemaSummary }}</span>
        </template>
        <div class="policy-grid">
          <div class="policy-row">
            <span class="pol-item"
              >自动加载 <NSwitch v-model:value="schAutoLoad" size="small"
            /></span>
            <span class="pol-item"
              >加载深度
              <NInputNumber
                v-model:value="schLoadDepth"
                size="small"
                :min="1"
                :max="10"
                style="width: 72px"
            /></span>
            <span class="pol-item"
              >显示系统表 <NSwitch v-model:value="schShowSystem" size="small"
            /></span>
          </div>
          <div class="policy-row">
            <span class="pol-item"
              >刷新间隔(秒)
              <NInputNumber
                v-model:value="schRefreshInterval"
                size="small"
                :min="0"
                :max="3600"
                style="width: 72px"
            /></span>
          </div>
        </div>
      </NCollapseItem>

      <NCollapseItem name="perf">
        <template #header>
          <span class="coll-title">⚡ {{ $t('navigator.performance') || '性能策略' }}</span>
          <span class="collapse-summary">{{ perfSummary }}</span>
        </template>
        <div class="policy-grid">
          <div class="policy-row">
            <span class="pol-item"
              >连接池大小
              <NInputNumber
                v-model:value="perfPoolSize"
                size="small"
                :min="1"
                :max="100"
                style="width: 72px"
            /></span>
            <span class="pol-item"
              >查询超时(s)
              <NInputNumber
                v-model:value="advQueryTimeout"
                size="small"
                :min="0"
                :max="3600"
                style="width: 72px"
            /></span>
            <span class="pol-item"
              >连接超时(s)
              <NInputNumber
                v-model:value="advConnectTimeout"
                size="small"
                :min="1"
                :max="300"
                style="width: 72px"
            /></span>
          </div>
          <div class="policy-row">
            <span class="pol-item"
              >心跳间隔(s)
              <NInputNumber
                v-model:value="advHeartbeat"
                size="small"
                :min="10"
                :max="600"
                style="width: 72px"
            /></span>
            <span class="pol-item"
              >最大重连
              <NInputNumber
                v-model:value="advMaxReconnect"
                size="small"
                :min="0"
                :max="20"
                style="width: 72px"
            /></span>
          </div>
        </div>
      </NCollapseItem>

      <NCollapseItem name="audit">
        <template #header>
          <span class="coll-title">📝 {{ $t('navigator.audit') || '审计策略' }}</span>
          <span class="collapse-summary">{{ auditSummary }}</span>
        </template>
        <div class="policy-grid">
          <div class="policy-row">
            <span class="pol-item"
              >SQL 日志 <NSwitch v-model:value="audSqlLog" size="small"
            /></span>
            <span class="pol-item"
              >操作记录 <NSwitch v-model:value="audOperationRecord" size="small"
            /></span>
            <span class="pol-item"
              >敏感表告警 <NSwitch v-model:value="audSensitiveTableAlert" size="small"
            /></span>
          </div>
        </div>
      </NCollapseItem>

      <NCollapseItem name="ui">
        <template #header>
          <span class="coll-title">🎨 {{ $t('navigator.uiPolicy') || 'UI 策略' }}</span>
          <span class="collapse-summary">{{ uiSummary }}</span>
        </template>
        <div class="policy-grid">
          <div class="policy-row">
            <span class="pol-item"
              >顶栏颜色 <input v-model="uiTopBarColor" type="color" class="color-input-sm"
            /></span>
            <span class="pol-item"
              >标签指示符
              <NInput
                v-model:value="uiTabIndicator"
                size="small"
                style="width: 80px"
                placeholder="🔴"
            /></span>
          </div>
          <div class="policy-row">
            <span class="pol-item"
              >SQL 警告横幅 <NSwitch v-model:value="uiSqlWarningBanner" size="small"
            /></span>
            <span class="pol-item"
              >写入按钮样式
              <NSelect
                v-model:value="uiWriteBtnStyle"
                size="small"
                :options="writeBtnStyleOpts"
                style="width: 100px"
              />
            </span>
          </div>
        </div>
      </NCollapseItem>
    </NCollapse>
  </div>
</template>

<script setup lang="ts">
import { NCollapse, NCollapseItem, NSwitch, NInputNumber, NSelect, NInput } from 'naive-ui'
import { ref, computed, watch, toRef } from 'vue'

import { useSecurityPolicies } from '../../../composables/useSecurityPolicies'
import { envDefaultValues, envDefs } from '../../../constants/envDefaults'
import SecurityPolicySection from '../SecurityPolicySection.vue'

import type { SelectOption } from 'naive-ui'

const props = defineProps<{
  /** 当前选中的环境ID（用于持久化策略的 save/load） */
  activeEnvId: string | null
  /** 原始环境 ID（用于应用 env 默认值，如 'env-prod'） */
  defaultsEnvId: string | null
  /** 连接作用域 */
  scope?: { global: boolean; project: boolean }
}>()

const emit = defineEmits<{
  'policy-override-changed': [overridden: boolean]
  'config-change': [config: Record<string, unknown>]
}>()

function emitConfig() {
  emit('config-change', collectFullConfig())
}

// ========== Security policies (composable) ==========
const envIdRef = toRef(props, 'activeEnvId')
const {
  polReadonly,
  polWriteConfirm,
  polDdlConfirm,
  polAutocommit,
  polDrop,
  polRowLimit,
  polSizeLimit,
  dropOpts,
  securitySummary,
  isPolicyOverridden,
  applyEnvDefaults: applySecurityDefaults,
  collectPolicyConfig: collectSecurityConfig,
  applyPolicyConfig: applySecurityConfig,
  checkPolicyOverride,
} = useSecurityPolicies(envIdRef)

// 向父组件报告覆盖状态
watch(isPolicyOverridden, v => emit('policy-override-changed', v))

// ========== Schema policies ==========
const schAutoLoad = ref(true)
const schLoadDepth = ref(3)
const schShowSystem = ref(false)
const schRefreshInterval = ref(0)

const schemaSummary = computed(() => {
  const parts: string[] = []
  if (schAutoLoad.value) parts.push('自动加载')
  else parts.push('手动加载')
  if (schShowSystem.value) parts.push('系统表')
  if (schLoadDepth.value > 0) parts.push(`深度${schLoadDepth.value}`)
  if (schRefreshInterval.value > 0) parts.push(`刷新${schRefreshInterval.value}s`)
  return parts.join('·') || '默认'
})

// ========== Performance policies ==========
const perfPoolSize = ref(10)
const advConnectTimeout = ref(30)
const advQueryTimeout = ref(0)
const advHeartbeat = ref(60)
const advMaxReconnect = ref(3)

const perfSummary = computed(() => {
  const parts: string[] = []
  parts.push(`池${perfPoolSize.value}`)
  if (advQueryTimeout.value > 0) parts.push(`查询${advQueryTimeout.value}s`)
  parts.push(`超时${advConnectTimeout.value}s`)
  parts.push(`重连${advMaxReconnect.value}`)
  return parts.join('·')
})

// ========== Audit policies ==========
const audSqlLog = ref(false)
const audOperationRecord = ref(false)
const audSensitiveTableAlert = ref(false)

const auditSummary = computed(() => {
  const parts: string[] = []
  if (audSqlLog.value) parts.push('SQL日志')
  if (audOperationRecord.value) parts.push('操作记录')
  if (audSensitiveTableAlert.value) parts.push('敏感告警')
  return parts.join('·') || '无审计'
})

// ========== UI policies ==========
const uiTopBarColor = ref('#a6e3a1')
const uiTabIndicator = ref('')
const uiSqlWarningBanner = ref(false)
const uiWriteBtnStyle = ref('default')
const writeBtnStyleOpts: SelectOption[] = [
  { label: '默认', value: 'default' },
  { label: '警告', value: 'warning' },
  { label: '危险', value: 'danger' },
  { label: '虚线', value: 'dashed' },
]

const uiSummary = computed(() => {
  const parts: string[] = []
  parts.push(uiTopBarColor.value)
  if (uiSqlWarningBanner.value) parts.push('警告横幅')
  if (uiWriteBtnStyle.value !== 'default') parts.push(`按钮:${uiWriteBtnStyle.value}`)
  return parts.join('·') || '默认'
})

// ========== Apply env defaults ==========
function applyEnvDefaults(id: string) {
  const d = envDefaultValues[id] || envDefaultValues['env-dev']
  applySecurityDefaults(id)
  advConnectTimeout.value = d.ct as number
  advQueryTimeout.value = d.qt as number
  advHeartbeat.value = d.hb as number
  advMaxReconnect.value = d.mr as number
  const isProd = id === 'env-prod'
  schAutoLoad.value = !isProd
  schShowSystem.value = !isProd
  schLoadDepth.value = isProd ? 1 : 3
  schRefreshInterval.value = isProd ? 60 : 0
  audSqlLog.value = !(id === 'env-dev' || id === 'env-sandbox')
  audOperationRecord.value = isProd || id === 'env-staging'
  audSensitiveTableAlert.value = isProd
  uiSqlWarningBanner.value = isProd || id === 'env-staging'
  uiTopBarColor.value = (envDefs.find(e => e.id === id) || envDefs[0]).color
  uiWriteBtnStyle.value = isProd ? 'danger' : 'default'
}

// ========== Policy persistence ==========
interface BackendEnvPolicy {
  id: string
  environment_id: string
  policy_type: string
  policy_config: string | null
  enabled: boolean
}

function collectPolicyConfig(policyType: string): Record<string, unknown> {
  switch (policyType) {
    case 'security':
      return collectSecurityConfig()
    case 'schema':
      return {
        autoLoad: schAutoLoad.value,
        loadDepth: schLoadDepth.value,
        showSystem: schShowSystem.value,
        refreshInterval: schRefreshInterval.value,
      }
    case 'performance':
      return {
        poolSize: perfPoolSize.value,
        queryTimeout: advQueryTimeout.value,
        connectTimeout: advConnectTimeout.value,
        heartbeat: advHeartbeat.value,
        maxReconnect: advMaxReconnect.value,
      }
    case 'audit':
      return {
        sqlLog: audSqlLog.value,
        operationRecord: audOperationRecord.value,
        sensitiveTableAlert: audSensitiveTableAlert.value,
      }
    case 'ui':
      return {
        topBarColor: uiTopBarColor.value,
        tabIndicator: uiTabIndicator.value,
        sqlWarningBanner: uiSqlWarningBanner.value,
        writeBtnStyle: uiWriteBtnStyle.value,
      }
    default:
      return {}
  }
}

function applyPolicyConfig(policyType: string, config: Record<string, unknown>) {
  switch (policyType) {
    case 'security': {
      applySecurityConfig(config)
      break
    }
    case 'schema': {
      if ('autoLoad' in config) schAutoLoad.value = config.autoLoad as boolean
      if ('loadDepth' in config) schLoadDepth.value = config.loadDepth as number
      if ('showSystem' in config) schShowSystem.value = config.showSystem as boolean
      if ('refreshInterval' in config) schRefreshInterval.value = config.refreshInterval as number
      break
    }
    case 'performance': {
      if ('poolSize' in config) perfPoolSize.value = config.poolSize as number
      if ('queryTimeout' in config) advQueryTimeout.value = config.queryTimeout as number
      if ('connectTimeout' in config) advConnectTimeout.value = config.connectTimeout as number
      if ('heartbeat' in config) advHeartbeat.value = config.heartbeat as number
      if ('maxReconnect' in config) advMaxReconnect.value = config.maxReconnect as number
      break
    }
    case 'audit': {
      if ('sqlLog' in config) audSqlLog.value = config.sqlLog as boolean
      if ('operationRecord' in config) audOperationRecord.value = config.operationRecord as boolean
      if ('sensitiveTableAlert' in config)
        audSensitiveTableAlert.value = config.sensitiveTableAlert as boolean
      break
    }
    case 'ui': {
      if ('topBarColor' in config) uiTopBarColor.value = config.topBarColor as string
      if ('tabIndicator' in config) uiTabIndicator.value = config.tabIndicator as string
      if ('sqlWarningBanner' in config)
        uiSqlWarningBanner.value = config.sqlWarningBanner as boolean
      if ('writeBtnStyle' in config) uiWriteBtnStyle.value = config.writeBtnStyle as string
      break
    }
  }
}

async function loadPoliciesForEnv(envId: string) {
  if (!envId || envId.startsWith('env-')) return
  const { invoke } = await import('@tauri-apps/api/core')
  try {
    let policies: BackendEnvPolicy[]
    if (props.scope?.project) {
      const { useProjectStore } = await import('@/core/project/stores/project')
      const pp = useProjectStore().currentProject?.path
      if (!pp) return
      policies = await invoke<BackendEnvPolicy[]>('project_list_environment_policies', {
        environmentId: envId,
        projectPath: pp,
      })
    } else {
      policies = await invoke<BackendEnvPolicy[]>('list_environment_policies', {
        environmentId: envId,
      })
    }
    for (const p of policies) {
      if (!p.enabled) continue
      const config = p.policy_config ? JSON.parse(p.policy_config) : {}
      applyPolicyConfig(p.policy_type, config)
    }
  } catch (err) {
    console.warn('[PolicySections] 策略/API不可用:', err)
  }
}

async function savePolicyForEnv(envId: string, policyType: string) {
  if (!envId || envId.startsWith('env-')) return
  const { invoke } = await import('@tauri-apps/api/core')
  const config = JSON.stringify(collectPolicyConfig(policyType))
  try {
    if (props.scope?.project) {
      const { useProjectStore } = await import('@/core/project/stores/project')
      const pp = useProjectStore().currentProject?.path
      if (!pp) return
      const policies = await invoke<BackendEnvPolicy[]>('project_list_environment_policies', {
        environmentId: envId,
        projectPath: pp,
      })
      const existing = policies.find(p => p.policy_type === policyType)
      if (existing) {
        await invoke('project_update_environment_policy', {
          id: existing.id,
          policyConfig: config,
          enabled: true,
          projectPath: pp,
        })
      } else {
        await invoke('project_create_environment_policy', {
          environmentId: envId,
          policyType,
          policyConfig: config,
          projectPath: pp,
        })
      }
    } else {
      const policies = await invoke<BackendEnvPolicy[]>('list_environment_policies', {
        environmentId: envId,
      })
      const existing = policies.find(p => p.policy_type === policyType)
      if (existing) {
        await invoke('update_environment_policy', {
          policy: {
            id: existing.id,
            environment_id: envId,
            policy_type: policyType,
            policy_config: config,
            enabled: true,
          },
        })
      } else {
        await invoke('create_environment_policy', {
          policy: {
            id: '',
            environment_id: envId,
            policy_type: policyType,
            policy_config: config,
            enabled: true,
          },
        })
      }
    }
  } catch (err) {
    console.warn('[PolicySections] 静默降级:', err)
  }
}

const policySaveTimers: Record<string, ReturnType<typeof setTimeout>> = {}
function debounceSavePolicy(envId: string, policyType: string) {
  const key = `${envId}:${policyType}`
  if (policySaveTimers[key]) clearTimeout(policySaveTimers[key])
  policySaveTimers[key] = setTimeout(() => savePolicyForEnv(envId, policyType), 800)
}

// ========== Auto-save policies on change (debounced) ==========
// Security
watch(
  [polReadonly, polWriteConfirm, polDdlConfirm, polAutocommit, polDrop, polRowLimit, polSizeLimit],
  () => {
    if (props.activeEnvId) debounceSavePolicy(props.activeEnvId, 'security')
  }
)
// Schema
watch([schAutoLoad, schLoadDepth, schShowSystem, schRefreshInterval], () => {
  if (props.activeEnvId) debounceSavePolicy(props.activeEnvId, 'schema')
})
// Performance
watch([perfPoolSize, advQueryTimeout, advConnectTimeout, advHeartbeat, advMaxReconnect], () => {
  if (props.activeEnvId) debounceSavePolicy(props.activeEnvId, 'performance')
})
// Audit
watch([audSqlLog, audOperationRecord, audSensitiveTableAlert], () => {
  if (props.activeEnvId) debounceSavePolicy(props.activeEnvId, 'audit')
})
// UI
watch([uiTopBarColor, uiTabIndicator, uiSqlWarningBanner, uiWriteBtnStyle], () => {
  if (props.activeEnvId) debounceSavePolicy(props.activeEnvId, 'ui')
})

// ========== React to env change from parent ==========
// Apply defaults when defaultsEnvId changes
watch(
  () => props.defaultsEnvId,
  newId => {
    if (newId) applyEnvDefaults(newId)
    emitConfig()
  }
)
// Load persisted policies when resolved ID (activeEnvId) changes
watch(
  () => props.activeEnvId,
  (newId, oldId) => {
    if (newId && newId !== oldId) {
      loadPoliciesForEnv(newId)
    }
  }
)

// ========== Emit config to parent on any policy change ==========
watch(
  [
    polReadonly,
    polWriteConfirm,
    polDdlConfirm,
    polAutocommit,
    polDrop,
    polRowLimit,
    polSizeLimit,
    schAutoLoad,
    schLoadDepth,
    schShowSystem,
    schRefreshInterval,
    perfPoolSize,
    advQueryTimeout,
    advConnectTimeout,
    advHeartbeat,
    advMaxReconnect,
    audSqlLog,
    audOperationRecord,
    audSensitiveTableAlert,
    uiTopBarColor,
    uiTabIndicator,
    uiSqlWarningBanner,
    uiWriteBtnStyle,
  ],
  () => emitConfig()
)

// ========== Full config collector (for parent) ==========
function collectFullConfig(): Record<string, unknown> {
  return {
    security: {
      readonly: polReadonly.value,
      writeConfirm: polWriteConfirm.value,
      ddlConfirm: polDdlConfirm.value,
      autocommit: polAutocommit.value,
      dropPolicy: polDrop.value,
      rowLimit: polRowLimit.value,
      sizeLimit: polSizeLimit.value,
    },
    schema: {
      autoLoad: schAutoLoad.value,
      loadDepth: schLoadDepth.value,
      showSystem: schShowSystem.value,
      refreshInterval: schRefreshInterval.value,
    },
    performance: {
      poolSize: perfPoolSize.value,
      queryTimeout: advQueryTimeout.value,
      connectTimeout: advConnectTimeout.value,
      heartbeat: advHeartbeat.value,
      maxReconnect: advMaxReconnect.value,
    },
    audit: {
      sqlLog: audSqlLog.value,
      operationRecord: audOperationRecord.value,
      sensitiveTableAlert: audSensitiveTableAlert.value,
    },
    ui: {
      topBarColor: uiTopBarColor.value,
      tabIndicator: uiTabIndicator.value,
      sqlWarningBanner: uiSqlWarningBanner.value,
      writeBtnStyle: uiWriteBtnStyle.value,
    },
    connection: {
      connectTimeout: advConnectTimeout.value,
      queryTimeout: advQueryTimeout.value,
      keepAlive: advHeartbeat.value,
      maxReconnect: advMaxReconnect.value,
    },
  }
}

defineExpose({ collectFullConfig })
</script>

<style scoped>
.policy-sections {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.adv-sec.no-gap {
  gap: 0;
}
.coll-title {
  font-size: 12px;
  font-weight: 600;
  color: var(--color-text);
}
.collapse-summary {
  font-size: 11px;
  color: var(--color-text-muted);
  margin-left: 8px;
}
.policy-grid {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 4px 0;
}
.policy-row {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  align-items: center;
}
.pol-item {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: 12px;
  color: var(--color-text-secondary);
}
.color-input-sm {
  width: 24px;
  height: 24px;
  padding: 0;
  border: 1px solid var(--color-border);
  border-radius: 3px;
  cursor: pointer;
}
</style>
