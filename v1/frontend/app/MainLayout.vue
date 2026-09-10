<template>
  <div class="main-layout">
    <WorkbenchTitleBar
      :is-maximized="isMaximized"
      @minimize="handleMinimize"
      @maximize="handleMaximize"
      @close="handleClose"
    />
    <div class="main-content">
      <router-view />
    </div>
    <WorkbenchStatusBar />
    <SettingsDialog :show="showSettingsDialog" @update:show="showSettingsDialog = $event" />
  </div>
</template>

<script setup lang="ts">
import { getCurrentWindow } from '@tauri-apps/api/window'
import { onMounted, onUnmounted, ref } from 'vue'

import { useProjectStore } from '@/core/project/stores/project'
import SettingsDialog from '@/extensions/builtin/settings/ui/components/SettingsDialog.vue'
import WorkbenchStatusBar from '@/extensions/builtin/workbench/ui/components/WorkbenchStatusBar.vue'
import WorkbenchTitleBar from '@/extensions/builtin/workbench/ui/components/WorkbenchTitleBar.vue'
import {
  WorkbenchEvent,
  listenWorkbenchEvent,
} from '@/extensions/builtin/workbench/ui/constants/workbench-events'

const isMaximized = ref(false)
const showSettingsDialog = ref(false)

const _projectStore = useProjectStore()

const handleMinimize = async () => {
  const window = getCurrentWindow()
  await window.minimize()
}

const handleMaximize = async () => {
  const window = getCurrentWindow()
  await window.toggleMaximize()
  isMaximized.value = !isMaximized.value
}

const handleClose = async () => {
  const window = getCurrentWindow()
  await window.close()
}

const handleOpenSettings = () => {
  showSettingsDialog.value = true
}

let cleanupSettingsListener: (() => void) | null = null

onMounted(() => {
  cleanupSettingsListener = listenWorkbenchEvent(WorkbenchEvent.OpenSettings, handleOpenSettings)
})

onUnmounted(() => {
  cleanupSettingsListener?.()
})
</script>

<style scoped>
.main-layout {
  width: 100vw;
  height: 100vh;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.main-content {
  flex: 1;
  overflow: hidden;
}
</style>
