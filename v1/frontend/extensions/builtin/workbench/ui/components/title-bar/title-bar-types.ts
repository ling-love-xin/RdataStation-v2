import type { Component } from 'vue'

export interface MenuItem {
  id: string
  label?: string
  icon?: Component
  shortcut?: string
  disabled?: boolean
  separator?: boolean
  action?: () => void
}

export interface MenuConfig {
  id: string
  label: string
  items: MenuItem[]
}

export interface ToolbarTool {
  id: string
  name: string
  icon: Component
  enabled: boolean
  action: () => void
}
