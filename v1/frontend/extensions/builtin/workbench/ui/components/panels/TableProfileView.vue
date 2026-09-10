<template>
  <div class="table-profile-panel" :class="{ dark: uiStore.isDark }">
    <div v-if="isLoading" class="profile-loading">
      <NSpin size="small" />
      <span class="loading-text">{{ t('resultPanel.probing', { table: tableName }) }}...</span>
    </div>

    <div v-else-if="error" class="profile-error">
      <AlertTriangle :size="20" class="error-icon" />
      <span class="error-text">{{ error }}</span>
      <NButton size="tiny" quaternary @click="retry">{{ t('navigator.retry') }}</NButton>
    </div>

    <div v-else-if="profile" class="profile-content">
      <div class="profile-header">
        <div class="profile-title-row">
          <Table :size="16" class="title-icon" />
          <span class="profile-title">{{ profile.table_name }}</span>
          <NTag :bordered="false" size="tiny" type="info">{{ profile.db_type }}</NTag>
          <NTag v-if="profile.row_count != null" :bordered="false" size="tiny">
            {{ profile.row_count.toLocaleString() }} {{ t('resultPanel.rowCount') }}
          </NTag>
        </div>
        <div class="profile-header-actions">
          <NButton
            size="tiny"
            :type="insightStore.isTableQualityLoading ? undefined : 'primary'"
            :loading="insightStore.isTableQualityLoading"
            @click="evaluateQuality"
          >
            {{
              insightStore.isTableQualityLoading
                ? t('resultPanel.evaluating')
                : t('resultPanel.qualityAssessment')
            }}
          </NButton>
        </div>
      </div>

      <div v-if="insightStore.tableQuality" class="quality-summary-bar">
        <div class="quality-score-big">
          <span
            class="score-number"
            :style="{ color: scoreColor(insightStore.tableQuality.overall_score) }"
          >
            {{ Math.round(insightStore.tableQuality.overall_score) }}
          </span>
          <span
            class="score-label"
            :style="{ color: scoreColor(insightStore.tableQuality.overall_score) }"
          >
            {{ insightStore.tableQuality.level }}
          </span>
          <span class="quality-desc">{{ insightStore.tableQuality.summary }}</span>
        </div>
      </div>

      <div v-if="evalError" class="eval-error">
        <span>{{ evalError }}</span>
      </div>

      <div class="profile-body">
        <NDataTable
          :columns="columnDefs"
          :data="profile.columns"
          :bordered="false"
          :single-line="false"
          size="small"
          :max-height="'calc(100vh - 200px)'"
          virtual-scroll
        />
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { AlertTriangle, Table } from 'lucide-vue-next'
import { NButton, NSpin, NTag, NDataTable } from 'naive-ui'
import { onMounted, ref, watch, h } from 'vue'
import { useI18n } from 'vue-i18n'

import { useUiStore } from '@/shared/stores/ui'

import { getTableProfile } from '../../services/result-analysis'
import { useInsightStore } from '../../stores/insight-store'

import type { TableProfile, TableColumnMeta } from '../../services/result-analysis'
import type { DataTableColumn } from 'naive-ui'

interface Props {
  connId?: string
  dbType?: string
  database?: string
  schema?: string
  table?: string
  autoEvaluate?: boolean
}

const props = defineProps<Props>()

const { t } = useI18n()
const uiStore = useUiStore()
const insightStore = useInsightStore()

const isLoading = ref(false)
const error = ref<string | null>(null)
const profile = ref<TableProfile | null>(null)
const tableName = ref('')
const evalError = ref<string | undefined>(undefined)

const columnDefs: DataTableColumn<TableColumnMeta>[] = [
  {
    title: '#',
    key: 'ordinal_position',
    width: 40,
    render: row => row.ordinal_position,
  },
  {
    title: t('resultPanel.column'),
    key: 'column_name',
    render: row => {
      return h('div', { style: { display: 'flex', alignItems: 'center', gap: '4px' } }, [
        row.is_primary_key
          ? h('span', { style: { color: '#f0a020', fontSize: '10px', fontWeight: '600' } }, 'PK')
          : null,
        h(
          'span',
          {
            style: {
              fontFamily: 'monospace',
              fontSize: '12px',
              cursor: 'pointer',
              color: 'var(--primary-color)',
            },
            onClick: () => emitColumnClick(row),
          },
          row.column_name
        ),
      ])
    },
  },
  {
    title: t('resultPanel.type'),
    key: 'data_type',
    width: 120,
    render: row => {
      return h(
        'span',
        { style: { fontFamily: 'monospace', fontSize: '11px', color: 'var(--text-secondary)' } },
        row.data_type
      )
    },
  },
  {
    title: t('resultPanel.nullable'),
    key: 'is_nullable',
    width: 50,
    align: 'center',
    render: row => {
      return row.is_nullable
        ? h('span', { style: { color: 'var(--text-tertiary)' } }, 'YES')
        : h('span', { style: { color: 'var(--text-primary)', fontWeight: '500' } }, 'NO')
    },
  },
  {
    title: t('resultPanel.qualityScore'),
    key: '_quality',
    width: 84,
    align: 'center',
    render: row => {
      const entry = insightStore.tableQuality?.column_scores?.find(
        e => e.column_name === row.column_name
      )
      if (!entry) {
        return h('span', { style: { fontSize: '10px', color: 'var(--text-tertiary)' } }, '\u2014')
      }
      return h(
        'div',
        { style: { display: 'flex', alignItems: 'center', gap: '4px', justifyContent: 'center' } },
        [
          h(
            'span',
            {
              style: {
                fontWeight: '700',
                fontSize: '13px',
                color: scoreColor(entry.quality_score),
              },
            },
            String(Math.round(entry.quality_score))
          ),
          h(
            'span',
            {
              style: {
                fontSize: '9px',
                color: scoreColor(entry.quality_score),
                opacity: '0.8',
              },
            },
            entry.level
          ),
        ]
      )
    },
  },
]

async function loadProfile(
  connId: string,
  dbType: string,
  database: string,
  schema: string,
  table: string
): Promise<void> {
  if (isLoading.value) return

  isLoading.value = true
  error.value = null
  tableName.value = table

  try {
    const result = await getTableProfile({
      conn_id: connId,
      db_type: dbType,
      database,
      schema,
      table,
    })
    profile.value = result
  } catch (e: unknown) {
    const msg = e instanceof Error ? e.message : String(e)
    error.value = `${t('tableProfile.probingFailed')}: ${msg}`
  } finally {
    isLoading.value = false
  }
}

function retry(): void {
  if (props.connId && props.dbType && props.database && props.schema && props.table) {
    loadProfile(props.connId, props.dbType, props.database, props.schema, props.table)
  }
}

function scoreColor(score: number): string {
  if (score >= 85) return '#1a7a1a'
  if (score >= 70) return '#1a6db5'
  if (score >= 50) return '#b57a1a'
  if (score >= 30) return '#b54a1a'
  return '#b51a1a'
}

async function evaluateQuality(): Promise<void> {
  const { connId, database, schema, table } = props
  if (!connId || !database || !schema || !table) return

  await insightStore.loadTableQuality({
    connId,
    database,
    schema,
    table,
  })
}

function emitColumnClick(col: TableColumnMeta): void {
  const { connId, database, schema, table } = props
  if (!connId || !database || !schema || !table) return
  insightStore.loadColumnFromTable({
    connId,
    database,
    schema,
    table,
    column: col.column_name,
  })
}

watch(
  () => insightStore.tableProfileReloadKey,
  () => {
    if (props.connId && props.dbType && props.database && props.schema && props.table) {
      loadProfile(props.connId, props.dbType, props.database, props.schema, props.table)
    }
  }
)

onMounted(() => {
  if (props.connId && props.dbType && props.database && props.schema && props.table) {
    loadProfile(props.connId, props.dbType, props.database, props.schema, props.table)
  }
  if (props.autoEvaluate) {
    evaluateQuality()
  }
})
</script>

<style scoped>
.table-profile-panel {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--bg-primary);
  overflow: hidden;
}

.profile-loading {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  height: 100%;
  gap: var(--spacing-sm);
  color: var(--text-secondary);
}
.loading-text {
  font-size: var(--font-size-xss);
}

.profile-error {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  height: 100%;
  gap: var(--spacing-sm);
  color: var(--danger-color);
  padding: var(--spacing-2xl) var(--spacing-lg);
  text-align: center;
}
.error-icon {
  opacity: 0.7;
}
.error-text {
  font-size: var(--font-size-xss);
  word-break: break-all;
}

.profile-content {
  display: flex;
  flex-direction: column;
  height: 100%;
}

.profile-header {
  flex-shrink: 0;
  padding: var(--spacing-sm) var(--spacing-md);
  border-bottom: 1px solid var(--border-color);
  background: var(--bg-secondary);
}
.profile-title-row {
  display: flex;
  align-items: center;
  gap: 6px;
}
.title-icon {
  color: var(--primary-color);
  flex-shrink: 0;
}
.profile-title {
  font-size: var(--font-size-lg);
  font-weight: 600;
  color: var(--text-primary);
}
.profile-schema {
  font-size: var(--font-size-xs);
  color: var(--text-tertiary);
  display: block;
  margin-top: 2px;
}
.profile-header-actions {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-top: 6px;
}

.quality-summary-bar {
  flex-shrink: 0;
  padding: 6px var(--spacing-md);
  border-bottom: 1px solid var(--border-color);
  background: var(--bg-tertiary);
}
.quality-overall {
  display: flex;
  align-items: center;
  gap: 6px;
}
.quality-score-big {
  font-size: var(--font-size-huge);
  font-weight: 700;
  line-height: 1;
}
.quality-level-big {
  font-size: var(--font-size-md);
  font-weight: 600;
}
.quality-desc {
  font-size: var(--font-size-xs);
  color: var(--text-secondary);
  flex: 1;
}

.eval-error {
  flex-shrink: 0;
  padding: var(--spacing-xs) var(--spacing-md);
  font-size: var(--font-size-xs);
  color: var(--brand-danger);
  background: var(--brand-danger-bg);
}

.profile-body {
  flex: 1;
  overflow: auto;
}

.table-profile-panel.dark .profile-body :deep(.n-data-table) {
  --n-td-color: var(--bg-primary);
  --n-th-color: var(--bg-secondary);
}
</style>
