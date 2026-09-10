<template>
  <div class="metadata-section">
    <!-- Schema + Encoding -->
    <div class="adv-sec">
      <div class="adv-inline">
        <div class="adv-cell" style="flex: 2">
          <span class="adv-lbl">{{ $t('navigator.advancedSchema') }}</span>
          <NSelect v-model:value="schemaStrategy" size="small" :options="schemaOpts" />
        </div>
        <div class="adv-cell" style="flex: 1; max-width: 160px">
          <span class="adv-lbl">{{ $t('navigator.advancedEncoding') }}</span>
          <NSelect v-model:value="encoding" size="small" :options="encOpts" />
        </div>
      </div>
    </div>

    <!-- Data source metadata -->
    <div class="adv-sec">
      <div class="sec-title">{{ $t('navigator.dataSourceMeta') || '数据源元数据' }}</div>
      <div class="adv-grid">
        <div class="adv-cell">
          <span class="adv-lbl">{{ $t('navigator.schemaName') || 'Schema 名称' }}</span>
          <NInput
            v-model:value="schemaName"
            size="small"
            :placeholder="$t('navigator.schemaNamePlaceholder') || '默认 schema'"
          />
        </div>
        <div class="adv-cell">
          <span class="adv-lbl">{{ $t('navigator.metadataPath') || '元数据路径' }}</span>
          <NInput
            v-model:value="metadataPath"
            size="small"
            :placeholder="$t('navigator.metadataPathPlaceholder') || '/path/to/metadata'"
          />
        </div>
        <div class="adv-cell">
          <span class="adv-lbl">{{ $t('navigator.tags') || '标签' }}</span>
          <NInput
            v-model:value="tags"
            size="small"
            :placeholder="$t('navigator.tagsPlaceholder') || 'production, analytics'"
          />
        </div>
        <div
          class="adv-cell"
          style="align-items: flex-start; gap: 2px; flex-direction: row; align-items: center"
        >
          <span class="adv-lbl" style="margin-bottom: 0"
            >DuckDB {{ $t('navigator.federation') || '联邦' }}</span
          >
          <NSwitch v-model:value="useDuckdbFed" size="small" />
        </div>
      </div>
      <div class="adv-cell" style="margin-top: 6px">
        <span class="adv-lbl">{{ $t('navigator.options') || '连接选项 (JSON)' }}</span>
        <NInput
          v-model:value="options"
          type="textarea"
          size="small"
          :placeholder="$t('navigator.optionsPlaceholder') || '{&quot;ssl&quot;: true}'"
          :autosize="{ minRows: 2, maxRows: 4 }"
        />
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { NSelect, NSwitch, NInput } from 'naive-ui'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()

const schemaStrategy = defineModel<string>('schemaStrategy', { required: true })
const encoding = defineModel<string>('encoding', { required: true })
const schemaName = defineModel<string>('schemaName', { required: true })
const options = defineModel<string>('options', { required: true })
const metadataPath = defineModel<string>('metadataPath', { required: true })
const tags = defineModel<string>('tags', { required: true })
const useDuckdbFed = defineModel<boolean>('useDuckdbFed', { required: true })

const schemaOpts = [
  { label: t('connection.advancedTab.schemaAuto'), value: 'auto' },
  { label: t('connection.advancedTab.schemaManual'), value: 'manual' },
]
const encOpts = [
  { label: 'UTF-8', value: 'UTF-8' },
  { label: 'GBK', value: 'GBK' },
  { label: 'Latin-1', value: 'Latin-1' },
]
</script>

<style scoped>
.metadata-section {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.adv-sec {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.adv-inline {
  display: flex;
  gap: 8px;
  align-items: flex-end;
}
.adv-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 6px;
}
.adv-cell {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.adv-lbl {
  font-size: 11px;
  color: var(--color-text-muted);
}
.sec-title {
  font-size: 11px;
  font-weight: 600;
  color: var(--color-text-muted);
  margin-bottom: 2px;
}
</style>
