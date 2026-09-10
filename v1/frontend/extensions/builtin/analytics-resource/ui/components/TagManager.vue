<template>
  <div class="tag-manager">
    <div class="tag-list">
      <button
        :class="['tag-chip', { active: activeTagId === null }]"
        @click="handleSelectTag(null)"
      >
        {{ t('analyticsResource.allTags') }}
      </button>
      <button
        v-for="tag in tags"
        :key="tag.id"
        :class="['tag-chip', { active: activeTagId === tag.id }]"
        :style="
          activeTagId === tag.id
            ? {
                background: tag.color || 'var(--resource-tag-default)',
                borderColor: tag.color || 'var(--resource-tag-default)',
              }
            : {}
        "
        @click="handleSelectTag(tag.id)"
      >
        <span class="tag-dot" :style="{ background: tag.color || 'var(--resource-tag-default)' }" />
        {{ tag.name }}
      </button>
    </div>
    <button
      class="add-tag-btn"
      :title="t('analyticsResource.createTag')"
      @click="$emit('create-tag')"
    >
      + {{ t('analyticsResource.newTag') }}
    </button>
  </div>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'

import type { AnalyticsTag } from '../../types'

const { t } = useI18n()

defineProps<{
  tags: AnalyticsTag[]
  activeTagId: string | null
}>()

const emit = defineEmits<{
  'update:activeTagId': [tagId: string | null]
  'create-tag': []
}>()

function handleSelectTag(tagId: string | null) {
  emit('update:activeTagId', tagId)
}
</script>

<style scoped>
.tag-manager {
  display: flex;
  align-items: center;
  gap: var(--size-sm);
  padding: var(--size-md) var(--size-lg);
  border-bottom: 1px solid var(--border-color-subtle, var(--border-color));
  overflow-x: auto;
}

.tag-list {
  display: flex;
  gap: var(--spacing-sm);
  flex: 1;
  overflow-x: auto;
  padding-bottom: 2px;
}

.tag-chip {
  display: flex;
  align-items: center;
  gap: 5px;
  padding: 4px 10px;
  border: 1px solid var(--border-color);
  border-radius: var(--border-radius-pill);
  background: var(--bg-secondary);
  color: var(--text-secondary);
  font-size: var(--font-size-sm);
  cursor: pointer;
  transition: all 0.2s;
  white-space: nowrap;
  flex-shrink: 0;
}

.tag-chip:hover {
  border-color: var(--primary-color);
  color: var(--primary-color);
}

.tag-chip.active {
  background: var(--primary-light, rgba(22, 93, 255, 0.1));
  border-color: var(--primary-color);
  color: var(--primary-color);
  font-weight: 500;
}

.tag-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
}

.add-tag-btn {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
  padding: 4px 10px;
  border: 1px dashed var(--border-color);
  border-radius: var(--border-radius-pill);
  background: transparent;
  color: var(--text-tertiary);
  font-size: var(--font-size-sm);
  cursor: pointer;
  transition: all 0.2s;
  white-space: nowrap;
  flex-shrink: 0;
}

.add-tag-btn:hover {
  border-color: var(--primary-color);
  color: var(--primary-color);
}
</style>
