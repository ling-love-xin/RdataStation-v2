<template>
  <div class="adv-tab">
    <div v-if="!driver" class="empty-hint">{{ $t('navigator.noDriver') }}</div>

    <template v-else>
      <!-- Environment section -->
      <EnvironmentSection
        ref="envSectionRef"
        v-model:environment-id="envId"
        :driver="driver"
        :scope="scope"
        @env-change="onEnvChange"
      />

      <!-- DuckDB acceleration -->
      <div v-if="showDuckDB" class="adv-sec">
        <div class="sec-title" style="color: var(--brand-warning)">
          ⚡ {{ $t('connection.advancedTab.localAccel') }} <span class="sec-sub">DuckDB</span>
        </div>
        <DuckDBAccelSection
          v-model:enabled="duckdbEnabled"
          v-model:sync="duckdbSync"
          v-model:interval="duckdbInterval"
          v-model:memory="duckdbMemory"
          v-model:threads="duckdbThreads"
          :sync-options="syncOpts"
          :title="$t('connection.advancedTab.enableDuckdbAccel')"
          :description="$t('connection.advancedTab.accelDesc', { dbType: driver?.name || 'DB' })"
          :sync-label="$t('connection.advancedTab.syncStrategy')"
          :interval-label="$t('connection.advancedTab.syncInterval')"
          :memory-label="$t('connection.advancedTab.memoryLimit')"
          :threads-label="$t('connection.advancedTab.threads')"
          :driver-id="props.driver?.id ?? ''"
        />
      </div>

      <!-- Policy sections (5-dimension) -->
      <PolicySections
        :active-env-id="envId"
        :defaults-env-id="envDefaultsId"
        :scope="scope"
        @policy-override-changed="onPolicyOverrideChange"
        @config-change="onPolicyConfigChange"
      />

      <!-- Metadata section -->
      <MetadataSection
        v-model:schema-strategy="schemaStrategy"
        v-model:encoding="encoding"
        v-model:schema-name="schemaName"
        v-model:options="options"
        v-model:metadata-path="metadataPath"
        v-model:tags="tags"
        v-model:use-duckdb-fed="useDuckdbFed"
      />
    </template>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import EnvironmentSection from './advanced/EnvironmentSection.vue'
import MetadataSection from './advanced/MetadataSection.vue'
import PolicySections from './advanced/PolicySections.vue'
import DuckDBAccelSection from './DuckDBAccelSection.vue'

import type { Driver } from '../../../domain/types'

const { t } = useI18n()

const props = defineProps<{
  driver?: Driver | null
  scope?: { global: boolean; project: boolean }
}>()

const showDuckDB = computed(() => {
  const driverId = (props.driver?.id || '').toLowerCase()
  return ['mysql', 'postgresql', 'postgres', 'sqlite', 'duckdb'].some(id => driverId.includes(id))
})

const emit = defineEmits<{
  'extra-config': [config: Record<string, unknown>]
}>()

const envId = ref<string | null>(null)
const envDefaultsId = ref<string | null>(null)
function onEnvChange(payload: {
  envId: string
  selectedEnvId: string
  envSnapshotId: string | null
}) {
  envDefaultsId.value = payload.envId
}

// ========== DuckDB ==========
const duckdbEnabled = ref(false)
const duckdbSync = ref('auto')
const duckdbInterval = ref(15)
const duckdbMemory = ref(512)
const duckdbThreads = ref(4)
const syncOpts = [
  { label: t('connection.advancedTab.syncAuto'), value: 'auto' },
  { label: t('connection.advancedTab.syncScheduled'), value: 'scheduled' },
  { label: t('connection.advancedTab.syncManual'), value: 'manual' },
]

// ========== Metadata ==========
const schemaName = ref('')
const options = ref('')
const metadataPath = ref('')
const tags = ref('')
const useDuckdbFed = ref(false)
const schemaStrategy = ref('auto')
const encoding = ref('UTF-8')

const policyConfig = ref<Record<string, unknown>>({})
function onPolicyConfigChange(config: Record<string, unknown>) {
  policyConfig.value = config
}

const envSectionRef = ref<InstanceType<typeof EnvironmentSection> | null>(null)
function onPolicyOverrideChange(overridden: boolean) {
  if (envSectionRef.value) {
    envSectionRef.value.setPolicyOverridden(overridden)
  }
}

// ========== Extra config emit ==========
watch(
  [
    duckdbEnabled,
    duckdbSync,
    duckdbInterval,
    duckdbMemory,
    duckdbThreads,
    schemaStrategy,
    encoding,
    envId,
    schemaName,
    options,
    metadataPath,
    tags,
    useDuckdbFed,
    policyConfig,
  ],
  () => {
    const pc = policyConfig.value
    const opts: Record<string, unknown> = {
      environmentId: envId.value,
      duckdb: {
        enabled: duckdbEnabled.value,
        sync: duckdbSync.value,
        interval: duckdbInterval.value,
        memory: duckdbMemory.value,
        threads: duckdbThreads.value,
      },
    }
    if (pc.security) opts.security = pc.security
    if (pc.schema) opts.schema = pc.schema
    if (pc.performance) opts.performance = pc.performance
    if (pc.audit) opts.audit = pc.audit
    if (pc.ui) opts.ui = pc.ui
    if (pc.connection) opts.connection = pc.connection
    opts.schemaStrategy = schemaStrategy.value
    opts.encoding = encoding.value

    emit('extra-config', {
      environmentId: envId.value,
      advancedOptions: JSON.stringify(opts),
      schemaName: schemaName.value || null,
      options: options.value || null,
      metadataPath: metadataPath.value || null,
      tags: tags.value || null,
      useDuckdbFed: useDuckdbFed.value,
    })
  },
  { deep: true }
)
</script>

<style scoped>
.adv-tab {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 0;
}
.empty-hint {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 120px;
  font-size: 13px;
  color: var(--color-text-muted);
}
.adv-sec {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.sec-title {
  font-size: 11px;
  font-weight: 700;
  text-transform: uppercase;
  color: var(--color-text-muted);
  letter-spacing: 0.5px;
  display: flex;
  align-items: center;
  gap: 6px;
}
.sec-sub {
  font-size: 9px;
  font-weight: 400;
  color: var(--color-text-muted);
}
</style>
