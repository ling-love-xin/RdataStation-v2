<template>
  <div class="multi-column-view">
    <div class="selector-section">
      <div class="selector-row">
        <span class="label">{{ t('workbench.selectColumns') }}</span>
        <NSelect
          v-model:value="selectedColumns"
          :options="columnOptions"
          multiple
          :placeholder="t('workbench.selectColumnsHint')"
          size="small"
          class="col-selector"
        />
      </div>
      <div v-if="insightStore.multiColumnRules.length > 0" class="selector-row">
        <span class="label">{{ t('workbench.analysisRules') }}</span>
        <NSelect
          v-model:value="selectedRuleId"
          :options="ruleOptions"
          :placeholder="t('workbench.selectRule')"
          size="small"
          class="rule-selector"
          filterable
        />
      </div>
    </div>

    <div class="action-row">
      <NButton
        size="small"
        type="primary"
        :disabled="!canExecute"
        :loading="insightStore.isMultiExecuting"
        @click="executeAnalysis"
      >
        <Play :size="14" />
        {{ t('workbench.executeAnalysis') }}
      </NButton>
      <NButton size="small" :disabled="!insightStore.multiResult" @click="clearResult">
        <Eraser :size="14" />
        {{ t('workbench.clearResult') }}
      </NButton>
    </div>

    <NDivider style="margin: 6px 0" />

    <div class="result-section">
      <NSpin :show="insightStore.isMultiExecuting" size="medium">
        <div v-if="!insightStore.multiResult" class="empty-state">
          <BarChart3 :size="28" class="empty-icon" />
          <span>{{ t('workbench.selectColumnsAndRule') }}</span>
        </div>

        <div v-else>
          <div class="result-header">
            <NTag type="info" size="small">{{ selectedRuleName }}</NTag>
            <NTag size="small">{{ resultType }}</NTag>
          </div>

          <template v-if="isSingleResult">
            <div v-for="(value, key) in insightStore.multiResult" :key="key" class="kv-row">
              <span class="kv-key">{{ formatKey(key as string) }}</span>
              <span class="kv-value">{{ formatValue(value) }}</span>
            </div>
          </template>

          <template v-else-if="isListResult && Array.isArray(insightStore.multiResult)">
            <div class="list-table-wrapper">
              <NDataTable
                :columns="listColumns"
                :data="listResultData"
                :max-height="220"
                size="small"
                :bordered="false"
                striped
              />
            </div>
          </template>
        </div>
      </NSpin>
    </div>
  </div>
</template>

<script setup lang="ts">
import { Play, Eraser, BarChart3 } from 'lucide-vue-next'
import { NSelect, NButton, NSpin, NTag, NDivider, NDataTable } from 'naive-ui'
import { ref, computed, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import { useInsightStore } from '../../stores/insight-store'

interface Props {
  tempTable: string
  allColumns: string[]
}

const { t } = useI18n()

const props = defineProps<Props>()

const insightStore = useInsightStore()

const selectedColumns = ref<string[]>([])
const selectedRuleId = ref<string | null>(null)

const columnOptions = computed(() => props.allColumns.map(col => ({ label: col, value: col })))

const ruleOptions = computed(() =>
  insightStore.multiColumnRules.map(r => ({
    label: `${r.name} (${r.category})`,
    value: r.id,
  }))
)

const selectedRuleName = computed(() => {
  const rule = insightStore.multiColumnRules.find(r => r.id === selectedRuleId.value)
  return rule?.name ?? ''
})

const canExecute = computed(
  () =>
    selectedColumns.value.length >= 2 &&
    selectedRuleId.value !== null &&
    !insightStore.isMultiExecuting
)

const isListResult = computed(() => {
  const rule = insightStore.multiColumnRules.find(r => r.id === selectedRuleId.value)
  return rule?.result_type === 'list'
})

const isSingleResult = computed(() => !isListResult.value)

const resultType = computed(() =>
  isListResult.value ? t('workbench.list') : t('workbench.singleValue')
)

const listColumns = computed(() => {
  const data = insightStore.multiResult
  if (!isListResult.value || !Array.isArray(data)) return []
  const first = data[0]
  if (!first || typeof first !== 'object') return []
  return Object.keys(first).map(key => ({
    title: formatKey(key),
    key,
    ellipsis: true,
  }))
})

const listResultData = computed(() => {
  if (!isListResult.value || !Array.isArray(insightStore.multiResult)) return []
  return insightStore.multiResult as Record<string, unknown>[]
})

function formatKey(key: string): string {
  const labels: Record<string, string> = {
    corr: t('workbench.correlation'),
    covar: t('workbench.covariance'),
    correlation: t('workbench.correlation'),
    covariance: t('workbench.covariance'),
    regression_slope: t('workbench.regressionSlope'),
    regression_intercept: t('workbench.regressionIntercept'),
    sample_size: t('workbench.sampleSize'),
    category: t('workbench.category'),
    count: t('resultPanel.countLabel'),
    cnt: t('resultPanel.countLabel'),
    avg: t('resultPanel.meanLabel'),
    avg_val: t('resultPanel.meanLabel'),
    stddev: t('resultPanel.stddevLabel'),
    stddev_val: t('resultPanel.stddevLabel'),
    median: t('resultPanel.medianLabel'),
    median_val: t('resultPanel.medianLabel'),
    min: t('resultPanel.minLabel'),
    min_val: t('resultPanel.minLabel'),
    max: t('resultPanel.maxLabel'),
    max_val: t('resultPanel.maxLabel'),
    row_value: t('workbench.rowValue'),
    col_value: t('workbench.colValue'),
    x: 'X',
    y: 'Y',
  }
  return labels[key] ?? key
}

function formatValue(value: unknown): string {
  if (value === null || value === undefined) return '\u2014'
  if (typeof value === 'number') {
    if (Math.abs(value) < 0.001) return value.toExponential(4)
    return Number(value.toFixed(4)).toString()
  }
  if (typeof value === 'string') return value
  if (typeof value === 'boolean') return value ? 'True' : 'False'
  return String(value)
}

function buildParams(): Record<string, string> {
  const rule = insightStore.multiColumnRules.find(r => r.id === selectedRuleId.value)
  const params: Record<string, string> = { table: props.tempTable }
  const cols = selectedColumns.value

  if (rule?.parameters) {
    for (let i = 0; i < cols.length && i < rule.parameters.length; i++) {
      const pname = rule.parameters[i]
      if (pname !== 'table') {
        params[pname] = cols[i] ?? ''
      }
    }
  }

  return params
}

async function executeAnalysis(): Promise<void> {
  if (!canExecute.value || !selectedRuleId.value) return
  const params = buildParams()
  await insightStore.executeMultiRule(selectedRuleId.value, params)
}

function clearResult(): void {
  insightStore.clearMultiResult()
}

watch(
  () => props.allColumns,
  newCols => {
    if (newCols.length > 0 && selectedColumns.value.length === 0) {
      selectedColumns.value = newCols.slice(0, Math.min(3, newCols.length))
    }
  },
  { immediate: true }
)

watch(
  () => insightStore.multiColumnRules,
  rules => {
    if (rules.length > 0 && !selectedRuleId.value) {
      selectedRuleId.value = rules[0].id
    }
  }
)
</script>

<style scoped>
.multi-column-view {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-sm);
}

.selector-section {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-sm);
}

.selector-row {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
}

.label {
  font-size: var(--font-size-sm);
  color: var(--text-secondary);
  min-width: 48px;
  flex-shrink: 0;
}

.col-selector,
.rule-selector {
  flex: 1;
  min-width: 0;
}

.action-row {
  display: flex;
  gap: 6px;
}

.result-section {
  min-height: 60px;
}

.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: var(--spacing-sm);
  padding: var(--spacing-xl) 0;
  color: var(--text-tertiary);
  font-size: var(--font-size-sm);
}

.empty-icon {
  opacity: 0.3;
}

.result-header {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-bottom: var(--spacing-sm);
}

.kv-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 3px 0;
  border-bottom: 1px solid var(--border-color);
}

.kv-row:last-child {
  border-bottom: none;
}

.kv-key {
  font-size: var(--font-size-sm);
  color: var(--text-secondary);
}

.kv-value {
  font-size: var(--font-size-md);
  font-family: var(--font-mono);
  color: var(--text-primary);
  font-weight: 500;
}

.list-table-wrapper {
  max-height: 240px;
  overflow: auto;
}
</style>
