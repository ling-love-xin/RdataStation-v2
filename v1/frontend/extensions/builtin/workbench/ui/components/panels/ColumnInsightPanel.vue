<template>
  <div class="col-insight-root">
    <div
      v-if="!insightStore.insightData && !insightStore.isLoading && !insightStore.error"
      class="insight-empty"
    >
      <Database :size="28" stroke-width="1.5" />
      <p>{{ t('resultPanel.selectColumnInsight') }}</p>
    </div>

    <div v-if="insightStore.isLoading" class="insight-loading">
      <div class="skeleton skeleton-title" :style="{ width: skeletonWidth() + '%' }"></div>
      <div class="skeleton skeleton-row" :style="{ width: skeletonWidth() + 15 + '%' }"></div>
      <div class="skeleton skeleton-row" :style="{ width: skeletonWidth() - 20 + '%' }"></div>
      <div class="skeleton skeleton-row" :style="{ width: skeletonWidth() + '%' }"></div>
      <div class="skeleton skeleton-block" :style="{ width: skeletonWidth() + '%' }"></div>
    </div>

    <div v-if="insightStore.error && !insightStore.insightData" class="insight-error">
      <p>{{ insightStore.error }}</p>
      <NButton size="small" @click="retry()">{{ t('resultPanel.retry') }}</NButton>
    </div>

    <template v-if="insightStore.insightData">
      <NTabs v-model:value="activeTab" type="segment" size="small" animated>
        <NTabPane name="column" :tab="t('resultPanel.columnInsight')">
          <div class="panel-header">
            <span class="panel-title mono">{{ insightStore.currentColumn }}</span>
            <div class="panel-actions">
              <NButton size="tiny" quaternary :loading="insightStore.isLoading" @click="retry()">
                <template #icon><RefreshCw :size="13" /></template>
              </NButton>
              <NButton size="tiny" quaternary @click="exportJSON()">
                <template #icon><Download :size="13" /></template>
              </NButton>
            </div>
          </div>

          <QualityScoreCard :quality-score="insightStore.qualityScore" />

          <InsightStatsSection
            :insight-data="insightStore.insightData"
            :stats-kind="statsKind"
            :total-count-display="totalCountDisplay"
            :null-rate-display="nullRateDisplay"
            :has-distribution="hasDistribution"
            @filter-by-value="filterByValue"
            @open-visualization="openVisualization"
          />

          <div class="panel-footer">
            <div class="storage-info">
              <span class="storage-key">{{ t('resultPanel.storageSize') }}:</span>
              <span class="storage-val">{{ storageSizeStr }}</span>
              <NButton v-if="storageSize > 0" size="tiny" quaternary @click="handleCleanup()">
                {{ t('resultPanel.cleanup') }}
              </NButton>
            </div>
          </div>

          <div v-if="applicableRules.length > 0" class="rules-footer">
            <span class="rules-tag-label">{{ t('resultPanel.applicableRules') }}:</span>
            <NTag v-for="r in applicableRules" :key="r.id" size="tiny" :bordered="false">
              {{ r.name }}
            </NTag>
            <NButton
              size="tiny"
              quaternary
              :loading="insightStore.isReloadingRules"
              :title="t('resultPanel.reloadRules')"
              @click="reloadRules()"
            >
              <template #icon><RefreshCw :size="12" /></template>
            </NButton>
          </div>
        </NTabPane>

        <NTabPane name="multi" :tab="t('resultPanel.multiColumn')">
          <MultiColumnView
            :temp-table="insightStore.currentTempTable ?? ''"
            :all-columns="availableColumns"
          />
        </NTabPane>

        <NTabPane name="history" :tab="t('resultPanel.history')">
          <InsightHistoryTab :is-loading="insightStore.isLoading" />
        </NTabPane>
      </NTabs>
    </template>
  </div>
</template>

<script setup lang="ts">
import { Database, RefreshCw, Download } from 'lucide-vue-next'
import { NTabs, NTabPane, NButton, NTag } from 'naive-ui'
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import { useProjectStore } from '@/core/project/stores/project'

import InsightHistoryTab from './insight/InsightHistoryTab.vue'
import InsightStatsSection from './insight/InsightStatsSection.vue'
import QualityScoreCard from './insight/QualityScoreCard.vue'
import MultiColumnView from './MultiColumnView.vue'
import { useInsightStore } from '../../stores/insight-store'

import type {
  BooleanStatsDetail,
  TextStatsDetail,
  DateTimeStatsDetail,
} from '../../services/result-analysis'

const { t } = useI18n()

const insightStore = useInsightStore()
const projectStore = useProjectStore()
const activeTab = ref('column')

const availableColumns = computed(() => {
  if (!insightStore.insightData) return []
  return [insightStore.currentColumn ?? 'unknown']
})

const statsKind = computed(() => {
  const detail = insightStore.insightData?.stats.stats_detail
  if (!detail) return 'Unknown'
  if ('Numeric' in detail) return 'Numeric'
  if ('Boolean' in detail) return 'Boolean'
  if ('Text' in detail) return 'Text'
  if ('DateTime' in detail) return 'DateTime'
  return 'Unknown'
})

const totalCountDisplay = computed(() => {
  const c = insightStore.insightData?.stats.total_count
  return c != null ? c.toLocaleString() : '—'
})

const nullRateDisplay = computed(() => {
  const r = insightStore.insightData?.stats.null_rate
  return r != null ? (r * 100).toFixed(2) + '%' : '—'
})

const hasDistribution = computed(() => {
  const d = insightStore.insightData
  if (!d) return false
  if (d.histogram && d.histogram.length > 0) return true
  if (statsKind.value === 'Text') {
    const td = (d.stats.stats_detail as TextStatsDetail).top_values
    return td && td.length > 0
  }
  if (statsKind.value === 'DateTime') {
    const td = (d.stats.stats_detail as DateTimeStatsDetail).monthly_distribution
    return td && td.length > 0
  }
  if (statsKind.value === 'Boolean') return true
  return false
})

const applicableRules = computed(() => {
  return insightStore.multiColumnRules.filter(r => {
    if (r.applies_to && r.applies_to.length > 0) {
      return r.applies_to.includes(statsKind.value.toLowerCase())
    }
    return true
  })
})

const storageSize = computed(() => insightStore.history.length * 2)
const storageSizeStr = computed(() => `${storageSize.value} KB`)

function openVisualization() {
  if (!insightStore.insightData) return
  const data = insightStore.insightData
  const kind = statsKind.value

  const extractors: Record<string, () => { columns: string[]; rows: Record<string, unknown>[] }> = {
    bar: () => {
      if (data.histogram) {
        return {
          columns: ['label', 'count', 'ratio'],
          rows: data.histogram.map(b => ({
            label: b.label,
            count: b.count,
            ratio: b.ratio,
          })),
        }
      }
      if (kind === 'Boolean') {
        const bd = data.stats.stats_detail as BooleanStatsDetail
        return {
          columns: ['value', 'count'],
          rows: [
            { value: 'True', count: bd.true_count },
            { value: 'False', count: bd.false_count },
          ],
        }
      }
      const td = data.stats.stats_detail as TextStatsDetail
      if (td.top_values) {
        return {
          columns: ['value', 'count', 'ratio'],
          rows: td.top_values.map(tv => ({
            value: tv.value,
            count: tv.count,
            ratio: tv.ratio,
          })),
        }
      }
      return { columns: [], rows: [] }
    },
    pie: () => {
      if (kind === 'Boolean') {
        const bd = data.stats.stats_detail as BooleanStatsDetail
        return {
          columns: ['category', 'count'],
          rows: [
            { category: 'True', count: bd.true_count },
            { category: 'False', count: bd.false_count },
          ],
        }
      }
      if (data.histogram) {
        return {
          columns: ['category', 'count', 'ratio'],
          rows: data.histogram.map(b => ({
            category: b.label,
            count: b.count,
            ratio: b.ratio,
          })),
        }
      }
      return { columns: [], rows: [] }
    },
  }

  const result = extractors.bar()
  if (result.columns.length === 0) return

  const renderHint = applicableRules.value.find(r => r.render?.component)?.render

  insightStore.pendingVisualizationRequest = {
    title: `${insightStore.currentColumn} ${t('resultPanel.insightChart')}`,
    columns: result.columns,
    data: result.rows,
    chartType: renderHint?.component ?? undefined,
  }
}

function filterByValue(val: string) {
  if (!insightStore.currentTempTable || !insightStore.currentColumn) return
  insightStore.loadColumnInsight(insightStore.currentTempTable, val)
}

function reloadRules() {
  const projectPath = projectStore.projectPath
  if (projectPath) {
    insightStore.reloadRules(projectPath)
  }
}

function retry() {
  if (insightStore.currentTempTable && insightStore.currentColumn) {
    insightStore.loadColumnInsight(insightStore.currentTempTable, insightStore.currentColumn)
  }
}

function skeletonWidth(): number {
  return 50 + Math.random() * 30
}

function handleCleanup() {
  const projectPath = projectStore.projectPath
  const col = insightStore.currentColumn
  const tt = insightStore.currentTempTable
  if (projectPath && col && tt) {
    insightStore.cleanupInsightSnapshots(30)
  }
}

function exportJSON() {
  if (!insightStore.insightData) return
  const blob = new Blob([JSON.stringify(insightStore.insightData, null, 2)], {
    type: 'application/json',
  })
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = `${insightStore.currentColumn ?? 'column'}_insight.json`
  a.click()
  URL.revokeObjectURL(url)
}

watch(
  () => insightStore.insightData,
  () => {
    if (insightStore.insightData && (activeTab.value === 'column' || !insightStore.insightData)) {
      activeTab.value = 'column'
    }
  }
)
</script>

<style scoped>
.col-insight-root {
  height: 100%;
  display: flex;
  flex-direction: column;
  overflow: auto;
}
.insight-empty,
.insight-error {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: var(--spacing-xl) var(--spacing-xl);
  gap: var(--spacing-sm);
  color: var(--text-tertiary);
  font-size: var(--font-size-md);
  text-align: center;
}
.insight-loading {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-sm);
  padding: var(--spacing-lg) var(--spacing-md);
}
.skeleton {
  height: 14px;
  background: var(--bg-elevated);
  border-radius: var(--border-radius-sm);
  opacity: 0.6;
  animation: skel-pulse 1.5s ease-in-out infinite;
}
.skeleton-title {
  height: 18px;
}
.skeleton-block {
  height: 40px;
}
@keyframes skel-pulse {
  0%,
  100% {
    opacity: 0.4;
  }
  50% {
    opacity: 0.8;
  }
}

.panel-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 6px 0;
}
.panel-title {
  font-size: var(--font-size-sm);
  font-weight: 500;
}
.panel-actions {
  display: flex;
  gap: 2px;
}

.panel-footer {
  margin-top: var(--spacing-sm);
  padding-top: 6px;
  border-top: 1px solid var(--border-color);
}
.storage-info {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: var(--font-size-xss);
}
.storage-key {
  color: var(--text-tertiary);
}
.storage-val {
  font-family: var(--font-mono);
}

.rules-footer {
  margin-top: var(--spacing-sm);
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--spacing-xs);
}
.rules-tag-label {
  font-size: var(--font-size-xss);
  color: var(--text-tertiary);
}
</style>
