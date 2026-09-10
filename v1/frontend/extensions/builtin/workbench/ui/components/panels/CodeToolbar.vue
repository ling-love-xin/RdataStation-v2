<template>
  <div class="code-toolbar">
    <div class="toolbar-group">
      <NButton
        size="tiny"
        quaternary
        :type="isDirty ? 'warning' : 'default'"
        title="保存 (Ctrl+S)"
        @click="$emit('save')"
      >
        <template #icon><Save :size="14" /></template>
        保存
      </NButton>
    </div>

    <div class="toolbar-separator" />

    <div class="toolbar-group">
      <NButton size="tiny" quaternary title="格式化文档" @click="$emit('format')">
        <template #icon><AlignLeft :size="14" /></template>
        格式化
      </NButton>
    </div>

    <div class="toolbar-separator" />

    <div class="toolbar-group">
      <NButton size="tiny" quaternary title="撤销 (Ctrl+Z)" @click="$emit('undo')">
        <template #icon><Undo2 :size="14" /></template>
        撤销
      </NButton>
      <NButton size="tiny" quaternary title="重做 (Ctrl+Shift+Z)" @click="$emit('redo')">
        <template #icon><Redo2 :size="14" /></template>
        重做
      </NButton>
    </div>

    <div class="toolbar-spacer" />

    <div class="toolbar-group">
      <NButton size="tiny" quaternary title="查找 (Ctrl+F)" @click="$emit('find')">
        <template #icon><Search :size="14" /></template>
        查找
      </NButton>
    </div>
  </div>
</template>

<script setup lang="ts">
import { Save, AlignLeft, Undo2, Redo2, Search } from 'lucide-vue-next'
import { NButton } from 'naive-ui'

interface Props {
  isDirty: boolean
}

defineProps<Props>()

interface Emits {
  (e: 'save'): void
  (e: 'format'): void
  (e: 'undo'): void
  (e: 'redo'): void
  (e: 'find'): void
}

defineEmits<Emits>()
</script>

<style scoped>
.code-toolbar {
  display: flex;
  align-items: center;
  gap: 2px;
  padding: 4px 8px;
  background: var(--toolbar-bg, #252526);
  border-bottom: 1px solid var(--toolbar-border, #3c3c3c);
  flex-shrink: 0;
  min-height: 32px;
  overflow-x: auto;
}

.toolbar-group {
  display: flex;
  align-items: center;
  gap: 2px;
}

.toolbar-separator {
  width: 1px;
  height: 20px;
  background: var(--toolbar-separator, #555);
  margin: 0 6px;
}

.toolbar-spacer {
  flex: 1;
}
</style>
