<template>
  <button
    :class="[
      'base-button',
      `variant-${variant}`,
      `size-${size}`,
      { loading, disabled: disabled || loading },
    ]"
    :disabled="disabled || loading"
    @click="$emit('click', $event)"
  >
    <span v-if="loading" class="spinner"></span>
    <slot />
  </button>
</template>

<script setup lang="ts">
defineProps<{
  variant?: 'primary' | 'secondary' | 'danger' | 'ghost'
  size?: 'sm' | 'md' | 'lg'
  disabled?: boolean
  loading?: boolean
}>()

defineEmits<{
  click: [event: MouseEvent]
}>()
</script>

<style scoped>
.base-button {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  border: none;
  border-radius: 8px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.2s;
}

.base-button:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

/* Variants */
.variant-primary {
  background: var(--primary-color);
  color: white;
}

.variant-primary:hover:not(:disabled) {
  background: var(--primary-hover);
}

.variant-secondary {
  background: var(--bg-secondary);
  color: var(--text-secondary);
  border: 1px solid var(--border-color);
}

.variant-secondary:hover:not(:disabled) {
  border-color: var(--primary-color);
  color: var(--primary-color);
}

.variant-danger {
  background: var(--danger-color);
  color: white;
}

.variant-danger:hover:not(:disabled) {
  background: var(--danger-hover);
}

.variant-ghost {
  background: transparent;
  color: var(--text-secondary);
}

.variant-ghost:hover:not(:disabled) {
  background: var(--bg-hover);
  color: var(--text-primary);
}

/* Sizes */
.size-sm {
  padding: 6px 12px;
  font-size: 12px;
}

.size-md {
  padding: 10px 16px;
  font-size: 14px;
}

.size-lg {
  padding: 14px 24px;
  font-size: 16px;
}

.spinner {
  width: 16px;
  height: 16px;
  border: 2px solid rgba(255, 255, 255, 0.3);
  border-top-color: currentColor;
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}
</style>
