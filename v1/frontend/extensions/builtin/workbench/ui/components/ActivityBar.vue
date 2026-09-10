<template>
  <div :class="['activity-bar', position]">
    <div class="activity-bar-icons">
      <div
        v-for="item in items"
        :key="item.id"
        :class="['activity-item', { active: isActive(item.id) }]"
        :title="item.title"
        @click="handleClick(item)"
      >
        <component :is="item.icon" class="activity-icon" :size="22" />
      </div>
    </div>
    <div class="activity-bar-bottom">
      <div
        v-if="showToggle"
        class="activity-item toggle"
        :title="isHidden ? t('workbench.show') : t('workbench.hide')"
        @click="handleToggle"
      >
        <PanelLeftClose v-if="!isHidden" class="activity-icon" :size="20" />
        <PanelLeft v-else class="activity-icon" :size="20" />
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { PanelLeft, PanelLeftClose } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'

import {
  useLayoutStore,
  type LeftActivityItem,
  type RightActivityItem,
} from '../stores/layout-store'

const { t } = useI18n()

interface Props {
  items: LeftActivityItem[] | RightActivityItem[]
  position?: 'left' | 'right'
  showToggle?: boolean
  isHidden?: boolean
}

const props = withDefaults(defineProps<Props>(), {
  items: () => [],
  position: 'left',
  showToggle: true,
  isHidden: false,
})

const layoutStore = useLayoutStore()

function isActive(id: string): boolean {
  if (props.position === 'left') {
    return layoutStore.selectedLeftItem === id
  }
  return layoutStore.selectedRightItem === id
}

function handleClick(item: LeftActivityItem | RightActivityItem) {
  if (props.position === 'left') {
    layoutStore.selectLeftItem(item.id)
  } else {
    layoutStore.selectRightItem(item.id)
  }
}

function handleToggle() {
  if (props.position === 'left') {
    layoutStore.togglePrimarySideBar()
  } else {
    layoutStore.toggleSecondarySideBar()
  }
}
</script>

<style scoped>
.activity-bar {
  display: flex;
  flex-direction: column;
  width: 48px;
  height: 100%;
  background-color: var(--bg-tertiary, #2d2d30);
  border-color: var(--border-color, #3e3e42);
  transition: width 0.2s ease;
}

.activity-bar.left {
  border-right: 1px solid var(--border-color, #3e3e42);
}

.activity-bar.right {
  border-left: 1px solid var(--border-color, #3e3e42);
}

.activity-bar-icons {
  display: flex;
  flex-direction: column;
  padding: 8px 0;
  gap: 2px;
}

.activity-item {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 40px;
  height: 40px;
  margin: 0 auto;
  border-radius: 6px;
  cursor: pointer;
  color: var(--text-tertiary, #666666);
  transition: all 0.15s ease;
}

.activity-item:hover {
  background-color: var(--bg-secondary, #252526);
  color: var(--text-primary, #cccccc);
}

.activity-item.active {
  background-color: var(--primary-color, #165dff);
  color: #ffffff;
}

.activity-item.toggle {
  margin-top: auto;
  margin-bottom: 8px;
}

.activity-icon {
  flex-shrink: 0;
}

.activity-bar-bottom {
  display: flex;
  flex-direction: column;
  margin-top: auto;
  padding-bottom: 4px;
}
</style>
