<template>
  <div class="form-section">
    <label class="form-label">
      {{ label }}
      <span v-if="required" class="required">*</span>
    </label>
    <input
      v-if="type === 'input'"
      :value="modelValue"
      type="text"
      class="form-input"
      :placeholder="placeholder"
      @input="onInput"
      @keyup.enter="$emit('keyup-enter')"
    />
    <textarea
      v-else
      :value="modelValue"
      class="form-input"
      :placeholder="placeholder"
      :rows="rows"
      @input="onInput"
    />
  </div>
</template>

<script setup lang="ts">
interface Props {
  label: string
  modelValue: string
  type?: 'input' | 'textarea'
  required?: boolean
  placeholder?: string
  rows?: number
}

withDefaults(defineProps<Props>(), {
  type: 'input',
  required: false,
  placeholder: '',
  rows: 3,
})

const emit = defineEmits<{
  'update:modelValue': [value: string]
  'keyup-enter': []
}>()

function onInput(event: Event) {
  emit('update:modelValue', (event.target as HTMLInputElement).value)
}
</script>
