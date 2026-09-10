<template>
  <div class="sidebar">
    <!-- Search -->
    <div class="sb-search">
      <Search :size="13" class="search-icon" />
      <input
        v-model="query"
        type="text"
        class="sb-search-input"
        :placeholder="$t('navigator.searchDatabaseType')"
      />
    </div>

    <!-- Staging list -->
    <div class="sb-staging">
      <div class="stage-title">
        {{ $t('navigator.stagingList') }}
        <span class="stage-add" @click="$emit('add-staging')">+ {{ $t('navigator.add') }}</span>
      </div>
      <div
        v-for="(s, i) in stagingItems"
        :key="i"
        :class="['stage-item', { active: stagingIndex === i }]"
        @click="$emit('select-staging', i)"
      >
        <span class="stage-dot" :style="{ background: 'var(--brand-accent)' }" />
        <span class="stage-badge" :style="{ background: typeColor(s.driver) }">
          {{ s.name?.slice(0, 2)?.toUpperCase() || 'DB' }}
        </span>
        <span class="stage-name">{{ s.name || $t('navigator.untitled') }}</span>
        <X :size="11" class="stage-close" @click.stop="$emit('remove-staging', i)" />
      </div>
    </div>

    <div class="sb-divider" />

    <!-- Database types -->
    <div class="sb-section">
      <div class="sb-sec-title">{{ $t('navigator.databaseTypes') }}</div>
      <template v-for="group in filteredGroups" :key="group.category">
        <div
          :class="['sb-cat', { expanded: expandedCats.has(group.category) }]"
          @click="toggleCat(group.category)"
        >
          <span class="cat-arrow">▸</span>
          <span class="cat-icon">{{ catIcon(group.category) }}</span>
          <span class="cat-label">{{ catLabel(group.category) }}</span>
          <span class="cat-count">{{ group.totalDrivers }}</span>
        </div>
        <div v-if="expandedCats.has(group.category)" class="sb-types">
          <div
            v-for="dsType in group.types"
            :key="dsType.id"
            :class="['sb-type', { selected: selectedTypeId === dsType.id }]"
            @click="$emit('update:selectedTypeId', dsType.id)"
          >
            <span class="type-dot" :style="{ background: typeColor(dsType.name) }" />
            <span class="type-name">{{ dsType.name }}</span>
            <span class="type-count">{{ dsType.driverCount }} {{ $t('navigator.drivers') }}</span>
          </div>
        </div>
      </template>
      <div v-if="filteredGroups.length === 0" class="sb-no-match">
        {{ $t('navigator.noMatch') }}
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { Search, X } from 'lucide-vue-next'
import { ref, computed } from 'vue'
import { useI18n } from 'vue-i18n'

import { useDriverRegistry } from '../composables/useDriverRegistry'

import type { StagingItem } from '../composables/useAddDataSource'

const { t } = useI18n()
const { drivers, getGroupedTypes } = useDriverRegistry()

defineProps<{
  selectedTypeId: string | null
  stagingItems: StagingItem[]
  stagingIndex: number
}>()

defineEmits<{
  'update:selectedTypeId': [id: string]
  'add-staging': []
  'remove-staging': [index: number]
  'select-staging': [index: number]
}>()

const query = ref('')
const expandedCats = ref(new Set<string>())

// Computed groups: { category, types: DataSourceType & { driverCount }, totalDrivers }
const groupedTypes = computed(() => {
  const groups = getGroupedTypes()
  return Object.entries(groups).map(([category, types]) => {
    const enriched = types.map(t => ({
      ...t,
      driverCount: drivers.value.filter(d => d.type_id === t.id && d.enabled).length,
    }))
    return {
      category,
      types: enriched,
      totalDrivers: enriched.reduce((sum, t) => sum + t.driverCount, 0),
    }
  })
})

const filteredGroups = computed(() => {
  const q = query.value.toLowerCase().trim()
  if (!q) return groupedTypes.value
  return groupedTypes.value
    .map(g => ({
      ...g,
      types: g.types.filter(t => t.name.toLowerCase().includes(q)),
    }))
    .filter(g => g.types.length > 0)
})

// Helpers
const catLabels: Record<string, string> = {
  relational: t('navigator.categoryRelational'),
  'file-based': t('navigator.categoryFile'),
  nosql: t('navigator.categoryNoSQL'),
  analytics: t('navigator.categoryAnalytics'),
  cloud: 'Cloud',
  mq: 'MQ',
  http: 'HTTP API',
}
function catLabel(c: string) {
  return catLabels[c] ?? c
}
function catIcon(c: string) {
  return (
    {
      relational: '🗄',
      'file-based': '📁',
      nosql: '📡',
      analytics: '📊',
      cloud: '☁',
      mq: '📨',
      http: '🌐',
    }[c] ?? '📦'
  )
}

const typeColors: Record<string, string> = {
  mysql: '#00758f',
  postgresql: '#336791',
  sqlite: '#003b57',
  duckdb: '#f9a825',
  mariadb: '#c49a6c',
  mongodb: '#47a248',
  redis: '#dc382d',
  clickhouse: '#faff00',
  sqlserver: '#cc2927',
  oracle: '#f80000',
  couchbase: '#ea2328',
  cockroachdb: '#6933ff',
  snowflake: '#29b5e8',
  bigquery: '#4285f4',
  redshift: '#205b97',
  h2: '#fc7303',
  trino: '#dd00a1',
  presto: '#5890ff',
  elasticsearch: '#fed10a',
}
function typeColor(n?: string) {
  return n ? (typeColors[n.toLowerCase()] ?? '#555') : '#555'
}

function toggleCat(cat: string) {
  const s = new Set(expandedCats.value)
  if (s.has(cat)) s.delete(cat)
  else s.add(cat)
  expandedCats.value = s
}
</script>

<style scoped>
.sidebar {
  width: 220px;
  flex-shrink: 0;
  border-right: 1px solid var(--color-border-subtle);
  padding: 8px;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 2px;
  background: var(--color-bg-elevated);
}
.sb-search {
  position: relative;
  margin-bottom: 4px;
}
.sb-search .search-icon {
  position: absolute;
  left: 8px;
  top: 50%;
  transform: translateY(-50%);
  color: var(--color-text-muted);
}
.sb-search-input {
  width: 100%;
  height: 28px;
  padding: 0 8px 0 26px;
  border: 1px solid var(--color-border-subtle);
  border-radius: 4px;
  font-size: 12px;
  background: var(--color-bg-secondary);
  color: var(--color-text-primary);
  outline: none;
}
.sb-search-input:focus {
  border-color: var(--brand-accent);
}

.sb-staging {
  margin-bottom: 2px;
}
.stage-title {
  font-size: 10px;
  font-weight: 700;
  text-transform: uppercase;
  color: var(--color-text-muted);
  padding: 6px 6px 4px;
  display: flex;
  align-items: center;
  justify-content: space-between;
}
.stage-add {
  font-weight: 400;
  opacity: 0.5;
  cursor: pointer;
  font-size: 10px;
}
.stage-add:hover {
  opacity: 1;
  color: var(--brand-accent);
}
.stage-item {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 5px 8px;
  border-radius: 4px;
  cursor: pointer;
  font-size: 12px;
  color: var(--color-text-secondary);
  transition: background 0.12s;
}
.stage-item:hover {
  background: var(--color-hover);
}
.stage-item.active {
  background: var(--color-bg-active);
  color: var(--brand-accent);
}
.stage-dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  flex-shrink: 0;
}
.stage-badge {
  width: 17px;
  height: 17px;
  border-radius: 3px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 9px;
  font-weight: 700;
  color: var(--color-text-primary);
  flex-shrink: 0;
}
.stage-name {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.stage-close {
  color: var(--color-text-muted);
  opacity: 0.4;
  cursor: pointer;
  flex-shrink: 0;
}
.stage-close:hover {
  opacity: 1;
}

.sb-divider {
  height: 1px;
  background: var(--color-border-subtle);
  margin: 4px 0;
}

.sb-sec-title {
  font-size: 10px;
  font-weight: 700;
  text-transform: uppercase;
  color: var(--color-text-muted);
  padding: 4px 6px;
}
.sb-cat {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 8px;
  border-radius: 4px;
  cursor: pointer;
  font-size: 12px;
  color: var(--color-text-muted);
  transition: all 0.12s;
}
.sb-cat:hover {
  background: var(--color-hover);
  color: var(--color-text-primary);
}
.sb-cat.expanded {
  color: var(--color-text-primary);
}
.cat-arrow {
  font-size: 9px;
  width: 10px;
  flex-shrink: 0;
  transition: transform 0.15s;
}
.sb-cat.expanded .cat-arrow {
  transform: rotate(90deg);
}
.cat-icon {
  font-size: 14px;
  width: 18px;
  text-align: center;
  flex-shrink: 0;
}
.cat-label {
  flex: 1;
}
.cat-count {
  font-size: 10px;
  color: var(--color-text-muted);
  padding: 1px 5px;
  background: var(--color-bg-secondary);
  border-radius: 6px;
}

.sb-types {
  display: flex;
  flex-direction: column;
  gap: 1px;
}
.sb-type {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 5px 8px 5px 28px;
  border-radius: 4px;
  cursor: pointer;
  font-size: 12px;
  color: var(--color-text-muted);
  transition: all 0.12s;
}
.sb-type:hover {
  background: var(--color-hover);
  color: var(--color-text-secondary);
}
.sb-type.selected {
  background: var(--brand-accent-soft);
  color: var(--brand-accent);
  font-weight: 600;
}
.type-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  flex-shrink: 0;
}
.type-name {
  flex: 1;
}
.type-count {
  font-size: 10px;
  color: var(--color-text-muted);
  padding: 1px 5px;
  background: var(--color-bg-secondary);
  border-radius: 6px;
}
.sb-no-match {
  padding: 12px 8px;
  font-size: 11px;
  color: var(--color-text-muted);
  text-align: center;
}
</style>
