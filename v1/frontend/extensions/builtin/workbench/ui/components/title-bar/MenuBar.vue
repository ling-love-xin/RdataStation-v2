<template>
  <div ref="menuBarRef" class="menu-bar-wrapper">
    <!-- 汉堡菜单按钮 -->
    <button
      class="icon-btn hamburger-btn no-drag"
      :class="{ active: showMenuBar }"
      :title="t('workbench.menu')"
      :aria-expanded="showMenuBar"
      aria-haspopup="true"
      @click="toggleMenuBar"
    >
      <Menu :size="16" />
    </button>

    <!-- 可展开的菜单栏 -->
    <Transition name="menu-slide">
      <div v-if="showMenuBar" class="menu-bar" role="menubar" :aria-label="t('workbench.menu')">
        <div
          v-for="menu in menus"
          :key="menu.id"
          ref="menuItemRefs"
          class="menu-item"
          :class="{ active: activeMenu === menu.id }"
          role="menuitem"
          :aria-expanded="activeMenu === menu.id"
          :aria-haspopup="true"
          tabindex="0"
          @click="handleMenuClick(menu)"
          @keydown.enter.prevent="handleMenuClick(menu)"
        >
          {{ menu.label }}
        </div>
      </div>
    </Transition>

    <!-- 菜单下拉面板 -->
    <Transition name="dropdown">
      <div
        v-if="activeMenu && activeMenuData"
        ref="dropdownRef"
        class="dropdown-panel menu-dropdown"
        role="menu"
        :aria-label="activeMenuData.label"
        :style="dropdownPosition"
      >
        <template v-for="(item, index) in activeMenuData.items" :key="item.id || index">
          <div v-if="item.separator" class="dropdown-divider" role="separator" />
          <div
            v-else
            class="dropdown-item"
            :class="{ disabled: item.disabled }"
            role="menuitem"
            :aria-disabled="item.disabled || false"
            tabindex="0"
            @click="handleMenuItemClick(item)"
            @keydown.enter.prevent="handleMenuItemClick(item)"
          >
            <component :is="item.icon" v-if="item.icon" :size="14" aria-hidden="true" />
            <span>{{ item.label }}</span>
            <span v-if="item.shortcut" class="shortcut">{{ item.shortcut }}</span>
          </div>
        </template>
      </div>
    </Transition>
  </div>
</template>

<script setup lang="ts">
import { Menu } from 'lucide-vue-next'
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import type { MenuConfig, MenuItem } from './title-bar-types'

export type { MenuConfig, MenuItem }

interface Props {
  menus: MenuConfig[]
}

const props = defineProps<Props>()

const emit = defineEmits<{
  (e: 'menu-action', item: MenuItem): void
}>()

const { t } = useI18n()

const showMenuBar = ref(false)
const activeMenu = ref<string | null>(null)
const menuBarRef = ref<HTMLElement | null>(null)
const dropdownRef = ref<HTMLElement | null>(null)
const menuItemRefs = ref<HTMLElement[]>([])
const activeMenuIndex = ref(-1)

const activeMenuData = computed(() => {
  if (!activeMenu.value) return null
  return props.menus.find(m => m.id === activeMenu.value) || null
})

// 缓存下拉面板位置，避免每次渲染都计算
const dropdownPosition = ref({ left: '0px', top: 'calc(100% + 4px)' })

function updateDropdownPosition() {
  if (activeMenuIndex.value < 0 || !menuItemRefs.value[activeMenuIndex.value]) {
    dropdownPosition.value = { left: '0px', top: 'calc(100% + 4px)' }
    return
  }
  const rect = menuItemRefs.value[activeMenuIndex.value].getBoundingClientRect()
  dropdownPosition.value = {
    left: `${rect.left}px`,
    top: `${rect.bottom + 4}px`,
  }
}

watch(activeMenuIndex, () => {
  nextTick(updateDropdownPosition)
})

function toggleMenuBar() {
  showMenuBar.value = !showMenuBar.value
  if (!showMenuBar.value) {
    activeMenu.value = null
    activeMenuIndex.value = -1
  }
}

function handleMenuClick(menu: MenuConfig) {
  const index = props.menus.findIndex(m => m.id === menu.id)
  if (activeMenu.value === menu.id) {
    activeMenu.value = null
    activeMenuIndex.value = -1
  } else {
    activeMenu.value = menu.id
    activeMenuIndex.value = index
  }
}

function handleMenuItemClick(item: MenuItem) {
  if (item.disabled) return
  if (item.action) {
    item.action()
  }
  emit('menu-action', item)
  closeAllMenus()
}

function closeAllMenus() {
  activeMenu.value = null
  activeMenuIndex.value = -1
}

function handleClickOutside(event: MouseEvent) {
  const target = event.target as HTMLElement
  const insideMenuBar = menuBarRef.value && menuBarRef.value.contains(target)
  const insideDropdown = dropdownRef.value && dropdownRef.value.contains(target)
  if (!insideMenuBar && !insideDropdown) {
    closeAllMenus()
  }
}

function handleKeyDown(event: KeyboardEvent) {
  if (event.key === 'Escape') {
    closeAllMenus()
    return
  }

  if (event.altKey && !event.ctrlKey && !event.shiftKey) {
    const key = event.key.toLowerCase()
    const menuMap: Record<string, string> = {
      f: 'file',
      e: 'edit',
      v: 'view',
      c: 'connection',
      r: 'run',
      t: 'tools',
      h: 'help',
    }
    const menuId = menuMap[key]
    if (menuId) {
      event.preventDefault()
      const menu = props.menus.find(m => m.id === menuId)
      if (menu) {
        showMenuBar.value = true
        handleMenuClick(menu)
      }
    }
  }
}

onMounted(() => {
  document.addEventListener('click', handleClickOutside)
  document.addEventListener('keydown', handleKeyDown)
})

onUnmounted(() => {
  document.removeEventListener('click', handleClickOutside)
  document.removeEventListener('keydown', handleKeyDown)
})
</script>

<style scoped>
@import './title-bar.css';

.menu-bar-wrapper {
  display: flex;
  align-items: center;
  height: 100%;
  gap: var(--spacing-xs);
}

.menu-dropdown {
  position: fixed;
  z-index: 1001;
}

.dropdown-item.disabled {
  opacity: 0.4;
  cursor: not-allowed;
}
</style>
