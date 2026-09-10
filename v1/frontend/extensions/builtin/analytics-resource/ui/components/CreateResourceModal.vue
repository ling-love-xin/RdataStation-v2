<template>
  <NModal :show="show" :on-update:show="(val: boolean) => !val && emit('close')">
    <NCard
      :title="isEdit ? t('analyticsResource.editResource') : t('analyticsResource.createResource')"
      closable
      style="width: 520px"
      @close="emit('close')"
    >
      <NForm label-placement="left" label-width="auto">
        <NFormItem :label="t('analyticsResource.resourceType')" required>
          <NSelect
            v-model:value="form.resource_type"
            :options="resourceTypeOptions"
            :disabled="isEdit"
          />
        </NFormItem>

        <NFormItem :label="t('analyticsResource.resourceName')" required>
          <NInput v-model:value="form.name" :placeholder="t('analyticsResource.resourceName')" />
        </NFormItem>

        <NFormItem :label="t('analyticsResource.alias')">
          <NInput
            v-model:value="form.alias"
            :placeholder="t('analyticsResource.aliasPlaceholder')"
          />
        </NFormItem>

        <NFormItem :label="t('analyticsResource.scope')" required>
          <NSelect v-model:value="form.scope" :options="scopeOptions" />
        </NFormItem>

        <NFormItem
          v-if="form.resource_type === 'table'"
          :label="t('analyticsResource.rowCountLabel')"
        >
          <NInputNumber
            v-model:value="form.row_count"
            :placeholder="t('analyticsResource.rowCountLabel')"
            :min="0"
          />
        </NFormItem>

        <NFormItem
          v-if="form.resource_type === 'table'"
          :label="t('analyticsResource.columnCount')"
        >
          <NInputNumber
            v-model:value="form.column_count"
            :placeholder="t('analyticsResource.columnCount')"
            :min="0"
          />
        </NFormItem>

        <NFormItem
          v-if="form.resource_type === 'file'"
          :label="t('analyticsResource.fileSizeLabel')"
        >
          <NInputNumber
            v-model:value="form.file_size"
            :placeholder="t('analyticsResource.fileSizeLabel')"
            :min="0"
          />
        </NFormItem>

        <NFormItem :label="t('analyticsResource.sourceQuery')">
          <NInput
            v-model:value="form.source_query"
            type="textarea"
            placeholder="SELECT * FROM table_name"
            :rows="3"
          />
        </NFormItem>

        <NFormItem :label="t('analyticsResource.configJson')">
          <NInput
            v-model:value="configJson"
            type="textarea"
            :placeholder="t('analyticsResource.configJsonPlaceholder')"
            :rows="4"
            @update:value="jsonError = null"
          />
          <div
            v-if="jsonError"
            style="
              color: var(--brand-danger, #d63031);
              font-size: var(--font-size-sm, 12px);
              margin-top: var(--spacing-xs, 4px);
            "
          >
            {{ jsonError }}
          </div>
        </NFormItem>
      </NForm>

      <template #footer>
        <NSpace justify="end">
          <NButton @click="emit('close')">
            {{ t('analyticsResource.cancel') }}
          </NButton>
          <NButton type="primary" :disabled="!isValid" @click="handleSubmit">
            {{ isEdit ? t('analyticsResource.save') : t('analyticsResource.create') }}
          </NButton>
        </NSpace>
      </template>
    </NCard>
  </NModal>
</template>

<script setup lang="ts">
import {
  NButton,
  NCard,
  NForm,
  NFormItem,
  NInput,
  NInputNumber,
  NModal,
  NSelect,
  NSpace,
} from 'naive-ui'
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'

import type {
  AnalyticsResource,
  CreateResourceRequest,
  ResourceScope,
  ResourceType,
} from '../../types'

const { t } = useI18n()

const props = defineProps<{
  show: boolean
  editResource?: AnalyticsResource
}>()

const emit = defineEmits<{
  close: []
  create: [input: CreateResourceRequest]
  update: [id: string, input: CreateResourceRequest]
}>()

const isEdit = computed(() => !!props.editResource)

const resourceTypeOptions = computed(() => [
  { label: `🔌 ${t('analyticsResource.connection')}`, value: 'connection' },
  { label: `📊 ${t('analyticsResource.table')}`, value: 'table' },
  { label: `📄 ${t('analyticsResource.file')}`, value: 'file' },
])

const scopeOptions = computed(() => [
  { label: `🌍 ${t('analyticsResource.global')}`, value: 'global' },
  { label: `📂 ${t('analyticsResource.project')}`, value: 'project' },
  { label: `📌 ${t('analyticsResource.session')}`, value: 'session' },
])

const form = ref({
  resource_type: 'connection' as ResourceType,
  name: '',
  alias: '',
  scope: 'project' as ResourceScope,
  row_count: undefined as number | undefined,
  column_count: undefined as number | undefined,
  file_size: undefined as number | undefined,
  parent_resource_id: undefined as string | undefined,
  source_query: undefined as string | undefined,
})

const configJson = ref('{}')
const jsonError = ref<string | null>(null)

const isValid = computed(() => form.value.name.trim() !== '')

function handleSubmit() {
  try {
    const config = JSON.parse(configJson.value)

    const input: CreateResourceRequest = {
      resource_type: form.value.resource_type,
      name: form.value.name.trim(),
      alias: form.value.alias.trim() || undefined,
      scope: form.value.scope,
      config,
      row_count: form.value.row_count,
      column_count: form.value.column_count,
      file_size: form.value.file_size,
      parent_resource_id: form.value.parent_resource_id,
      source_query: form.value.source_query || undefined,
    }

    jsonError.value = null
    if (isEdit.value && props.editResource) {
      emit('update', props.editResource.id, input)
    } else {
      emit('create', input)
    }
  } catch {
    jsonError.value = t('analyticsResource.jsonFormatError')
  }
}

onMounted(() => {
  if (props.editResource) {
    const r = props.editResource
    form.value = {
      resource_type: r.resource_type,
      name: r.name,
      alias: r.alias || '',
      scope: r.scope,
      row_count: r.row_count,
      column_count: r.column_count,
      file_size: r.file_size,
      parent_resource_id: r.parent_resource_id,
      source_query: r.source_query,
    }
    configJson.value = JSON.stringify(r.config, null, 2)
  }
})
</script>
