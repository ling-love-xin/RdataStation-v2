import { defineStore } from 'pinia'
import { ref, computed } from 'vue'

import type { Theme } from '@/stores/config'
import { SIDEBAR_WIDTH_MIN, SIDEBAR_WIDTH_MAX } from '@/stores/config'
import { useAppStore } from '@/stores/useAppStore'

export const useUiStore = defineStore('ui', () => {
  const sidebarCollapsed = ref(false)
  const sidebarWidth = ref(280)
  const showHistoryPanel = ref(false)
  const showConnectionPanel = ref(true)
  const navigatorViewMode = ref<'tree' | 'flat'>('tree')

  const isDark = computed(() => {
    const appStore = useAppStore()
    return appStore.isDark
  })

  const effectiveTheme = computed(() => {
    const appStore = useAppStore()
    return appStore.effectiveTheme
  })

  const theme = computed<Theme>({
    get: () => {
      const appStore = useAppStore()
      return appStore.effectiveTheme
    },
    set: (value: Theme) => {
      const appStore = useAppStore()
      appStore.setTheme(value)
    },
  })

  function setTheme(newTheme: Theme) {
    theme.value = newTheme
    applyTheme()
  }

  async function toggleTheme() {
    const appStore = useAppStore()
    const current = appStore.effectiveTheme
    const next: Theme = current === 'dark' ? 'light' : current === 'light' ? 'system' : 'dark'
    await appStore.setTheme(next)
    applyTheme()
  }

  function applyTheme() {
    const appStore = useAppStore()
    appStore.applyTheme()
  }

  function toggleSidebar() {
    sidebarCollapsed.value = !sidebarCollapsed.value
  }

  function setSidebarWidth(width: number) {
    sidebarWidth.value = Math.max(SIDEBAR_WIDTH_MIN, Math.min(SIDEBAR_WIDTH_MAX, width))
  }

  function toggleHistoryPanel() {
    showHistoryPanel.value = !showHistoryPanel.value
  }

  function toggleConnectionPanel() {
    showConnectionPanel.value = !showConnectionPanel.value
  }

  function setNavigatorViewMode(mode: 'tree' | 'flat') {
    navigatorViewMode.value = mode
  }

  function toggleNavigatorViewMode() {
    navigatorViewMode.value = navigatorViewMode.value === 'tree' ? 'flat' : 'tree'
  }

  return {
    theme,
    sidebarCollapsed,
    sidebarWidth,
    showHistoryPanel,
    showConnectionPanel,
    isDark,
    effectiveTheme,
    setTheme,
    toggleTheme,
    toggleSidebar,
    setSidebarWidth,
    toggleHistoryPanel,
    toggleConnectionPanel,
    navigatorViewMode,
    setNavigatorViewMode,
    toggleNavigatorViewMode,
    applyTheme,
  }
})
