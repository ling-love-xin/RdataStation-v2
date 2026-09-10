<template>
  <div class="cap-tab">
    <NAlert type="info" :bordered="false" class="cap-banner">
      {{ $t('connection.capabilitiesTab.desc') }}
    </NAlert>

    <div v-if="!driver" class="empty-hint">{{ $t('navigator.noDriver') }}</div>

    <div v-else class="cap-table">
      <div class="cap-thead">
        <span class="cap-th cap-name">{{ $t('navigator.capability') }}</span>
        <span class="cap-th cap-status">{{ $t('navigator.status') }}</span>
        <span class="cap-th cap-desc-col">{{ $t('navigator.description') }}</span>
      </div>
      <div v-for="cap in capList" :key="cap.key" class="cap-row">
        <span class="cap-td cap-name">{{ cap.label }}</span>
        <span class="cap-td cap-status">
          <span :class="['cap-badge', cap.has ? 'yes' : 'no']">
            {{
              cap.has
                ? '✓ ' + $t('connection.capabilitiesTab.supported')
                : '✗ ' + $t('connection.capabilitiesTab.unsupported')
            }}
          </span>
        </span>
        <span class="cap-td cap-desc-col">{{ cap.desc }}</span>
      </div>
      <div v-if="capList.length === 0" class="cap-row-empty">
        {{ $t('navigator.noCapabilities') }}
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { NAlert } from 'naive-ui'
import { computed, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import type { Driver } from '../../../domain/types'

const { t } = useI18n()

interface Props {
  driver: Driver | null
}
const props = withDefaults(defineProps<Props>(), { driver: null })

const emit = defineEmits<{
  (e: 'extraConfig', config: Record<string, unknown>): void
}>()

interface CapItem {
  key: string
  label: string
  has: boolean
  desc: string
}

// 所有后端驱动 capabilities 的元数据定义（与 drivers 表 seed data 中 capabilities JSON 严格同步）
// 新增能力只需在此添加一条 + 对应 i18n key
const CAP_META: Record<string, { label: string; desc: string }> = {
  tree: {
    label: t('connection.capabilitiesTab.tree'),
    desc: t('connection.capabilitiesTab.treeDesc'),
  },
  health_check: {
    label: t('connection.capabilitiesTab.healthCheck'),
    desc: t('connection.capabilitiesTab.healthCheckDesc'),
  },
  transactions: {
    label: t('connection.capabilitiesTab.transactions'),
    desc: t('connection.capabilitiesTab.transactionsDesc'),
  },
  index_analysis: {
    label: t('connection.capabilitiesTab.indexAnalysis'),
    desc: t('connection.capabilitiesTab.indexAnalysisDesc'),
  },
  sql_autocomplete: {
    label: t('connection.capabilitiesTab.sqlAutocomplete'),
    desc: t('connection.capabilitiesTab.sqlAutocompleteDesc'),
  },
  table_editor: {
    label: t('connection.capabilitiesTab.tableEditor'),
    desc: t('connection.capabilitiesTab.tableEditorDesc'),
  },
  schema_browser: {
    label: t('connection.capabilitiesTab.schemaBrowser'),
    desc: t('connection.capabilitiesTab.schemaBrowserDesc'),
  },
  analytics: {
    label: t('connection.capabilitiesTab.analytics'),
    desc: t('connection.capabilitiesTab.analyticsDesc'),
  },
  federation: {
    label: t('connection.capabilitiesTab.federation'),
    desc: t('connection.capabilitiesTab.federationDesc'),
  },
}

const parsedCaps = computed<string[]>(() => {
  if (!props.driver?.capabilities) return []
  try {
    const arr = JSON.parse(props.driver.capabilities)
    return Array.isArray(arr) ? arr.filter((c): c is string => typeof c === 'string') : []
  } catch (err) {
    console.warn('[parseCapabilities] 解析失败:', err)
    return []
  }
})

const capList = computed<CapItem[]>(() => {
  const caps = parsedCaps.value
  const knownKeys = caps.filter(c => c in CAP_META)
  if (knownKeys.length > 0) {
    return knownKeys.map(c => ({
      key: c,
      label: CAP_META[c]?.label || c,
      has: true,
      desc: CAP_META[c]?.desc || '—',
    }))
  }
  const set = new Set(caps)
  return Object.entries(CAP_META).map(([key, meta]) => ({
    key,
    label: meta.label,
    has: set.has(key),
    desc: meta.desc,
  }))
})

// 将解析后的能力标志包装为 driver_supports_xxx 格式发出
watch(
  parsedCaps,
  caps => {
    if (!props.driver) return
    const driverCaps: Record<string, unknown> = {}
    for (const cap of caps) {
      driverCaps[`driver_supports_${cap}`] = true
    }
    emit('extraConfig', driverCaps)
  },
  { immediate: true }
)
</script>

<style scoped>
.cap-tab {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 4px 0;
}
.cap-banner {
  border-radius: 6px;
}
.empty-hint {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 120px;
  font-size: 13px;
  color: var(--color-text-muted);
}
.cap-table {
  border: 1px solid var(--color-border-subtle);
  border-radius: 8px;
  overflow: hidden;
  font-size: 12px;
}
.cap-thead {
  display: flex;
  background: var(--color-bg-elevated);
  padding: 10px 14px;
  border-bottom: 1px solid var(--color-border-subtle);
}
.cap-th {
  font-weight: 600;
  color: var(--color-text-muted);
}
.cap-row {
  display: flex;
  padding: 8px 14px;
  border-bottom: 1px solid var(--color-border-subtle);
}
.cap-row:last-child {
  border-bottom: none;
}
.cap-row-empty {
  padding: 16px;
  text-align: center;
  color: var(--color-text-muted);
}
.cap-td {
  display: flex;
  align-items: center;
}
.cap-name {
  width: 35%;
  font-weight: 500;
  color: var(--color-text-secondary);
}
.cap-status {
  width: 20%;
}
.cap-desc-col {
  flex: 1;
  color: var(--color-text-muted);
  font-size: 11px;
}
.cap-badge {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  padding: 2px 8px;
  border-radius: 10px;
  font-size: 11px;
}
.cap-badge.yes {
  background: rgba(166, 227, 161, 0.08);
  color: var(--brand-success);
}
.cap-badge.no {
  background: rgba(243, 139, 168, 0.08);
  color: var(--brand-danger);
}
</style>
