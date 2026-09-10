<template>
  <NDrawer :show="visible" :width="420" placement="right" :on-update:show="onClose">
    <NDrawerContent :title="`${columnName} 列 · 高级配置`" closable>
      <div class="drawer-body">
        <div class="field-header">
          <div class="field-row">
            <span class="field-label">字段名</span>
            <NInput v-model:value="localFieldName" size="small" style="flex: 1" />
          </div>
          <div class="field-row">
            <span class="field-label">类型</span>
            <NSelect
              v-model:value="localDataType"
              size="small"
              :options="dataTypeOptions"
              style="flex: 1"
            />
          </div>
        </div>

        <div class="section-title">数据类型筛选</div>
        <div class="filter-tabs">
          <NButton
            v-for="tab in filterTabs"
            :key="tab.key"
            :type="activeFilter === tab.key ? 'primary' : 'default'"
            size="tiny"
            :secondary="activeFilter !== tab.key"
            @click="activeFilter = tab.key"
          >
            {{ tab.label }}
          </NButton>
        </div>

        <div class="section-title">生成器选择</div>
        <div class="generator-list">
          <div
            v-for="gen in filteredGenerators"
            :key="gen.type"
            :class="['generator-item', { selected: localGeneratorType === gen.type }]"
            @click="selectGenerator(gen)"
          >
            <div class="gen-info">
              <span v-if="gen.recommended" class="recommend-badge">🟢 推荐</span>
              <span class="gen-name">{{ gen.label }}</span>
              <span class="gen-type">{{ gen.type }}</span>
            </div>
            <span class="gen-example">{{ gen.example }}</span>
          </div>
          <div v-if="filteredGenerators.length === 0" class="empty-list">
            该分类暂无可用生成器
          </div>
        </div>

        <NDivider style="margin: 12px 0" />

        <div v-if="paramFields.length > 0" class="params-section">
          <div class="section-title">参数配置</div>
          <div class="params-form">
            <div v-for="field in paramFields" :key="field.name" class="param-field">
              <label class="param-field-label">{{ field.label }}</label>

              <NInputNumber
                v-if="field.type === 'number'"
                :value="(localParams[field.name] as number) ?? (field.default as number)"
                size="small"
                :min="field.min"
                :max="field.max"
                @update:value="(v: number | null) => setParam(field.name, v ?? field.default)"
              />

              <NInput
                v-if="field.type === 'text'"
                :value="(localParams[field.name] as string) ?? ''"
                size="small"
                @update:value="(v: string) => setParam(field.name, v)"
              />

              <NSwitch
                v-if="field.type === 'boolean'"
                :value="(localParams[field.name] as boolean) ?? false"
                @update:value="(v: boolean) => setParam(field.name, v)"
              />
            </div>
          </div>
        </div>

        <div v-else class="empty-params"> 此生成器无需额外配置参数 </div>

        <div v-if="showTimeSeries" class="timeseries-section">
          <NDivider style="margin: 12px 0" />
          <div class="section-title">时序关联（可选）</div>
          <div class="ts-row">
            <span class="ts-label">关联日期列</span>
            <NSelect
              v-model:value="tsDateColumn"
              size="small"
              :options="dateColumnOptions"
              placeholder="选择日期列"
              style="flex: 1"
            />
          </div>
          <div v-if="tsDateColumn" class="ts-row">
            <span class="ts-label">趋势方向</span>
            <NSelect v-model:value="tsTrend" size="small" :options="trendOptions" style="flex: 1" />
          </div>
          <div v-if="tsDateColumn && tsTrend === 'cycle'" class="ts-row">
            <span class="ts-label">周期长度(天)</span>
            <NInputNumber v-model:value="tsCycleLength" size="small" :min="1" style="flex: 1" />
          </div>
          <div v-if="tsDateColumn" class="ts-row">
            <span class="ts-label">波动幅度(%)</span>
            <NInputNumber
              v-model:value="tsVolatility"
              size="small"
              :min="0"
              :max="100"
              style="flex: 1"
            />
          </div>
        </div>

        <NDivider style="margin: 12px 0" />
        <div class="section-title">其他选项</div>
        <div class="other-options">
          <div class="opt-row">
            <span class="opt-label">允许空值比例</span>
            <NInputNumber
              v-model:value="localNullRatio"
              size="small"
              :min="0"
              :max="1"
              :step="0.05"
              style="width: 80px"
            />
            <span class="opt-pct">{{ Math.round(localNullRatio * 100) }}%</span>
          </div>
          <div class="opt-row">
            <span class="opt-label">唯一值</span>
            <NSwitch v-model:value="localUnique" />
          </div>
        </div>

        <div class="source-info"> 生成器来源: fake-rs · {{ generatorModule }} </div>
      </div>

      <template #footer>
        <div class="drawer-footer">
          <NButton size="small" quaternary @click="restoreDefault">恢复智能默认</NButton>
          <div class="footer-right">
            <NButton size="small" @click="onClose">取消</NButton>
            <NButton type="primary" size="small" @click="onApply">应用</NButton>
          </div>
        </div>
      </template>
    </NDrawerContent>
  </NDrawer>
</template>

<script setup lang="ts">
import {
  NDrawer,
  NDrawerContent,
  NDivider,
  NButton,
  NInput,
  NInputNumber,
  NSelect,
  NSwitch,
} from 'naive-ui'
import { ref, computed, watch } from 'vue'

import type { GeneratorType, ColumnDataType } from '@/shared/api/mock-api'

import {
  GENERATOR_LIST,
  GENERATOR_PARAM_SCHEMA,
  GENERATOR_MODULES,
  DATA_TYPE_OPTIONS,
  FILTER_TABS,
} from './mockGeneratorDefs'

import type { GeneratorDesc } from './mockGeneratorDefs'

const props = defineProps<{
  show: boolean
  generatorType: GeneratorType
  currentParams: Record<string, unknown>
  columnName: string
  columnIndex: number
  columnDataType: ColumnDataType
  columnNullableRatio: number
  columnUnique: boolean
  allColumns: Array<{ name: string; dataType: string }>
}>()

const emit = defineEmits<{
  'update:show': [value: boolean]
  apply: [
    index: number,
    type: GeneratorType,
    params: Record<string, unknown>,
    fieldName: string,
    dataType: ColumnDataType,
    nullableRatio: number,
    unique: boolean,
  ]
}>()

const visible = ref(props.show)
const activeFilter = ref('all')
const localGeneratorType = ref<GeneratorType>(props.generatorType)
const localParams = ref<Record<string, unknown>>({ ...props.currentParams })
const localFieldName = ref(props.columnName)
const localDataType = ref<ColumnDataType>(props.columnDataType)
const localNullRatio = ref(props.columnNullableRatio)
const localUnique = ref(props.columnUnique)
const tsDateColumn = ref<string | null>(null)
const tsTrend = ref<'growth' | 'decline' | 'cycle' | 'walk'>('growth')
const tsCycleLength = ref(30)
const tsVolatility = ref(10)

watch(
  () => props.show,
  val => {
    visible.value = val
    if (val) {
      localGeneratorType.value = props.generatorType
      localParams.value = { ...props.currentParams }
      localFieldName.value = props.columnName
      localDataType.value = props.columnDataType
      localNullRatio.value = props.columnNullableRatio
      localUnique.value = props.columnUnique
      const genDesc = GENERATOR_LIST.find(g => g.type === props.generatorType)
      if (genDesc) {
        activeFilter.value = genDesc.category
      }
    }
  }
)

watch(visible, val => emit('update:show', val))

// ===================== 计算属性 =====================

const dataTypeOptions = DATA_TYPE_OPTIONS

const filterTabs = FILTER_TABS

const filteredGenerators = computed(() => {
  if (activeFilter.value === 'all') return GENERATOR_LIST
  return GENERATOR_LIST.filter(g => g.category === activeFilter.value)
})

const paramFields = computed(() => GENERATOR_PARAM_SCHEMA[localGeneratorType.value] ?? [])

const generatorModule = computed(
  () => GENERATOR_MODULES[localGeneratorType.value] ?? localGeneratorType.value
)

const dateColumnOptions = computed(() =>
  props.allColumns
    .filter(c => c.name !== props.columnName)
    .map(c => ({ label: c.name, value: c.name }))
)

const trendOptions = [
  { label: '增长', value: 'growth' },
  { label: '下降', value: 'decline' },
  { label: '周期波动', value: 'cycle' },
  { label: '随机游走', value: 'walk' },
]

const showTimeSeries = computed(() => {
  const gen = GENERATOR_LIST.find(g => g.type === localGeneratorType.value)
  return gen?.category === 'numeric'
})

// ===================== 方法 =====================

function selectGenerator(gen: GeneratorDesc) {
  localGeneratorType.value = gen.type
  localParams.value = {}
}

function setParam(name: string, value: unknown) {
  localParams.value = { ...localParams.value, [name]: value }
}

function restoreDefault() {
  localParams.value = {}
  const defaultGen = GENERATOR_LIST.find(g => g.category === activeFilter.value && g.recommended)
  if (defaultGen) {
    localGeneratorType.value = defaultGen.type
  }
}

function onApply() {
  emit(
    'apply',
    props.columnIndex,
    localGeneratorType.value,
    localParams.value,
    localFieldName.value,
    localDataType.value,
    localNullRatio.value,
    localUnique.value
  )
  visible.value = false
}

function onClose() {
  visible.value = false
}
</script>

<style scoped>
.drawer-body {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-xs);
}

.field-header {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-xs);
  margin-bottom: var(--spacing-xs);
}

.field-row {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
}

.field-label {
  font-size: var(--font-size-sm);
  color: var(--color-text-muted);
  width: 48px;
  flex-shrink: 0;
}

.section-title {
  font-size: var(--font-size-sm);
  font-weight: 600;
  color: var(--color-text-muted);
  margin-top: var(--spacing-xs);
  margin-bottom: var(--spacing-xs);
}

.filter-tabs {
  display: flex;
  flex-wrap: wrap;
  gap: var(--spacing-xs);
}

.generator-list {
  max-height: 240px;
  overflow-y: auto;
  border: 1px solid var(--color-border);
  border-radius: var(--border-radius-sm);
}

.generator-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: var(--spacing-xs) var(--spacing-sm);
  cursor: pointer;
  border-bottom: 1px solid var(--color-border-subtle);
  font-size: var(--font-size-sm);
}

.generator-item:last-child {
  border-bottom: none;
}

.generator-item:hover {
  background: var(--color-hover);
}

.generator-item.selected {
  background: var(--brand-accent-soft);
  border-left: 2px solid var(--brand-accent);
}

.gen-info {
  display: flex;
  gap: var(--spacing-xs);
  align-items: center;
}

.recommend-badge {
  font-size: var(--font-size-xs);
  color: var(--brand-success);
  font-weight: 500;
}

.gen-name {
  font-weight: 500;
}

.gen-type {
  font-size: var(--font-size-xs);
  color: var(--color-text-muted);
  font-family: var(--font-mono);
}

.gen-example {
  font-size: var(--font-size-xs);
  color: var(--color-text-muted);
  max-width: 180px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  flex-shrink: 0;
}

.empty-list {
  padding: var(--spacing-xl) 0;
  text-align: center;
  font-size: var(--font-size-sm);
  color: var(--color-text-muted);
}

.params-form {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-sm);
}

.param-field {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.param-field-label {
  font-size: var(--font-size-sm);
  font-weight: 500;
  color: var(--color-text-secondary);
}

.empty-params {
  text-align: center;
  padding: var(--spacing-sm) 0;
  font-size: var(--font-size-md);
  color: var(--color-text-muted);
}

.timeseries-section {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-xs);
}

.ts-row {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
}

.ts-label {
  font-size: var(--font-size-sm);
  color: var(--color-text-muted);
  width: 72px;
  flex-shrink: 0;
}

.other-options {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-xs);
}

.opt-row {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
}

.opt-label {
  font-size: var(--font-size-sm);
  color: var(--color-text-muted);
}

.opt-pct {
  font-size: var(--font-size-sm);
  color: var(--brand-accent);
  font-weight: 500;
}

.source-info {
  margin-top: var(--spacing-xs);
  font-size: var(--font-size-xs);
  color: var(--color-text-muted);
  text-align: right;
}

.drawer-footer {
  display: flex;
  justify-content: space-between;
  align-items: center;
  width: 100%;
}

.footer-right {
  display: flex;
  gap: var(--spacing-sm);
}
</style>
