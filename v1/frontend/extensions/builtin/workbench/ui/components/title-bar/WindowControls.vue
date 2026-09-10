<template>
  <div class="window-controls no-drag">
    <button class="window-btn" :title="t('workbench.minimize')" @click="$emit('minimize')">
      <Minus :size="16" />
    </button>
    <button class="window-btn" :title="t('workbench.maximize')" @click="$emit('maximize')">
      <Maximize2 v-if="!isMaximized" :size="14" />
      <Copy v-else :size="14" />
    </button>
    <button class="window-btn close" :title="t('workbench.close')" @click="$emit('close')">
      <X :size="16" />
    </button>
  </div>
</template>

<script setup lang="ts">
import { Minus, Maximize2, Copy, X } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'

interface Props {
  isMaximized?: boolean
}

withDefaults(defineProps<Props>(), {
  isMaximized: false,
})

defineEmits<{
  minimize: []
  maximize: []
  close: []
}>()

const { t } = useI18n()
</script>

<style scoped>
@import './title-bar.css';
</style>
