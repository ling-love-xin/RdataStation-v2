import { ref, watch } from 'vue'

import type { FilterMode } from '../types/result'

export interface FilterPreset {
  id: string
  name: string
  filterMode: FilterMode
  expression: string
  createdAt: string
}

const STORAGE_KEY = 'rds_filter_presets'

function loadAll(): FilterPreset[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return []
    return JSON.parse(raw) as FilterPreset[]
  } catch {
    return []
  }
}

function saveAll(presets: FilterPreset[]): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(presets))
  } catch {
    // localStorage may be unavailable
  }
}

export function useResultFilterPresets() {
  const presets = ref<FilterPreset[]>(loadAll())

  watch(
    presets,
    val => {
      saveAll(val)
    },
    { deep: true }
  )

  function addPreset(name: string, filterMode: FilterMode, expression: string): FilterPreset {
    const preset: FilterPreset = {
      id: `fpreset_${Date.now()}`,
      name,
      filterMode,
      expression,
      createdAt: new Date().toISOString(),
    }
    presets.value.push(preset)
    return preset
  }

  function removePreset(id: string): void {
    const idx = presets.value.findIndex(p => p.id === id)
    if (idx >= 0) {
      presets.value.splice(idx, 1)
    }
  }

  function updatePreset(id: string, name: string, expression: string): boolean {
    const preset = presets.value.find(p => p.id === id)
    if (!preset) return false
    preset.name = name
    preset.expression = expression
    return true
  }

  function getPreset(id: string): FilterPreset | undefined {
    return presets.value.find(p => p.id === id)
  }

  function getPresetsByMode(mode: FilterMode): FilterPreset[] {
    return presets.value.filter(p => p.filterMode === mode)
  }

  return {
    presets,
    addPreset,
    removePreset,
    updatePreset,
    getPreset,
    getPresetsByMode,
  }
}
