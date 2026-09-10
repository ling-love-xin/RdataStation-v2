import { ref, watch, onMounted } from 'vue'

export type ThemeMode = 'light' | 'dark' | 'system'

export interface ThemeConfig {
  mode: ThemeMode
  accentColor: string
  fontSize: 'small' | 'medium' | 'large'
  compactMode: boolean
}

const defaultConfig: ThemeConfig = {
  mode: 'system',
  accentColor: '#00b464',
  fontSize: 'medium',
  compactMode: false,
}

const THEME_STORAGE_KEY = 'rdata-station-theme-config'

export function useThemeManager() {
  const config = ref<ThemeConfig>({ ...defaultConfig })
  const isDark = ref(false)

  function loadConfig() {
    try {
      const saved = localStorage.getItem(THEME_STORAGE_KEY)
      if (saved) {
        const parsed = JSON.parse(saved)
        config.value = { ...defaultConfig, ...parsed }
      }
    } catch {
      config.value = { ...defaultConfig }
    }
  }

  function saveConfig() {
    localStorage.setItem(THEME_STORAGE_KEY, JSON.stringify(config.value))
  }

  function getSystemTheme(): 'light' | 'dark' {
    if (typeof window !== 'undefined' && window.matchMedia) {
      return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
    }
    return 'light'
  }

  function updateIsDark() {
    if (config.value.mode === 'system') {
      isDark.value = getSystemTheme() === 'dark'
    } else {
      isDark.value = config.value.mode === 'dark'
    }
  }

  function setMode(mode: ThemeMode) {
    config.value.mode = mode
    updateIsDark()
    applyTheme()
    saveConfig()
  }

  function setAccentColor(color: string) {
    config.value.accentColor = color
    applyTheme()
    saveConfig()
  }

  function setFontSize(size: 'small' | 'medium' | 'large') {
    config.value.fontSize = size
    applyTheme()
    saveConfig()
  }

  function toggleCompactMode() {
    config.value.compactMode = !config.value.compactMode
    applyTheme()
    saveConfig()
  }

  function applyTheme() {
    const root = document.documentElement

    if (isDark.value) {
      root.classList.add('dark')
      root.classList.remove('light')
    } else {
      root.classList.add('light')
      root.classList.remove('dark')
    }

    root.style.setProperty('--primary-color', config.value.accentColor)

    const fontSizeMap = {
      small: '12px',
      medium: '13px',
      large: '14px',
    }
    root.style.setProperty('--base-font-size', fontSizeMap[config.value.fontSize])

    root.style.setProperty('--compact-mode', config.value.compactMode ? 'true' : 'false')
  }

  function subscribeToSystemTheme() {
    if (typeof window !== 'undefined' && window.matchMedia) {
      const mediaQuery = window.matchMedia('(prefers-color-scheme: dark)')
      mediaQuery.addEventListener('change', () => {
        if (config.value.mode === 'system') {
          updateIsDark()
          applyTheme()
        }
      })
    }
  }

  watch(
    config,
    () => {
      saveConfig()
    },
    { deep: true }
  )

  onMounted(() => {
    loadConfig()
    updateIsDark()
    applyTheme()
    subscribeToSystemTheme()
  })

  return {
    config,
    isDark,
    setMode,
    setAccentColor,
    setFontSize,
    toggleCompactMode,
    availableThemes: [
      { id: 'light', label: '浅色', icon: 'Sun' },
      { id: 'dark', label: '深色', icon: 'Moon' },
      { id: 'system', label: '跟随系统', icon: 'Monitor' },
    ] as const,
    availableFontSizes: [
      { id: 'small', label: '小' },
      { id: 'medium', label: '中' },
      { id: 'large', label: '大' },
    ] as const,
    accentColors: ['#00b464', '#6464ff', '#ff6464', '#ffb400', '#9664ff', '#00c853'],
  }
}
