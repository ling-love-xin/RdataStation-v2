<template>
  <div class="form-section">
    <label class="form-label">
      {{ label }}
      <span v-if="required" class="required">*</span>
    </label>
    <div class="path-input-wrapper">
      <input
        :value="modelValue"
        type="text"
        class="form-input"
        :placeholder="placeholder"
        readonly
        @input="onInput"
      />
      <button class="btn-browse" @click="$emit('browse')">
        {{ t('workbench.browse') }}
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'

interface Props {
  label: string
  modelValue: string
  required?: boolean
  placeholder?: string
}

withDefaults(defineProps<Props>(), {
  required: false,
  placeholder: '',
})

const emit = defineEmits<{
  'update:modelValue': [value: string]
  browse: []
}>()

const { t } = useI18n()

function onInput(event: Event) {
  emit('update:modelValue', (event.target as HTMLInputElement).value)
}
</script>
