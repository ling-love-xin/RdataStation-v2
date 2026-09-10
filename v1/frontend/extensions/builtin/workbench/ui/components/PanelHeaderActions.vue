<template>
  <div class="panel-header-actions">
    <NTooltip v-if="!isFloating" trigger="hover" placement="bottom">
      <template #trigger>
        <NButton size="tiny" quaternary @click="handleMaximize">
          <Maximize2 v-if="!isMaximized" :size="14" />
          <Minimize2 v-else :size="14" />
        </NButton>
      </template>
      <span>{{ isMaximized ? t('workbench.restore') : t('workbench.maximizeTooltip') }}</span>
    </NTooltip>
    <NTooltip trigger="hover" placement="bottom">
      <template #trigger>
        <NButton size="tiny" quaternary @click="handleFloat">
          <ExternalLink v-if="!isFloating" :size="14" />
          <ArrowLeftToLine v-else :size="14" />
        </NButton>
      </template>
      <span>{{ isFloating ? t('workbench.dockBack') : t('workbench.floatWindow') }}</span>
    </NTooltip>
    <NTooltip trigger="hover" placement="bottom">
      <template #trigger>
        <NButton size="tiny" quaternary :class="{ 'is-pinned': isPinned }" @click="handlePin">
          <Pin v-if="!isPinned" :size="14" />
          <PinOff v-else :size="14" />
        </NButton>
      </template>
      <span>{{ isPinned ? t('workbench.unpin') : t('workbench.pinTooltip') }}</span>
    </NTooltip>
  </div>
</template>

<script setup lang="ts">
import { ExternalLink, Maximize2, Minimize2, ArrowLeftToLine, Pin, PinOff } from 'lucide-vue-next'
import { NButton, NTooltip } from 'naive-ui'
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import { useLayoutStore } from '@/extensions/builtin/workbench/ui/stores/layout-store'

const { t } = useI18n()

interface PanelHeaderParams {
  api?: {
    location?: { type: string }
    moveTo?(options: unknown): void
  }
  panel?: {
    id: string
  }
  group?: {
    api: {
      isMaximized?(): boolean
      maximize(): void
      exitMaximized(): void
      close?(): void
      moveTo?(options: unknown): void
      addFloatingGroup?(group: unknown): void
      addPopoutGroup?(group: unknown): void
      onDidMaximizedChange?(cb: (isMax: boolean) => void): void
    }
    model?: {
      accessor: {
        isMaximized?(): boolean
        addFloatingGroup?(group: unknown): void
      }
    }
  }
}

const p = defineProps<{ params: PanelHeaderParams }>()

const layoutStore = useLayoutStore()

const localMaximized = ref(false)

watch(
  () => p.params,
  params => {
    if (params?.group?.api) {
      const groupApi = params.group.api
      localMaximized.value = groupApi.isMaximized?.() ?? false

      groupApi.onDidMaximizedChange?.((isMax: boolean) => {
        localMaximized.value = isMax
      })
    }
  },
  { immediate: true }
)

function getGroup() {
  return p.params?.group
}

function getAccessor() {
  return p.params?.group?.model?.accessor
}

function getApi() {
  return p.params?.api
}

function getGroupApi() {
  return p.params?.group?.api
}

function getPanelId() {
  return p.params?.panel?.id
}

const isFloating = computed(() => {
  const api = getApi()
  return api?.location?.type === 'floating'
})

const isMaximized = computed(() => {
  return localMaximized.value
})

const isPinned = computed(() => {
  const panelId = getPanelId()
  if (!panelId) return false
  return layoutStore.isPanelPinned(panelId)
})

function handleMaximize() {
  const groupApi = getGroupApi()
  if (isMaximized.value) {
    groupApi?.exitMaximized?.()
  } else {
    groupApi?.maximize?.()
  }
}

function handleFloat() {
  const group = getGroup()
  const accessor = getAccessor()
  const api = getApi()

  if (isFloating.value) {
    api?.moveTo?.({ position: 'center' })
  } else {
    accessor?.addFloatingGroup?.(group)
  }
}

function handlePin() {
  const panelId = getPanelId()
  if (panelId) {
    layoutStore.togglePanelPinned(panelId)
  }
}
</script>

<style scoped>
.is-pinned {
  color: var(--primary-color, #165dff);
}
</style>
