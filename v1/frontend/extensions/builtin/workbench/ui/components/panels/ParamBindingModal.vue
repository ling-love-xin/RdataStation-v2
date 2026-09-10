<template>
  <NModal :show="visible" preset="card" @update:show="$emit('cancel')">
    <div class="param-modal">
      <div class="param-header">
        <h3>{{ $t('sqlEditor.paramBinding') }}</h3>
        <NButton quaternary circle size="small" @click="$emit('cancel')">
          <X :size="16" />
        </NButton>
      </div>
      <p class="param-hint">{{ $t('sqlEditor.paramHint') }}</p>
      <div class="param-fields">
        <div v-for="param in params" :key="param" class="param-field">
          <label class="param-label">{{ param }}</label>
          <NInput
            v-model:value="values[param]"
            :placeholder="`Enter value for ${param}`"
            size="small"
          />
        </div>
      </div>
      <div class="param-actions">
        <NButton secondary @click="$emit('cancel')">{{ $t('common.cancel') }}</NButton>
        <NButton type="primary" @click="handleConfirm">{{ $t('sqlEditor.execute') }}</NButton>
      </div>
    </div>
  </NModal>
</template>

<script setup lang="ts">
import { X } from 'lucide-vue-next'
import { NButton, NInput, NModal } from 'naive-ui'
import { reactive, watch } from 'vue'

interface Props {
  visible: boolean
  params: string[]
}

const props = defineProps<Props>()

const emit = defineEmits<{
  (e: 'cancel'): void
  (e: 'confirm', values: Record<string, string>): void
}>()

const values = reactive<Record<string, string>>({})

watch(
  () => props.params,
  newParams => {
    Object.keys(values).forEach(k => delete values[k])
    newParams.forEach(p => {
      values[p] = ''
    })
  },
  { immediate: true }
)

function handleConfirm(): void {
  const result: Record<string, string> = {}
  props.params.forEach(p => {
    result[p] = values[p] || ''
  })
  emit('confirm', result)
}
</script>

<style scoped>
.param-modal {
  width: 420px;
  max-width: 100%;
}

.param-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 12px;
}

.param-header h3 {
  margin: 0;
  font-size: 16px;
  font-weight: 600;
  color: var(--text-primary, #cccccc);
}

.param-hint {
  margin: 0 0 16px;
  font-size: 13px;
  color: var(--text-secondary, #858585);
}

.param-fields {
  display: flex;
  flex-direction: column;
  gap: 12px;
  margin-bottom: 20px;
}

.param-label {
  display: block;
  font-size: 13px;
  font-weight: 500;
  color: var(--text-primary, #cccccc);
  margin-bottom: 4px;
  font-family: 'Consolas', 'Courier New', monospace;
}

.param-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}
</style>
