<template>
  <div class="command-center">
    <button
      class="command-btn"
      :title="t('workbench.search') + ' (' + t('workbench.searchShortcut') + ')'"
      @click="$emit('open')"
    >
      <Search :size="14" />
      <span class="command-text">{{ t('workbench.search') }}</span>
      <span class="shortcut">{{ t('workbench.searchShortcut') }}</span>
    </button>
  </div>
</template>

<script setup lang="ts">
import { Search } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'

defineEmits<{
  open: []
}>()

const { t } = useI18n()
</script>

<style scoped>
@import './title-bar.css';
</style>
