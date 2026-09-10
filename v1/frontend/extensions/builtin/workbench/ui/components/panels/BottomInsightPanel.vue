<template>
  <div class="bottom-insight-panel">
    <NTabs v-model:value="activeTab" type="segment" size="small" animated>
      <NTabPane name="multi" :tab="t('navigator.multiColumnAnalysis')">
        <MultiColumnView
          :temp-table="insightStore.currentTempTable ?? ''"
          :all-columns="insightStore.availableColumns"
        />
      </NTabPane>
      <NTabPane name="history" :tab="t('navigator.historyVersions')">
        <InsightHistoryTab :is-loading="insightStore.isLoading" />
      </NTabPane>
      <NTabPane name="schema" :tab="t('navigator.schemaInsight')">
        <SchemaInsightPanel
          :conn-id="schemaConnId"
          :db-type="schemaDbType"
          :database="schemaDatabase"
          :schema="schemaSchema"
        />
      </NTabPane>
      <NTabPane name="table" :tab="t('navigator.tableProfile')">
        <TableProfileView
          :conn-id="profileConnId"
          :db-type="profileDbType"
          :database="profileDatabase"
          :schema="profileSchema"
          :table="profileTable"
          :auto-evaluate="profileAutoEvaluate"
        />
      </NTabPane>
    </NTabs>
  </div>
</template>

<script setup lang="ts">
import { NTabs, NTabPane } from 'naive-ui'
import { computed, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import InsightHistoryTab from './insight/InsightHistoryTab.vue'
import MultiColumnView from './MultiColumnView.vue'
import SchemaInsightPanel from './SchemaInsightPanel.vue'
import TableProfileView from './TableProfileView.vue'
import { useInsightStore } from '../../stores/insight-store'
import { useLayoutStore } from '../../stores/layout-store'

const { t } = useI18n()
const layoutStore = useLayoutStore()
const insightStore = useInsightStore()

const activeTab = computed({
  get: () => layoutStore.bottomInsightTab,
  set: (val: 'multi' | 'history' | 'schema' | 'table') => {
    layoutStore.setBottomInsightTab(val)
  },
})

const schemaConnId = computed(() => insightStore.pendingSchemaInsightRequest?.connId ?? '')
const schemaDbType = computed(() => insightStore.pendingSchemaInsightRequest?.dbType)
const schemaDatabase = computed(() => insightStore.pendingSchemaInsightRequest?.database ?? '')
const schemaSchema = computed(() => insightStore.pendingSchemaInsightRequest?.schema ?? '')

const profileConnId = computed(() => insightStore.pendingTableProfileRequest?.connId)
const profileDbType = computed(() => insightStore.pendingTableProfileRequest?.dbType)
const profileDatabase = computed(() => insightStore.pendingTableProfileRequest?.database)
const profileSchema = computed(() => insightStore.pendingTableProfileRequest?.schema)
const profileTable = computed(() => insightStore.pendingTableProfileRequest?.table)
const profileAutoEvaluate = computed(() => insightStore.pendingTableProfileRequest?.autoEvaluate)

watch(
  () => insightStore.pendingSchemaInsightRequest,
  request => {
    if (request) {
      layoutStore.setBottomInsightTab('schema')
    }
  }
)

watch(
  () => insightStore.pendingTableProfileRequest,
  request => {
    if (request) {
      layoutStore.setBottomInsightTab('table')
    }
  }
)
</script>

<style scoped>
.bottom-insight-panel {
  height: 100%;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.bottom-insight-panel :deep(.n-tabs) {
  flex: 1;
  display: flex;
  flex-direction: column;
}

.bottom-insight-panel :deep(.n-tabs-pane-wrapper) {
  flex: 1;
  overflow-y: auto;
}
</style>
