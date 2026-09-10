<template>
  <div class="distribution-chart">
    <div v-if="!hasData" class="empty-state">
      <span class="empty-text">暂无数据</span>
    </div>

    <template v-for="info in columnInfos" :key="info.name">
      <div class="dist-section">
        <div class="dist-header">
          <span class="dist-col-name">{{ info.name }}</span>
          <span class="dist-col-type">{{ info.dataType }}</span>
        </div>

        <!-- 数值型：直方图 -->
        <template v-if="info.kind === 'numeric'">
          <div class="dist-stats">
            <span class="dist-stat">Min: {{ fmtNum(info.min) }}</span>
            <span class="dist-stat">Max: {{ fmtNum(info.max) }}</span>
            <span class="dist-stat">Avg: {{ fmtNum(info.avg) }}</span>
          </div>
          <div class="histogram">
            <div v-for="bin in info.bins" :key="bin.label" class="histo-row">
              <span class="histo-label">{{ bin.label }}</span>
              <div class="histo-bar-wrap">
                <div class="histo-bar" :style="{ width: (bin.ratio * 100).toFixed(1) + '%' }" />
              </div>
              <span class="histo-count">{{ bin.count }}</span>
            </div>
          </div>
        </template>

        <!-- 文本型：频率列表 -->
        <template v-if="info.kind === 'text'">
          <div class="dist-stats">
            <span class="dist-stat">唯一值: {{ info.uniqueCount }}</span>
            <span class="dist-stat">空值: {{ info.nullCount }}</span>
          </div>
          <div class="freq-list">
            <div v-for="item in info.topValues" :key="item.value" class="freq-row">
              <span class="freq-label">{{ item.value }}</span>
              <div class="freq-bar-wrap">
                <div class="freq-bar" :style="{ width: (item.ratio * 100).toFixed(1) + '%' }" />
              </div>
              <span class="freq-count">{{ item.count }}</span>
            </div>
          </div>
        </template>

        <!-- 布尔型 -->
        <template v-if="info.kind === 'boolean'">
          <div class="dist-stats">
            <span class="dist-stat">True: {{ info.trueCount }}</span>
            <span class="dist-stat">False: {{ info.falseCount }}</span>
          </div>
          <div class="histogram">
            <div class="histo-row">
              <span class="histo-label">True</span>
              <div class="histo-bar-wrap">
                <div
                  class="histo-bar histo-bar-bool"
                  :style="{ width: ((info.trueRatio ?? 0) * 100).toFixed(1) + '%' }"
                />
              </div>
              <span class="histo-count">{{ info.trueCount }}</span>
            </div>
            <div class="histo-row">
              <span class="histo-label">False</span>
              <div class="histo-bar-wrap">
                <div
                  class="histo-bar histo-bar-bool"
                  :style="{ width: ((info.falseRatio ?? 0) * 100).toFixed(1) + '%' }"
                />
              </div>
              <span class="histo-count">{{ info.falseCount }}</span>
            </div>
          </div>
        </template>
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'

/** 列数据：colName -> values[] */
interface Props {
  columns: string[]
  columnTypes: string[]
  rows: unknown[][]
  /** 直方图分箱数，默认 10 */
  binCount?: number
  /** 文本频率 Top N，默认 10 */
  topN?: number
}

const props = withDefaults(defineProps<Props>(), {
  binCount: 10,
  topN: 10,
})

interface ColumnInfo {
  name: string
  dataType: string
  kind: 'numeric' | 'text' | 'boolean' | 'unknown'
  // numeric
  min?: number
  max?: number
  avg?: number
  bins?: { label: string; count: number; ratio: number }[]
  // text
  uniqueCount?: number
  nullCount?: number
  topValues?: { value: string; count: number; ratio: number }[]
  // boolean
  trueCount?: number
  falseCount?: number
  trueRatio?: number
  falseRatio?: number
}

const NUMERIC_TYPES = new Set([
  'integer',
  'bigint',
  'float',
  'double',
  'decimal',
  'int',
  'int4',
  'int8',
  'real',
  'numeric',
])
const TEXT_TYPES = new Set(['varchar', 'text', 'char', 'uuid', 'blob'])
const BOOLEAN_TYPES = new Set(['boolean', 'bool'])

function guessKind(dataType: string): 'numeric' | 'text' | 'boolean' | 'unknown' {
  const dt = dataType.toLowerCase()
  if (NUMERIC_TYPES.has(dt)) return 'numeric'
  if (BOOLEAN_TYPES.has(dt)) return 'boolean'
  if (TEXT_TYPES.has(dt) || dt.includes('varchar') || dt.includes('char')) return 'text'
  return 'unknown'
}

function toNum(v: unknown): number | null {
  if (v == null) return null
  if (typeof v === 'number') return Number.isFinite(v) ? v : null
  const n = parseFloat(String(v))
  return Number.isFinite(n) ? n : null
}

function buildNumericBins(values: number[], binCount: number) {
  const valid = values.filter(v => v != null)
  if (valid.length === 0) return { bins: [], min: 0, max: 0, avg: 0 }

  const min = Math.min(...valid)
  const max = Math.max(...valid)
  const avg = valid.reduce((a, b) => a + b, 0) / valid.length

  if (min === max) {
    return {
      bins: [{ label: fmtNumDisplay(min), count: valid.length, ratio: 1 }],
      min,
      max,
      avg,
    }
  }

  const step = (max - min) / binCount
  const bins: { label: string; count: number; ratio: number }[] = []

  for (let i = 0; i < binCount; i++) {
    const lo = min + step * i
    const hi = i === binCount - 1 ? max + 1e-9 : lo + step
    const count = valid.filter(v => v >= lo && v < hi).length
    bins.push({
      label: `${fmtNumDisplay(lo)}-${fmtNumDisplay(hi)}`,
      count,
      ratio: valid.length > 0 ? count / valid.length : 0,
    })
  }

  return { bins, min, max, avg }
}

function fmtNumDisplay(n: number): string {
  if (Number.isInteger(n)) return n.toFixed(0)
  return n.toFixed(2)
}

function fmtNum(n: number | undefined): string {
  if (n == null) return '-'
  if (Number.isInteger(n)) return n.toLocaleString()
  return n.toFixed(2)
}

const hasData = computed(() => props.rows.length > 0 && props.columns.length > 0)

const columnInfos = computed<ColumnInfo[]>(() => {
  if (!hasData.value) return []

  return props.columns.map((colName, colIdx) => {
    const dataType = props.columnTypes[colIdx] ?? 'varchar'
    const kind = guessKind(dataType)
    const values = props.rows.map(row => row[colIdx])

    const info: ColumnInfo = { name: colName, dataType, kind }

    if (kind === 'numeric') {
      const nums = values.map(toNum).filter(v => v != null) as number[]
      const { bins, min, max, avg } = buildNumericBins(nums, props.binCount)
      info.min = min
      info.max = max
      info.avg = avg
      info.bins = bins
    } else if (kind === 'text') {
      const freq = new Map<string, number>()
      let nullCount = 0
      for (const v of values) {
        if (v == null) {
          nullCount++
          continue
        }
        const s = String(v)
        freq.set(s, (freq.get(s) ?? 0) + 1)
      }
      info.uniqueCount = freq.size
      info.nullCount = nullCount
      info.topValues = Array.from(freq.entries())
        .sort((a, b) => b[1] - a[1])
        .slice(0, props.topN)
        .map(([value, count]) => ({ value, count, ratio: count / values.length }))
    } else if (kind === 'boolean') {
      let trueCount = 0
      let falseCount = 0
      for (const v of values) {
        if (v === true || v === 'true' || v === 1) trueCount++
        else falseCount++
      }
      info.trueCount = trueCount
      info.falseCount = falseCount
      info.trueRatio = values.length > 0 ? trueCount / values.length : 0
      info.falseRatio = values.length > 0 ? falseCount / values.length : 0
    }

    return info
  })
})
</script>

<style scoped>
.distribution-chart {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 8px 0;
}

.empty-state {
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px;
  color: var(--text-tertiary, #999);
  font-size: 12px;
}

.dist-section {
  border: 1px solid var(--border-color, #3c3c3c);
  border-radius: 4px;
  overflow: hidden;
}

.dist-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 6px 10px;
  background: var(--bg-secondary, #2d2d30);
  border-bottom: 1px solid var(--border-color, #3c3c3c);
}

.dist-col-name {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-primary, #ccc);
}

.dist-col-type {
  font-size: 10px;
  color: var(--text-tertiary, #999);
  background: var(--bg-tag, #3c3c3c);
  padding: 1px 6px;
  border-radius: 3px;
}

.dist-stats {
  display: flex;
  gap: 12px;
  padding: 6px 10px;
  font-size: 11px;
  color: var(--text-secondary, #aaa);
}

.dist-stat {
  font-variant-numeric: tabular-nums;
}

/* 直方图 */
.histogram {
  padding: 4px 10px 8px;
}

.histo-row {
  display: flex;
  align-items: center;
  gap: 6px;
  height: 20px;
  font-size: 11px;
}

.histo-label {
  width: 72px;
  text-align: right;
  color: var(--text-tertiary, #999);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  flex-shrink: 0;
}

.histo-bar-wrap {
  flex: 1;
  height: 12px;
  background: var(--bg-hover, #3a3a3d);
  border-radius: 2px;
  overflow: hidden;
}

.histo-bar {
  height: 100%;
  background: linear-gradient(90deg, #188df0, #83bff6);
  border-radius: 2px;
  min-width: 2px;
  transition: width 0.3s ease;
}

.histo-bar-bool {
  background: linear-gradient(90deg, #4caf50, #a5d6a7);
}

.histo-count {
  width: 40px;
  text-align: left;
  color: var(--text-secondary, #aaa);
  font-variant-numeric: tabular-nums;
  flex-shrink: 0;
}

/* 文本频率 */
.freq-list {
  padding: 4px 10px 8px;
}

.freq-row {
  display: flex;
  align-items: center;
  gap: 6px;
  height: 20px;
  font-size: 11px;
}

.freq-label {
  width: 80px;
  text-align: right;
  color: var(--text-secondary, #aaa);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  flex-shrink: 0;
}

.freq-bar-wrap {
  flex: 1;
  height: 12px;
  background: var(--bg-hover, #3a3a3d);
  border-radius: 2px;
  overflow: hidden;
}

.freq-bar {
  height: 100%;
  background: linear-gradient(90deg, #d4a017, #f0d060);
  border-radius: 2px;
  min-width: 2px;
  transition: width 0.3s ease;
}

.freq-count {
  width: 40px;
  text-align: left;
  color: var(--text-secondary, #aaa);
  font-variant-numeric: tabular-nums;
  flex-shrink: 0;
}
</style>
