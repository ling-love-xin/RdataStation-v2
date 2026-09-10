<template>
  <component :is="iconComponent" v-bind="iconProps" />
</template>

<script setup lang="ts">
import {
  BarChart3,
  BookOpen,
  Check,
  Command,
  Database,
  FileCode,
  History,
  Keyboard,
  LayoutTemplate,
  Monitor,
  Moon,
  Paintbrush,
  Palette,
  PanelBottom,
  PanelTop,
  Puzzle,
  RefreshCw,
  RotateCcw,
  Settings,
  StickyNote,
  Sun,
  Terminal,
  Trash2,
  Wrench,
  Zap,
} from 'lucide-vue-next'
import { computed } from 'vue'

import { useAppStore } from '@/stores/useAppStore'

import type { Component } from 'vue'

const icons: Record<string, Component> = {
  BarChart3,
  BookOpen,
  Check,
  Command,
  Database,
  FileCode,
  History,
  Keyboard,
  LayoutTemplate,
  Monitor,
  Moon,
  Paintbrush,
  Palette,
  PanelBottom,
  PanelTop,
  Puzzle,
  RefreshCw,
  RotateCcw,
  Settings,
  StickyNote,
  Sun,
  Terminal,
  Trash2,
  Wrench,
  Zap,
}

interface Props {
  name: string
  size?: number
  strokeWidth?: number
  accent?: boolean
}

const props = withDefaults(defineProps<Props>(), {
  size: 16,
  strokeWidth: 2,
  accent: false,
})

const appStore = useAppStore()

const iconComponent = computed(() => {
  return icons[props.name] || Database
})

const iconProps = computed(() => {
  const color = props.accent ? '#E17055' : appStore.isDark ? '#E5E7EB' : '#1F2937'
  return {
    size: props.size,
    'stroke-width': props.strokeWidth,
    color,
  }
})
</script>
