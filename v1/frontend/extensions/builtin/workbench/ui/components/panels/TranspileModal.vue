<template>
  <NModal :show="visible" preset="card" @update:show="$emit('close')">
    <div class="transpile-modal">
      <div class="transpile-header">
        <h3>{{ $t('sqlEditor.transpileTitle') }}</h3>
        <NButton quaternary circle size="small" @click="$emit('close')">
          <X :size="16" />
        </NButton>
      </div>
      <p class="transpile-hint">{{ $t('sqlEditor.transpileHint') }}</p>
      <div class="transpile-options">
        <NButton
          v-for="opt in dialectOptions"
          :key="opt.key"
          size="large"
          quaternary
          block
          @click="$emit('transpile', opt.key)"
        >
          {{ opt.label }}
        </NButton>
      </div>
    </div>
  </NModal>
</template>

<script setup lang="ts">
import { X } from 'lucide-vue-next'
import { NButton, NModal } from 'naive-ui'

import type { SqlDialect } from '@/shared/types/sql'

interface Props {
  visible: boolean
  dialectOptions: Array<{ label: string; key: SqlDialect }>
}

defineProps<Props>()

interface Emits {
  (e: 'close'): void
  (e: 'transpile', targetDialect: SqlDialect): void
}

defineEmits<Emits>()
</script>

<style scoped>
.transpile-modal {
  width: 360px;
  max-width: 100%;
}

.transpile-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 16px;
}

.transpile-header h3 {
  margin: 0;
  font-size: 16px;
  font-weight: 600;
  color: var(--text-primary, #cccccc);
}

.transpile-hint {
  margin: 0 0 16px;
  font-size: 13px;
  color: var(--text-secondary, #858585);
}

.transpile-options {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
</style>
