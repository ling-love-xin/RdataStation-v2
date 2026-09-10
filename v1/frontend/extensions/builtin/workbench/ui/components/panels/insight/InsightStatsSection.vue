<template>
  <NCollapse :default-expanded-names="['basic', 'dist', 'quality', 'sample']">
    <NCollapseItem name="basic" :title="t('resultPanel.basicStats')">
      <div class="stat-grid">
        <div class="stat-row">
          <span class="stat-label">{{ t('resultPanel.countLabel') }}</span>
          <span class="stat-value">{{ totalCountDisplay }}</span>
        </div>
        <div class="stat-row">
          <span class="stat-label">{{ t('resultPanel.typeLabel') }}</span>
          <NTag :bordered="false" size="tiny">{{ insightData.stats.data_type }}</NTag>
        </div>
        <div class="stat-row">
          <span class="stat-label">{{ t('resultPanel.nullRateLabel') }}</span>
          <span class="stat-value" :class="{ 'text-warning': insightData.stats.null_rate > 0.05 }">
            {{ nullRateDisplay }}
            <span class="stat-sub"
              >({{
                t('resultPanel.nullRateDetail', { count: insightData.stats.null_count })
              }})</span
            >
          </span>
        </div>
        <div v-if="insightData.stats.unique_count != null" class="stat-row">
          <span class="stat-label">{{ t('resultPanel.uniqueLabel') }}</span>
          <span class="stat-value">{{ insightData.stats.unique_count.toLocaleString() }}</span>
        </div>

        <template v-if="statsKind === 'Numeric'">
          <div class="stat-row">
            <span class="stat-label">{{ t('resultPanel.meanLabel') }}</span>
            <span class="stat-value mono">{{ fmtNum(numericDetail!.avg) }}</span>
          </div>
          <div class="stat-row">
            <span class="stat-label">{{ t('resultPanel.medianLabel') }}</span>
            <span class="stat-value mono">{{ fmtNum(numericDetail!.median) }}</span>
          </div>
          <div class="stat-row">
            <span class="stat-label">{{ t('resultPanel.minLabel') }}</span>
            <span class="stat-value mono">{{ fmtNum(numericDetail!.min) }}</span>
          </div>
          <div class="stat-row">
            <span class="stat-label">{{ t('resultPanel.maxLabel') }}</span>
            <span class="stat-value mono">{{ fmtNum(numericDetail!.max) }}</span>
          </div>
          <div class="stat-row">
            <span class="stat-label">{{ t('resultPanel.percentileLabel') }}</span>
            <span class="stat-value mono"
              >{{ fmtNum(numericDetail!.p25) }} / {{ fmtNum(numericDetail!.p75) }}</span
            >
          </div>
          <div v-if="numericDetail!.stddev != null" class="stat-row">
            <span class="stat-label">{{ t('resultPanel.stddevLabel') }}</span>
            <span class="stat-value mono">{{ fmtNum(numericDetail!.stddev ?? 0) }}</span>
          </div>
          <div v-if="numericDetail!.skewness != null" class="stat-row">
            <span class="stat-label">{{ t('resultPanel.skewnessLabel') }}</span>
            <span class="stat-value mono"
              >{{ fmtNum(numericDetail!.skewness ?? 0) }}
              <span class="stat-sub">{{ skewDesc(numericDetail!.skewness ?? 0) }}</span>
            </span>
          </div>
        </template>

        <template v-if="statsKind === 'Text'">
          <div class="stat-row">
            <span class="stat-label">{{ t('resultPanel.lengthRangeLabel') }}</span>
            <span class="stat-value"
              >{{ textDetail!.min_length }} ~ {{ textDetail!.max_length }}</span
            >
          </div>
        </template>

        <template v-if="statsKind === 'DateTime'">
          <div class="stat-row">
            <span class="stat-label">{{ t('resultPanel.earliestLabel') }}</span>
            <span class="stat-value mono small">{{ dateTimeDetail!.earliest }}</span>
          </div>
          <div class="stat-row">
            <span class="stat-label">{{ t('resultPanel.latestLabel') }}</span>
            <span class="stat-value mono small">{{ dateTimeDetail!.latest }}</span>
          </div>
          <div class="stat-row">
            <span class="stat-label">{{ t('resultPanel.spanLabel') }}</span>
            <span class="stat-value">{{
              t('resultPanel.spanDays', { days: dateTimeDetail!.span_days })
            }}</span>
          </div>
        </template>

        <template v-if="statsKind === 'Boolean'">
          <div class="stat-row">
            <span class="stat-label">{{ t('resultPanel.trueLabel') }}</span>
            <span class="stat-value"
              >{{ booleanDetail!.true_count.toLocaleString() }}
              <span class="stat-sub"
                >({{
                  t('resultPanel.trueRatio', {
                    ratio: (booleanDetail!.true_ratio * 100).toFixed(1),
                  })
                }})</span
              >
            </span>
          </div>
          <div class="stat-row">
            <span class="stat-label">{{ t('resultPanel.falseLabel') }}</span>
            <span class="stat-value">{{ booleanDetail!.false_count.toLocaleString() }}</span>
          </div>
        </template>
      </div>
    </NCollapseItem>

    <NCollapseItem v-if="hasDistribution" name="dist" :title="t('resultPanel.dataDistribution')">
      <template v-if="statsKind === 'Numeric'">
        <div class="histogram">
          <div v-for="bin in insightData.histogram" :key="bin.label" class="histo-row">
            <span class="histo-label">{{ bin.label }}</span>
            <div class="histo-bar-wrap">
              <div class="histo-bar" :style="{ width: (bin.ratio * 100).toFixed(1) + '%' }"></div>
            </div>
            <span class="histo-ratio">{{ (bin.ratio * 100).toFixed(1) }}%</span>
          </div>
        </div>
      </template>

      <template v-if="statsKind === 'Text'">
        <div class="freq-list">
          <button
            v-for="(item, i) in textDetail!.top_values"
            :key="i"
            class="freq-item freq-clickable"
            @click="emit('filterByValue', item.value)"
          >
            <span class="freq-label" :title="item.value">[{{ item.value }}]</span>
            <div class="freq-bar-wrap">
              <div class="freq-bar" :style="{ width: (item.ratio * 100).toFixed(1) + '%' }"></div>
            </div>
            <span class="freq-ratio">{{ (item.ratio * 100).toFixed(1) }}%</span>
          </button>
        </div>
      </template>

      <template v-if="statsKind === 'DateTime'">
        <div class="freq-list">
          <button
            v-for="(item, i) in dateTimeDetail!.monthly_distribution"
            :key="i"
            class="freq-item freq-clickable"
            @click="emit('filterByValue', item.value)"
          >
            <span class="freq-label">{{ item.value }}</span>
            <div class="freq-bar-wrap">
              <div
                class="freq-bar freq-bar-datetime"
                :style="{ width: (item.ratio * 100).toFixed(1) + '%' }"
              ></div>
            </div>
            <span class="freq-ratio">{{ (item.ratio * 100).toFixed(1) }}%</span>
          </button>
        </div>
      </template>

      <template v-if="statsKind === 'Boolean'">
        <div class="bool-dist">
          <div class="histo-row">
            <span class="histo-label">True</span>
            <div class="histo-bar-wrap">
              <div
                class="histo-bar histo-bar-bool"
                :style="{ width: (booleanDetail!.true_ratio * 100).toFixed(1) + '%' }"
              ></div>
            </div>
            <span class="histo-ratio">{{ (booleanDetail!.true_ratio * 100).toFixed(1) }}%</span>
          </div>
        </div>
      </template>

      <div class="viz-action">
        <NButton size="tiny" quaternary @click="emit('openVisualization')">
          <template #icon><BarChart3 :size="14" /></template>
          {{ t('resultPanel.openChart') }}
        </NButton>
      </div>
    </NCollapseItem>

    <NCollapseItem name="quality" :title="t('resultPanel.dataQuality')">
      <div class="quality-list">
        <div v-if="insightData.stats.null_rate === 0" class="quality-item quality-ok">
          ✅ {{ t('resultPanel.nullRateOk') }}
        </div>
        <div v-else-if="insightData.stats.null_rate > 0.1" class="quality-item quality-warn">
          ⚠️ {{ t('resultPanel.highNullRate', { rate: nullRateDisplay }) }}
        </div>

        <template v-if="statsKind === 'Numeric'">
          <div
            v-for="(ext, i) in numericDetail!.is_extreme"
            :key="i"
            class="quality-item quality-warn"
          >
            ⚠️ {{ t('resultPanel.extremeValue', { value: ext.value }) }}
          </div>
          <template v-if="numericDetail!.skewness != null">
            <div
              v-if="Math.abs(numericDetail!.skewness ?? 0) > 1"
              class="quality-item quality-info"
            >
              ℹ️ {{ skewDesc(numericDetail!.skewness ?? 0) }}
            </div>
          </template>
        </template>

        <div
          v-if="statsKind === 'Text' && textDetail!.top_values.length >= 5"
          class="quality-item quality-info"
        >
          ℹ️ {{ t('resultPanel.categoryCount', { count: textDetail!.top_values.length }) }}
        </div>

        <div
          v-if="statsKind === 'Boolean' && booleanDetail!.true_ratio > 0.95"
          class="quality-item quality-info"
        >
          ℹ️
          {{
            t('resultPanel.highlyImbalanced', {
              ratio: (booleanDetail!.true_ratio * 100).toFixed(1),
            })
          }}
        </div>
      </div>
    </NCollapseItem>

    <NCollapseItem name="sample" :title="t('resultPanel.sampleData')">
      <div class="sample-list">
        <div v-for="(val, i) in insightData.sample" :key="i" class="sample-item">
          <span class="sample-idx">{{ i + 1 }}</span>
          <span class="sample-val mono">{{ formatCellValue(val) }}</span>
        </div>
      </div>
    </NCollapseItem>
  </NCollapse>
</template>

<script setup lang="ts">
import { BarChart3 } from 'lucide-vue-next'
import { NCollapse, NCollapseItem, NButton, NTag } from 'naive-ui'
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'

import type {
  ColumnInsightFull,
  NumericStatsDetail,
  TextStatsDetail,
  DateTimeStatsDetail,
  BooleanStatsDetail,
} from '../../../services/result-analysis'

type StatsKind = 'Numeric' | 'Text' | 'DateTime' | 'Boolean' | 'Unknown'

const { t } = useI18n()

const props = defineProps<{
  insightData: ColumnInsightFull
  statsKind: StatsKind
  totalCountDisplay: string
  nullRateDisplay: string
  hasDistribution: boolean
}>()

const emit = defineEmits<{
  filterByValue: [value: string]
  openVisualization: []
}>()

const numericDetail = computed<NumericStatsDetail | null>(() =>
  props.statsKind === 'Numeric'
    ? (props.insightData.stats.stats_detail as NumericStatsDetail)
    : null
)
const textDetail = computed<TextStatsDetail | null>(() =>
  props.statsKind === 'Text' ? (props.insightData.stats.stats_detail as TextStatsDetail) : null
)
const dateTimeDetail = computed<DateTimeStatsDetail | null>(() =>
  props.statsKind === 'DateTime'
    ? (props.insightData.stats.stats_detail as DateTimeStatsDetail)
    : null
)
const booleanDetail = computed<BooleanStatsDetail | null>(() =>
  props.statsKind === 'Boolean'
    ? (props.insightData.stats.stats_detail as BooleanStatsDetail)
    : null
)

function fmtNum(n: number): string {
  if (!isFinite(n)) return 'N/A'
  if (Number.isInteger(n)) return n.toLocaleString()
  return parseFloat(n.toFixed(4)).toString()
}

function skewDesc(s: number): string {
  if (s > 1) return t('resultPanel.rightSkew')
  if (s < -1) return t('resultPanel.leftSkew')
  return t('resultPanel.approxSymmetric')
}

function formatCellValue(val: unknown): string {
  if (val === null || val === undefined) return 'NULL'
  if (typeof val === 'object') {
    try {
      return JSON.stringify(val)
    } catch {
      return String(val)
    }
  }
  const str = String(val)
  if (str.length > 200) return str.substring(0, 200) + '...'
  return str
}
</script>

<style scoped>
.stat-grid {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-xs);
}
.stat-row {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 3px 0;
}
.stat-label {
  color: var(--text-secondary);
  font-size: var(--font-size-xss);
}
.stat-value {
  font-size: var(--font-size-sm);
  font-weight: 500;
}
.stat-value.mono {
  font-family: var(--font-mono);
  font-size: var(--font-size-xss);
}
.stat-value.small {
  font-size: var(--font-size-xs);
}
.stat-sub {
  font-size: var(--font-size-xs);
  color: var(--text-tertiary);
  margin-left: var(--spacing-xs);
}
.text-warning {
  color: var(--brand-warning);
}

.histogram,
.freq-list,
.bool-dist {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-xs);
}
.histo-row,
.freq-item {
  display: flex;
  align-items: center;
  gap: 6px;
}
.histo-label,
.freq-label {
  width: 60px;
  font-size: var(--font-size-xs);
  text-align: right;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--text-tertiary);
}
.histo-bar-wrap,
.freq-bar-wrap {
  flex: 1;
  height: var(--spacing-sm);
  background: var(--bg-secondary);
  border-radius: var(--border-radius-sm);
  overflow: hidden;
}
.histo-bar,
.freq-bar {
  height: 100%;
  background: var(--primary-color);
  border-radius: var(--border-radius-sm);
  transition: width 0.4s ease;
}
.histo-bar-bool {
  background: var(--brand-success);
}
.freq-bar-datetime {
  background: var(--brand-accent);
}
.freq-clickable {
  cursor: pointer;
  background: none;
  border: none;
  padding: 2px 0;
  color: inherit;
  font: inherit;
  width: 100%;
  text-align: left;
}
.freq-clickable:hover {
  background: var(--bg-hover);
  border-radius: var(--border-radius-sm);
}
.histo-ratio,
.freq-ratio {
  font-size: var(--font-size-xs);
  color: var(--text-secondary);
  width: 40px;
  text-align: right;
}

.viz-action {
  margin-top: var(--spacing-sm);
  display: flex;
  justify-content: flex-end;
}

.quality-list {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-xs);
}
.quality-item {
  font-size: var(--font-size-xss);
  padding: var(--spacing-xs) 6px;
  border-radius: var(--border-radius-sm);
}
.quality-ok {
  background: rgba(0, 184, 148, 0.08);
  color: var(--brand-success);
}
.quality-warn {
  background: rgba(253, 203, 110, 0.08);
  color: var(--brand-warning);
}
.quality-info {
  background: rgba(116, 185, 255, 0.08);
  color: var(--brand-blue);
}

.sample-list {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.sample-item {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  padding: 2px var(--spacing-xs);
}
.sample-idx {
  font-size: var(--font-size-xs);
  color: var(--text-tertiary);
  width: 16px;
  text-align: right;
}
.sample-val {
  font-size: var(--font-size-xss);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  flex: 1;
}
</style>
