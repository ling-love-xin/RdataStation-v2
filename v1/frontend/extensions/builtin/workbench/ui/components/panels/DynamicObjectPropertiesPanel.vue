<template>
  <div class="properties-editor">
    <!-- 顶部 Tab 栏：Properties | Data -->
    <div class="top-tabs">
      <div
        :class="['top-tab', { active: activeTopTab === 'properties' }]"
        @click="activeTopTab = 'properties'"
      >
        {{ t('workbench.properties') }}
      </div>
      <div :class="['top-tab', { active: activeTopTab === 'data' }]" @click="activeTopTab = 'data'">
        {{ t('workbench.data') }}
      </div>
    </div>

    <!-- 面包屑 -->
    <div class="breadcrumb">
      <span class="breadcrumb-item">{{ dbName }}</span>
      <span class="breadcrumb-sep">/</span>
      <span class="breadcrumb-item">{{ schemaName }}</span>
      <span class="breadcrumb-sep">/</span>
      <span class="breadcrumb-item breadcrumb-current">{{ result?.title || objectName }}</span>
      <span :class="['scope-badge', scope === 'global' ? 'scope-global' : 'scope-project']">
        {{ scope === 'global' ? t('workbench.scopeGlobal') : t('workbench.scopeProject') }}
      </span>
      <span class="type-badge">{{ result?.objectType || '' }}</span>
    </div>

    <!-- Properties 内容 -->
    <div v-if="activeTopTab === 'properties'" class="props-content">
      <!-- 上 pane：属性网格 -->
      <div class="top-pane">
        <!-- 加载骨架屏 -->
        <div v-if="isLoading" class="loading-skeleton">
          <div v-for="i in 5" :key="i" class="skeleton-row">
            <div class="skeleton-cell skeleton-key" />
            <div class="skeleton-cell skeleton-val" />
            <div class="skeleton-cell skeleton-key" />
            <div class="skeleton-cell skeleton-val" />
          </div>
        </div>
        <div v-else-if="result" class="prop-grid">
          <template v-for="(row, ri) in result.properties" :key="ri">
            <div class="prop-cell prop-key">{{ row.label }}</div>
            <div class="prop-cell prop-val">{{ row.value }}</div>
            <div class="prop-cell prop-key">{{ row.label2 || '' }}</div>
            <div class="prop-cell prop-val">{{ row.value2 || '' }}</div>
          </template>
        </div>
        <div v-else class="empty-state">
          <p>{{ t('workbench.selectObject') }}</p>
        </div>
      </div>

      <!-- 下 pane：左侧子实体 Tabs + 右侧内容 -->
      <div v-if="result && result.subEntities.length > 0" class="bottom-pane">
        <div class="sub-tabs">
          <div
            v-for="se in result.subEntities"
            :key="se.id"
            :class="['sub-tab', { active: activeSubEntity === se.id }]"
            @click="activeSubEntity = se.id"
          >
            <span class="sub-tab-label">{{ se.label }}</span>
            <span class="sub-tab-count">{{ se.count }}</span>
          </div>
        </div>
        <div class="sub-content">
          <template v-for="se in result.subEntities" :key="se.id">
            <div v-if="activeSubEntity === se.id" class="sub-entity-panel">
              <!-- 表格类型 -->
              <template v-if="se.kind === 'table' && se.table">
                <div class="sub-toolbar">
                  <span class="sub-toolbar-title">{{ se.label }}</span>
                  <span class="spacer" />
                  <input
                    v-if="se.table.rows.length > 5"
                    v-model="subFilter"
                    class="sub-search"
                    :placeholder="t('workbench.filter') + '...'"
                  />
                </div>
                <div class="sub-table-wrap">
                  <table class="sub-table">
                    <thead>
                      <tr>
                        <th v-for="col in se.table.columns" :key="col.key">{{ col.label }}</th>
                      </tr>
                    </thead>
                    <tbody>
                      <tr v-for="(row, ri) in filteredRows(se.table.rows)" :key="ri">
                        <td v-for="(cell, ci) in row" :key="ci">
                          <span v-if="ci === 0 && cell === 'PK'" class="tag-pk">PK</span>
                          <span v-else-if="ci === 0 && cell === 'FK'" class="tag-fk">FK</span>
                          <span v-else-if="isNotNull(cell)">{{ cell }}</span>
                          <span v-else>{{ cell }}</span>
                        </td>
                      </tr>
                    </tbody>
                  </table>
                </div>
              </template>

              <!-- 代码类型 -->
              <template v-else-if="se.kind === 'code' && se.code">
                <div class="ddl-toolbar">
                  <span class="sub-toolbar-title">{{ se.label }}</span>
                  <span class="spacer" />
                  <button class="tb-btn" @click="copyDdl(se.code || '')">
                    {{ t('workbench.copy') }}
                  </button>
                  <span v-if="copySuccess" class="copy-toast copy-toast-ok"
                    >{{ t('workbench.copy') }} OK</span
                  >
                  <span v-if="copyError" class="copy-toast copy-toast-err"
                    >{{ t('workbench.copy') }} Failed</span
                  >
                </div>
                <div class="ddl-content">
                  <pre>{{ se.code }}</pre>
                </div>
              </template>

              <!-- 空状态 -->
              <template v-else-if="se.kind === 'empty'">
                <div class="empty-state">
                  <p>{{ se.emptyMessage || t('workbench.noData') }}</p>
                </div>
              </template>
            </div>
          </template>
        </div>
      </div>
    </div>

    <!-- Data Tab 占位 -->
    <div v-else class="data-placeholder">
      <p>{{ t('workbench.dataPlaceholder') }}</p>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import { propertiesRegistry } from '@/extensions/builtin/database/ui/composables/properties-registry'
import type { PropertiesResult } from '@/extensions/builtin/database/ui/composables/properties-registry'
import { useDatabaseNavigatorStore } from '@/extensions/builtin/database/ui/stores/database-navigator-store'

import type { IDockviewPanelProps } from 'dockview-vue'

interface Props {
  params?: IDockviewPanelProps & {
    connectionId?: string
    scope?: 'global' | 'project'
    dbType?: string
    dbName?: string
    schemaName?: string
    objectType?: string
    objectName?: string
    tableName?: string
    columnName?: string
    indexName?: string
    constraintName?: string
  }
}

const { t } = useI18n()
const props = defineProps<Props>()

const navigatorStore = useDatabaseNavigatorStore()

// 从 params 提取参数
const connectionId = computed(() => props.params?.connectionId || '')
const scope = computed(() => props.params?.scope || 'global')
const dbType = computed(() => props.params?.dbType || '')
const dbName = computed(() => props.params?.dbName || '')
const schemaName = computed(() => props.params?.schemaName || '')
const objectType = computed(() => props.params?.objectType || '')
const objectName = computed(() => props.params?.objectName || '')
const tableName = computed(() => props.params?.tableName || '')
const columnName = computed(() => props.params?.columnName || '')
const indexName = computed(() => props.params?.indexName || '')
const constraintName = computed(() => props.params?.constraintName || '')

// 状态
const activeTopTab = ref<'properties' | 'data'>('properties')
const activeSubEntity = ref('')
const subFilter = ref('')
const copySuccess = ref(false)
const copyError = ref(false)

// 构建结果
const result = computed<PropertiesResult | null>(() => {
  const ot = objectType.value
  if (!ot || !connectionId.value) return null

  const extractor = propertiesRegistry[ot]
  if (!extractor) return null

  return extractor({
    connectionId: connectionId.value,
    scope: scope.value as 'global' | 'project',
    dbType: dbType.value,
    catalogName: dbName.value,
    schemaName: schemaName.value,
    objectName: objectName.value,
    tableName: tableName.value,
    columnName: columnName.value,
    indexName: indexName.value,
    constraintName: constraintName.value,
    navigatorStore,
    t,
  })
})

// 加载状态：store 数据是否已就绪
const isLoading = computed(() => {
  const connId = connectionId.value
  if (!connId) return false
  const catalogs = navigatorStore.connectionCatalogs.get(connId)
  return !catalogs || catalogs.length === 0
})

// 默认选中第一个子实体
watch(
  () => result.value?.subEntities,
  entities => {
    if (entities && entities.length > 0) {
      activeSubEntity.value = entities[0].id
    }
  },
  { immediate: true }
)

// 过滤后的行
function filteredRows(rows: string[][]): string[][] {
  if (!subFilter.value) return rows
  const q = subFilter.value.toLowerCase()
  return rows.filter(row => row.some(cell => cell.toLowerCase().includes(q)))
}

function isNotNull(val: string): boolean {
  return val === 'NOT NULL'
}

function copyDdl(code: string): void {
  navigator.clipboard
    .writeText(code)
    .then(() => {
      copySuccess.value = true
      setTimeout(() => {
        copySuccess.value = false
      }, 2000)
    })
    .catch(() => {
      copyError.value = true
      setTimeout(() => {
        copyError.value = false
      }, 3000)
    })
}
</script>

<style scoped>
.properties-editor {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--bg-primary, #1e1e2e);
  color: var(--text-primary, #cdd6f4);
  font-size: 13px;
}

/* ===== 顶部 Tab 栏 ===== */
.top-tabs {
  display: flex;
  flex-shrink: 0;
  background: var(--bg-surface, #1a1b26);
  border-bottom: 1px solid var(--border-color, rgba(255, 255, 255, 0.07));
}
.top-tab {
  padding: 6px 16px;
  cursor: pointer;
  color: var(--text-muted, #6c7086);
  border-bottom: 2px solid transparent;
  user-select: none;
  font-size: 12px;
}
.top-tab:hover {
  color: var(--text-secondary, #a6adc8);
}
.top-tab.active {
  color: var(--text-primary, #cdd6f4);
  border-bottom-color: var(--accent, #89b4fa);
}

/* ===== 面包屑 ===== */
.breadcrumb {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 4px 12px;
  font-size: 11px;
  background: var(--bg-raised, #11111b);
  border-bottom: 1px solid var(--border-color, rgba(255, 255, 255, 0.07));
  flex-shrink: 0;
}
.breadcrumb-item {
  color: var(--text-muted, #6c7086);
}
.breadcrumb-sep {
  opacity: 0.4;
  color: var(--text-muted, #6c7086);
}
.breadcrumb-current {
  color: var(--accent, #89b4fa);
  font-weight: 500;
}
.scope-badge {
  font-size: 10px;
  padding: 2px 8px;
  border-radius: 10px;
  font-weight: 600;
  margin-left: 6px;
}
.scope-global {
  background: rgba(79, 168, 245, 0.12);
  color: #4fa8f5;
}
.scope-project {
  background: rgba(166, 227, 161, 0.12);
  color: #a6e3a1;
}
.type-badge {
  font-size: 9px;
  padding: 1px 6px;
  border-radius: 8px;
  background: rgba(255, 255, 255, 0.08);
  color: var(--text-muted, #6c7086);
  margin-left: 4px;
}

/* ===== Properties 内容 ===== */
.props-content {
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

/* 上 pane */
.top-pane {
  flex: 0 0 auto;
  max-height: 40%;
  overflow-y: auto;
  border-bottom: 1px solid var(--border-color, rgba(255, 255, 255, 0.07));
}
.prop-grid {
  display: grid;
  grid-template-columns: 140px 1fr 140px 1fr;
  font-size: 12px;
}
.prop-cell {
  padding: 4px 10px;
  border-bottom: 1px solid rgba(255, 255, 255, 0.03);
  min-height: 26px;
  display: flex;
  align-items: center;
}
.prop-key {
  color: var(--text-muted, #6c7086);
  font-weight: 500;
}
.prop-val {
  color: var(--text-primary, #cdd6f4);
  font-family: 'Cascadia Code', 'Fira Code', 'Consolas', monospace;
  font-size: 11px;
}

/* 下 pane */
.bottom-pane {
  flex: 1;
  display: flex;
  overflow: hidden;
}
.sub-tabs {
  width: 130px;
  flex-shrink: 0;
  background: var(--bg-surface, #1a1b26);
  border-right: 1px solid var(--border-color, rgba(255, 255, 255, 0.07));
  display: flex;
  flex-direction: column;
  padding: 2px 0;
  overflow-y: auto;
}
.sub-tab {
  padding: 6px 12px;
  font-size: 12px;
  cursor: pointer;
  color: var(--text-muted, #6c7086);
  border-left: 2px solid transparent;
  user-select: none;
  display: flex;
  align-items: center;
  gap: 6px;
}
.sub-tab:hover {
  color: var(--text-secondary, #a6adc8);
  background: var(--bg-hover, rgba(255, 255, 255, 0.05));
}
.sub-tab.active {
  color: var(--text-primary, #cdd6f4);
  background: var(--accent-bg, rgba(137, 180, 250, 0.1));
  border-left-color: var(--accent, #89b4fa);
}
.sub-tab-label {
  flex: 1;
}
.sub-tab-count {
  font-size: 10px;
  color: var(--text-muted, #6c7086);
  background: var(--bg-raised, #11111b);
  padding: 1px 6px;
  border-radius: 8px;
}
.sub-content {
  flex: 1;
  overflow: hidden;
}

.sub-entity-panel {
  display: flex;
  flex-direction: column;
  height: 100%;
}

/* 子实体工具栏 */
.sub-toolbar {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 4px 8px;
  background: var(--bg-raised, #11111b);
  border-bottom: 1px solid var(--border-color, rgba(255, 255, 255, 0.07));
  flex-shrink: 0;
}
.sub-toolbar-title {
  font-size: 11px;
  color: var(--text-secondary, #a6adc8);
  font-weight: 500;
}
.spacer {
  flex: 1;
}
.sub-search {
  width: 140px;
  padding: 3px 8px;
  font-size: 11px;
  background: var(--bg-raised, #11111b);
  border: 1px solid var(--border-color, rgba(255, 255, 255, 0.07));
  border-radius: 3px;
  color: var(--text-primary, #cdd6f4);
  outline: none;
}
.sub-search:focus {
  border-color: var(--accent, #89b4fa);
}
.sub-search::placeholder {
  color: var(--text-muted, #6c7086);
}

/* 子实体表格 */
.sub-table-wrap {
  flex: 1;
  overflow: auto;
}
.sub-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
}
.sub-table th {
  padding: 5px 10px;
  text-align: left;
  font-weight: 600;
  color: var(--text-muted, #6c7086);
  font-size: 11px;
  background: var(--bg-raised, #11111b);
  border-bottom: 1px solid var(--border-color, rgba(255, 255, 255, 0.07));
  position: sticky;
  top: 0;
  z-index: 1;
  white-space: nowrap;
}
.sub-table td {
  padding: 4px 10px;
  border-bottom: 1px solid rgba(255, 255, 255, 0.03);
  white-space: nowrap;
}
.sub-table tbody tr:hover td {
  background: var(--bg-hover, rgba(255, 255, 255, 0.05));
}
.tag-pk {
  color: #f9e2af;
  font-weight: 600;
  font-size: 10px;
  background: rgba(249, 226, 175, 0.15);
  padding: 1px 4px;
  border-radius: 3px;
}
.tag-fk {
  color: #cba6f7;
  font-size: 10px;
  background: rgba(203, 166, 247, 0.15);
  padding: 1px 4px;
  border-radius: 3px;
}

/* DDL */
.ddl-toolbar {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 4px 8px;
  background: var(--bg-raised, #11111b);
  border-bottom: 1px solid var(--border-color, rgba(255, 255, 255, 0.07));
  flex-shrink: 0;
}
.tb-btn {
  padding: 3px 10px;
  font-size: 11px;
  border: 1px solid var(--border-color, rgba(255, 255, 255, 0.07));
  background: transparent;
  color: var(--text-muted, #6c7086);
  cursor: pointer;
  border-radius: 3px;
}
.tb-btn:hover {
  background: var(--bg-hover, rgba(255, 255, 255, 0.05));
  color: var(--text-primary, #cdd6f4);
}

/* 复制状态提示 */
.copy-toast {
  font-size: 10px;
  padding: 1px 6px;
  border-radius: 3px;
  animation: toast-fade 2s ease-out forwards;
}
.copy-toast-ok {
  color: #a6e3a1;
  background: rgba(166, 227, 161, 0.15);
}
.copy-toast-err {
  color: #f38ba8;
  background: rgba(243, 139, 168, 0.15);
}
@keyframes toast-fade {
  0% {
    opacity: 1;
  }
  70% {
    opacity: 1;
  }
  100% {
    opacity: 0;
  }
}
.ddl-content {
  flex: 1;
  overflow: auto;
  background: var(--bg-raised, #11111b);
}
.ddl-content pre {
  padding: 12px;
  margin: 0;
  font-family: 'Cascadia Code', 'Fira Code', 'Consolas', monospace;
  font-size: 12px;
  line-height: 1.6;
  color: var(--text-primary, #cdd6f4);
  white-space: pre;
  overflow-x: auto;
}

/* 空状态 */
.empty-state {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
  color: var(--text-muted, #6c7086);
  font-size: 13px;
}
.data-placeholder {
  display: flex;
  align-items: center;
  justify-content: center;
  flex: 1;
  color: var(--text-muted, #6c7086);
  font-size: 13px;
}

/* 加载骨架屏 */
.loading-skeleton {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 8px;
}
.skeleton-row {
  display: grid;
  grid-template-columns: 1fr 2fr 1fr 2fr;
  gap: 4px;
}
.skeleton-cell {
  height: 14px;
  border-radius: 3px;
  background: linear-gradient(
    90deg,
    var(--bg-surface, #1e1e2e) 25%,
    var(--bg-raised, #313244) 50%,
    var(--bg-surface, #1e1e2e) 75%
  );
  background-size: 200% 100%;
  animation: skeleton-pulse 1.5s ease-in-out infinite;
}
.skeleton-key {
  max-width: 80px;
}
.skeleton-val {
  max-width: 160px;
}
@keyframes skeleton-pulse {
  0% {
    background-position: 200% 0;
  }
  100% {
    background-position: -200% 0;
  }
}
</style>
