import { defineStore } from 'pinia'
import { computed, ref } from 'vue'

import { useAppStore } from '@/stores/useAppStore'

export interface Command {
  id: string
  label: string
  category: string
  icon?: string
  shortcut?: string
  disabled?: boolean
  action: () => void
}

export const useCommandStore = defineStore('commands', () => {
  // State - 使用数组替代 Map，更好的 Vue 3 响应式支持
  const commands = ref<Command[]>([])
  const recentCommands = ref<string[]>([])

  // 从配置系统读取命令面板设置
  const appStore = useAppStore()
  const paletteSettings = computed(() => appStore.effectiveCommandPaletteSettings)
  const maxRecent = computed(() => paletteSettings.value.maxRecentCommands)

  // Getters
  const allCommands = computed(() => commands.value)

  const commandsByCategory = computed(() => {
    const grouped = new Map<string, Command[]>()
    allCommands.value.forEach(cmd => {
      if (!grouped.has(cmd.category)) {
        grouped.set(cmd.category, [])
      }
      const entry = grouped.get(cmd.category)
      if (entry) {
        entry.push(cmd)
      }
    })
    return grouped
  })

  const recentCommandList = computed(() => {
    return recentCommands.value
      .map(id => commands.value.find(cmd => cmd.id === id))
      .filter((cmd): cmd is Command => cmd !== undefined)
  })

  // Actions
  function register(command: Command) {
    const index = commands.value.findIndex(cmd => cmd.id === command.id)
    if (index >= 0) {
      commands.value[index] = command
    } else {
      commands.value.push(command)
    }
  }

  function unregister(commandId: string) {
    const index = commands.value.findIndex(cmd => cmd.id === commandId)
    if (index >= 0) {
      commands.value.splice(index, 1)
    }
  }

  function execute(commandId: string) {
    const cmd = commands.value.find(cmd => cmd.id === commandId)
    if (cmd) {
      try {
        cmd.action()
        addToRecent(commandId)
      } catch (error) {
        console.error(`[CommandStore] Command "${commandId}" execution failed:`, error)
      }
    }
  }

  function addToRecent(commandId: string) {
    recentCommands.value = [
      commandId,
      ...recentCommands.value.filter(id => id !== commandId),
    ].slice(0, maxRecent.value)
  }

  function search(query: string): Command[] {
    if (!query.trim()) {
      return recentCommandList.value.length > 0
        ? recentCommandList.value
        : allCommands.value.slice(0, 10)
    }

    const terms = query.toLowerCase().split(/\s+/)
    const includeDisabled = paletteSettings.value.includeDisabledCommands

    return allCommands.value
      .filter(cmd => {
        if (!includeDisabled && cmd.disabled) return false
        const text = `${cmd.label} ${cmd.category}`.toLowerCase()
        return terms.every(term => text.includes(term))
      })
      .sort((a, b) => {
        const aLabel = a.label.toLowerCase()
        const bLabel = b.label.toLowerCase()
        const aStartsWith = aLabel.startsWith(query.toLowerCase())
        const bStartsWith = bLabel.startsWith(query.toLowerCase())
        if (aStartsWith && !bStartsWith) return -1
        if (!aStartsWith && bStartsWith) return 1
        return a.label.localeCompare(b.label)
      })
  }

  return {
    commands,
    recentCommands,
    allCommands,
    commandsByCategory,
    recentCommandList,
    register,
    unregister,
    execute,
    search,
  }
})
