<template>
  <div class="filter-panel" :class="{ expanded: isExpanded }">
    <button class="filter-toggle" @click="isExpanded = !isExpanded">
      <Filter :size="14" />
      <span>{{ t('navigator.filter') }}</span>
      <ChevronDown :size="14" :class="{ rotated: isExpanded }" />
    </button>

    <Transition name="slide">
      <div v-if="isExpanded" class="filter-content">
        <div class="filter-section">
          <label class="filter-label">{{ t('navigator.databaseType') }}</label>
          <div class="filter-options">
            <button
              v-for="type in databaseTypes"
              :key="type.value"
              class="filter-option"
              :class="{ active: filters.databaseType === type.value }"
              @click="updateFilter('databaseType', filters.databaseType === type.value ? '' : type.value)"
            >
              {{ type.label }}
            </button>
          </div>
        </div>

        <div class="filter-section">
          <label class="filter-label">{{ t('navigator.connectionStatus') }}</label>
          <div class="filter-options">
            <button
              v-for="status in connectionStatuses"
              :key="status.value"
              class="filter-option"
              :class="{ active: filters.connectionStatus === status.value }"
              @click="updateFilter('connectionStatus', filters.connectionStatus === status.value ? '' : status.value)"
            >
              {{ status.label }}
            </button>
          </div>
        </div>

        <div class="filter-section">
          <label class="filter-label">{{ t('navigator.nodeType') }}</label>
          <div class="filter-options">
            <button
              v-for="type in nodeTypes"
              :key="type.value"
              class="filter-option"
              :class="{ active: filters.nodeTypes.includes(type.value) }"
              @click="toggleNodeType(type.value)"
            >
              {{ type.label }}
            </button>
          </div>
        </div>

        <div class="filter-section">
          <label class="filter-label">
            <input v-model="filters.showSystemObjects" type="checkbox" />
            {{ t('navigator.showSystemObjects') }}
          </label>
        </div>

        <div class="filter-actions">
          <button class="btn-reset" @click="resetFilters">{{ t('navigator.reset') }}</button>
          <button class="btn-apply" @click="applyFilters">{{ t('navigator.apply') }}</button>
        </div>
      </div>
    </Transition>
  </div>
</template>

<script setup lang="ts">
import { Filter, ChevronDown } from 'lucide-vue-next'
import { ref, reactive } from 'vue'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()

interface Filters {
  databaseType: string
  connectionStatus: string
  nodeTypes: string[]
  showSystemObjects: boolean
}

const emit = defineEmits<{
  filtersChange: [filters: Filters]
}>()

const isExpanded = ref(false)

const filters = reactive<Filters>({
  databaseType: '',
  connectionStatus: '',
  nodeTypes: ['table', 'view'],
  showSystemObjects: false,
})

const databaseTypes = [
  { value: '', label: t('navigator.all') },
  { value: 'mysql', label: 'MySQL' },
  { value: 'postgresql', label: 'PostgreSQL' },
  { value: 'sqlite', label: 'SQLite' },
  { value: 'duckdb', label: 'DuckDB' },
]

const connectionStatuses = [
  { value: '', label: t('navigator.all') },
  { value: 'connected', label: t('navigator.connected') },
  { value: 'connecting', label: t('navigator.connecting') },
  { value: 'disconnected', label: t('navigator.disconnected') },
]

const nodeTypes = [
  { value: 'table', label: t('navigator.table') },
  { value: 'view', label: t('navigator.view') },
  { value: 'procedure', label: t('navigator.procedure') },
  { value: 'function', label: t('navigator.function') },
  { value: 'column', label: t('navigator.column') },
]

function updateFilter(key: keyof Filters, value: string) {
  ;(filters[key] as string) = value
}

function toggleNodeType(type: string) {
  const index = filters.nodeTypes.indexOf(type)
  if (index === -1) {
    filters.nodeTypes.push(type)
  } else {
    filters.nodeTypes.splice(index, 1)
  }
}

function resetFilters() {
  filters.databaseType = ''
  filters.connectionStatus = ''
  filters.nodeTypes = ['table', 'view']
  filters.showSystemObjects = false
  emit('filtersChange', { ...filters })
}

function applyFilters() {
  isExpanded.value = false
  emit('filtersChange', { ...filters })
}
</script>

<style scoped>
.filter-panel {
  background: var(--bg-primary);
  border-radius: 8px;
  border: 1px solid var(--border-color);
  overflow: hidden;
}

.filter-toggle {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 8px 12px;
  background: transparent;
  border: none;
  cursor: pointer;
  font-size: 13px;
  color: var(--text-primary);
  width: 100%;
  transition: background-color 0.15s;
}

.filter-toggle:hover {
  background: var(--bg-tertiary);
}

.filter-toggle span {
  flex: 1;
  text-align: left;
}

.rotated {
  transform: rotate(180deg);
}

.filter-content {
  padding: 12px;
  border-top: 1px solid var(--border-color);
}

.filter-section {
  margin-bottom: 16px;
}

.filter-section:last-of-type {
  margin-bottom: 0;
}

.filter-label {
  display: block;
  font-size: 12px;
  font-weight: 500;
  color: var(--text-secondary);
  margin-bottom: 8px;
  cursor: pointer;
}

.filter-label input[type='checkbox'] {
  margin-right: 6px;
}

.filter-options {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.filter-option {
  padding: 4px 10px;
  background: var(--bg-secondary);
  border: 1px solid var(--border-color);
  border-radius: 4px;
  font-size: 12px;
  color: var(--text-secondary);
  cursor: pointer;
  transition: all 0.15s;
}

.filter-option:hover {
  background: var(--bg-tertiary);
}

.filter-option.active {
  background: var(--primary-color);
  border-color: var(--primary-color);
  color: white;
}

.filter-actions {
  display: flex;
  gap: 8px;
  justify-content: flex-end;
  margin-top: 16px;
  padding-top: 12px;
  border-top: 1px solid var(--border-color);
}

.btn-reset,
.btn-apply {
  padding: 4px 12px;
  border-radius: 4px;
  font-size: 12px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.15s;
}

.btn-reset {
  background: transparent;
  border: 1px solid var(--border-color);
  color: var(--text-secondary);
}

.btn-reset:hover {
  background: var(--bg-tertiary);
}

.btn-apply {
  background: var(--primary-color);
  border: none;
  color: white;
}

.btn-apply:hover {
  background: var(--primary-dark);
}

.slide-enter-active,
.slide-leave-active {
  transition: all 0.3s ease;
  overflow: hidden;
}

.slide-enter-from,
.slide-leave-to {
  opacity: 0;
  max-height: 0;
}

.slide-enter-to,
.slide-leave-from {
  max-height: 400px;
}
</style>
