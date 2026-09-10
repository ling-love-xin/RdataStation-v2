<template>
  <div class="data-visualization-panel">
    <!-- 工具栏 -->
    <div class="viz-toolbar">
      <div class="toolbar-left">
        <span class="viz-title">
          <BarChart3 :size="18" />
          {{ t('workbench.dataVisualization') }}
        </span>
      </div>
      <div class="toolbar-center">
        <NRadioGroup v-model:value="chartType" size="small">
          <NRadioButton value="bar">
            <template #icon>
              <BarChart3 :size="14" />
            </template>
            {{ t('workbench.barChart') }}
          </NRadioButton>
          <NRadioButton value="line">
            <template #icon>
              <LineChart :size="14" />
            </template>
            {{ t('workbench.lineChart') }}
          </NRadioButton>
          <NRadioButton value="pie">
            <template #icon>
              <PieChart :size="14" />
            </template>
            {{ t('workbench.pieChart') }}
          </NRadioButton>
          <NRadioButton value="scatter">
            <template #icon>
              <ScatterChart :size="14" />
            </template>
            {{ t('workbench.scatterChart') }}
          </NRadioButton>
        </NRadioGroup>
      </div>
      <div class="toolbar-right">
        <NSelect
          v-model:value="xAxisColumn"
          size="small"
          :placeholder="t('workbench.xAxisColumn')"
          :options="columnOptions"
          style="width: 120px"
        />
        <NSelect
          v-model:value="yAxisColumn"
          size="small"
          :placeholder="t('workbench.yAxisColumn')"
          :options="columnOptions"
          style="width: 120px"
        />
      </div>
    </div>

    <!-- 图表区域 -->
    <div class="chart-container">
      <div ref="chartRef" class="chart-wrapper" />
      <div v-if="!hasData" class="empty-state">
        <NEmpty :description="t('workbench.selectColumnsToGenerate')" />
      </div>
    </div>

    <!-- 数据摘要 -->
    <div v-if="hasData" class="data-summary">
      <NDescriptions :column="4" size="small" bordered>
        <NDescriptionsItem :label="t('workbench.dataRows')">
          {{ data.length }}
        </NDescriptionsItem>
        <NDescriptionsItem :label="t('workbench.xAxisColumn')">
          {{ xAxisColumn || '-' }}
        </NDescriptionsItem>
        <NDescriptionsItem :label="t('workbench.yAxisColumn')">
          {{ yAxisColumn || '-' }}
        </NDescriptionsItem>
        <NDescriptionsItem :label="t('workbench.chartType')">
          {{ chartTypeName }}
        </NDescriptionsItem>
      </NDescriptions>
    </div>
  </div>
</template>

<script setup lang="ts">
import {
  BarChart as EChartsBarChart,
  LineChart as EChartsLineChart,
  PieChart as EChartsPieChart,
  ScatterChart as EChartsScatterChart,
} from 'echarts/charts'
import { TooltipComponent, GridComponent, LegendComponent } from 'echarts/components'
import * as echarts from 'echarts/core'
import { CanvasRenderer } from 'echarts/renderers'
import { BarChart3, LineChart, PieChart, ScatterChart } from 'lucide-vue-next'
import {
  NRadioGroup,
  NRadioButton,
  NSelect,
  NEmpty,
  NDescriptions,
  NDescriptionsItem,
} from 'naive-ui'
import { ref, computed, watch, onMounted, onUnmounted } from 'vue'
import { useI18n } from 'vue-i18n'

import type { EChartsOption } from 'echarts'

// 注册 echarts 组件
echarts.use([
  EChartsBarChart,
  EChartsLineChart,
  EChartsPieChart,
  EChartsScatterChart,
  TooltipComponent,
  GridComponent,
  LegendComponent,
  CanvasRenderer,
])

interface Props {
  data: Record<string, unknown>[]
  columns: string[]
  chartType?: string
}

const props = defineProps<Props>()

const { t } = useI18n()

const chartType = ref<'bar' | 'line' | 'pie' | 'scatter'>(
  ['bar', 'line', 'pie', 'scatter'].includes(props.chartType ?? '')
    ? (props.chartType as 'bar' | 'line' | 'pie' | 'scatter')
    : 'bar'
)
const xAxisColumn = ref('')
const yAxisColumn = ref('')
const chartRef = ref<HTMLElement>()
let chartInstance: ReturnType<typeof echarts.init> | null = null

// 计算属性
const columnOptions = computed(() => {
  return props.columns.map(col => ({
    label: col,
    value: col,
  }))
})

const hasData = computed(() => {
  return props.data.length > 0 && xAxisColumn.value && yAxisColumn.value
})

const chartTypeName = computed(() => {
  const names: Record<string, string> = {
    bar: t('workbench.barChart'),
    line: t('workbench.lineChart'),
    pie: t('workbench.pieChart'),
    scatter: t('workbench.scatterChart'),
  }
  return names[chartType.value] || chartType.value
})

// 初始化图表
const initChart = () => {
  if (!chartRef.value) return

  chartInstance = echarts.init(chartRef.value)
  updateChart()
}

// 更新图表
const updateChart = () => {
  if (!chartInstance || !hasData.value) return

  const xData = props.data.map(row => row[xAxisColumn.value])
  const yData = props.data.map(row => {
    const val = row[yAxisColumn.value]
    return typeof val === 'number' ? val : parseFloat(String(val)) || 0
  })

  let option: EChartsOption

  switch (chartType.value) {
    case 'bar':
      option = {
        tooltip: { trigger: 'axis' },
        xAxis: { type: 'category', data: xData as string[] },
        yAxis: { type: 'value' },
        series: [
          {
            type: 'bar',
            data: yData,
            itemStyle: {
              color: new echarts.graphic.LinearGradient(0, 0, 0, 1, [
                { offset: 0, color: '#83bff6' },
                { offset: 0.5, color: '#188df0' },
                { offset: 1, color: '#188df0' },
              ]),
            },
          },
        ],
      }
      break

    case 'line':
      option = {
        tooltip: { trigger: 'axis' },
        xAxis: { type: 'category', data: xData as string[] },
        yAxis: { type: 'value' },
        series: [
          {
            type: 'line',
            data: yData,
            smooth: true,
            areaStyle: {
              color: new echarts.graphic.LinearGradient(0, 0, 0, 1, [
                { offset: 0, color: 'rgba(128, 255, 165, 0.5)' },
                { offset: 1, color: 'rgba(1, 191, 236, 0.1)' },
              ]),
            },
          },
        ],
      }
      break

    case 'pie':
      option = {
        tooltip: { trigger: 'item' },
        series: [
          {
            type: 'pie',
            radius: ['40%', '70%'],
            data: xData.map((x, i) => ({
              name: String(x),
              value: yData[i],
            })),
            emphasis: {
              itemStyle: {
                shadowBlur: 10,
                shadowOffsetX: 0,
                shadowColor: 'rgba(0, 0, 0, 0.5)',
              },
            },
          },
        ],
      }
      break

    case 'scatter':
      option = {
        tooltip: {
          trigger: 'item',
          formatter: (params: unknown) => {
            const p = params as { data?: [number, number] }
            return `${xAxisColumn.value}: ${p.data?.[0] ?? '-'}<br/>${yAxisColumn.value}: ${p.data?.[1] ?? '-'}`
          },
        } as Record<string, unknown>,
        xAxis: { type: 'value', name: xAxisColumn.value },
        yAxis: { type: 'value', name: yAxisColumn.value },
        series: [
          {
            type: 'scatter',
            data: xData.map((x, i) => [
              typeof x === 'number' ? x : parseFloat(String(x)) || 0,
              yData[i],
            ]),
            symbolSize: 10,
          },
        ],
      }
      break

    default:
      option = {}
  }

  chartInstance.setOption(option, true)
}

// 监听变化
watch(
  [chartType, xAxisColumn, yAxisColumn, () => props.data],
  () => {
    updateChart()
  },
  { deep: true }
)

// 窗口大小变化
const handleResize = () => {
  chartInstance?.resize()
}

onMounted(() => {
  initChart()
  window.addEventListener('resize', handleResize)
})

onUnmounted(() => {
  window.removeEventListener('resize', handleResize)
  chartInstance?.dispose()
})
</script>

<style scoped>
.data-visualization-panel {
  height: 100%;
  display: flex;
  flex-direction: column;
  background-color: var(--bg-primary);
}

.viz-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: 36px;
  padding: 0 var(--spacing-lg);
  background-color: var(--bg-secondary);
  border-bottom: 1px solid var(--border-color);
}

.toolbar-left {
  display: flex;
  align-items: center;
}

.viz-title {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  font-size: var(--font-size-lg);
  font-weight: 500;
  color: var(--text-primary);
}

.toolbar-center {
  display: flex;
  align-items: center;
}

.toolbar-right {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
}

.chart-container {
  flex: 1;
  position: relative;
  padding: var(--spacing-lg);
}

.chart-wrapper {
  width: 100%;
  height: 100%;
}

.empty-state {
  position: absolute;
  top: 50%;
  left: 50%;
  transform: translate(-50%, -50%);
}

.data-summary {
  padding: var(--spacing-md) var(--spacing-lg);
  background-color: var(--bg-secondary);
  border-top: 1px solid var(--border-color);
}
</style>
