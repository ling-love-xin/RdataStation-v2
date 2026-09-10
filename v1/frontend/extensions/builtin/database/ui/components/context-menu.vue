<template>
  <div v-if="visible" class="context-menu" :style="positionStyle">
    <NMenu :options="menuOptions" @select="handleSelect" />
  </div>
</template>

<script setup lang="ts">
import { NMenu } from 'naive-ui'
import { computed } from 'vue'

import type { MenuOption } from 'naive-ui'

interface Props {
  visible: boolean
  x: number
  y: number
  options?: MenuOption[]
}

const props = withDefaults(defineProps<Props>(), {
  options: () => [],
})

const emit = defineEmits<{
  select: [key: string]
  close: []
}>()

const positionStyle = computed(() => ({
  left: `${props.x}px`,
  top: `${props.y}px`,
}))

const menuOptions = computed(() => props.options)

const handleSelect = (key: string) => {
  emit('select', key)
  emit('close')
}
</script>

<style scoped>
.context-menu {
  position: fixed;
  z-index: 1000;
  background: var(--context-menu-bg, #fff);
  border: 1px solid var(--border-color, #ddd);
  border-radius: 4px;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.15);
  min-width: 160px;
}
</style>
