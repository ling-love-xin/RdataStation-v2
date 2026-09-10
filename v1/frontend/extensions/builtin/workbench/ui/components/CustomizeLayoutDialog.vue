<template>
  <Teleport to="body">
    <Transition name="modal">
      <div v-if="visible" class="modal-overlay" @click.self="handleClose">
        <div class="modal-container">
          <header class="modal-header">
            <h2 class="header-title">{{ t('workbench.customizeLayout') }}</h2>
            <button class="btn-close" @click="handleClose">
              <X :size="18" />
            </button>
          </header>

          <div class="modal-body">
            <!-- 布局结构可视化预览 -->
            <div class="preview-section">
              <span class="section-title">{{
                t('workbench.customizeLayoutDialog.layoutPreview')
              }}</span>
              <div class="layout-preview">
                <div
                  class="preview-col preview-left"
                  :class="{ collapsed: layoutStore.leftEdgeGroupCollapsed }"
                >
                  <span class="preview-label">{{
                    t('workbench.customizeLayoutDialog.columnA')
                  }}</span>
                  <span class="preview-width">{{ layoutStore.primarySideBarWidth }}px</span>
                </div>
                <div class="preview-col preview-center">
                  <span class="preview-label">{{
                    t('workbench.customizeLayoutDialog.columnB')
                  }}</span>
                  <span class="preview-width">auto</span>
                </div>
                <div
                  class="preview-col preview-right"
                  :class="{ collapsed: layoutStore.rightEdgeGroupCollapsed }"
                >
                  <span class="preview-label">{{
                    t('workbench.customizeLayoutDialog.columnC')
                  }}</span>
                  <span class="preview-width">{{ layoutStore.secondarySideBarWidth }}px</span>
                </div>
              </div>
              <div class="preview-chrome">
                <div class="chrome-indicator" :class="{ active: layoutStore.menuBarVisible }">
                  {{ t('workbench.customizeLayoutDialog.menuBar') }}
                </div>
                <div class="chrome-indicator" :class="{ active: layoutStore.statusBarVisible }">
                  {{ t('workbench.customizeLayoutDialog.statusBar') }}
                </div>
              </div>
            </div>

            <!-- Edge Group 控制 -->
            <div class="edge-groups-section">
              <div class="edge-group-card">
                <div class="card-row">
                  <span class="card-label"
                    >A {{ t('workbench.customizeLayoutDialog.column') }}</span
                  >
                  <NSwitch
                    :value="!layoutStore.leftEdgeGroupCollapsed"
                    size="small"
                    @update:value="handleLeftEdgeToggle"
                  />
                  <span class="switch-label">{{
                    layoutStore.leftEdgeGroupCollapsed
                      ? t('workbench.customizeLayoutDialog.hidden')
                      : t('workbench.customizeLayoutDialog.visible')
                  }}</span>
                </div>
                <div v-if="!layoutStore.leftEdgeGroupCollapsed" class="card-row">
                  <span class="card-label width-label">{{
                    t('workbench.customizeLayoutDialog.width')
                  }}</span>
                  <NInputNumber
                    :value="layoutStore.primarySideBarWidth"
                    :min="MIN_WIDTH"
                    :max="MAX_WIDTH"
                    :step="10"
                    size="small"
                    style="width: 100px"
                    @update:value="handleLeftWidthChange"
                  />
                  <span class="unit-label">px</span>
                </div>
              </div>

              <div class="edge-group-card">
                <div class="card-row">
                  <span class="card-label"
                    >C {{ t('workbench.customizeLayoutDialog.column') }}</span
                  >
                  <NSwitch
                    :value="!layoutStore.rightEdgeGroupCollapsed"
                    size="small"
                    @update:value="handleRightEdgeToggle"
                  />
                  <span class="switch-label">{{
                    layoutStore.rightEdgeGroupCollapsed
                      ? t('workbench.customizeLayoutDialog.hidden')
                      : t('workbench.customizeLayoutDialog.visible')
                  }}</span>
                </div>
                <div v-if="!layoutStore.rightEdgeGroupCollapsed" class="card-row">
                  <span class="card-label width-label">{{
                    t('workbench.customizeLayoutDialog.width')
                  }}</span>
                  <NInputNumber
                    :value="layoutStore.secondarySideBarWidth"
                    :min="MIN_WIDTH"
                    :max="MAX_WIDTH"
                    :step="10"
                    size="small"
                    style="width: 100px"
                    @update:value="handleRightWidthChange"
                  />
                  <span class="unit-label">px</span>
                </div>
              </div>
            </div>

            <!-- 布局模板快速选择器 -->
            <div class="templates-section">
              <span class="section-title">{{
                t('workbench.customizeLayoutDialog.layoutTemplates')
              }}</span>
              <div class="template-chips">
                <button
                  v-for="preset in presetOptions"
                  :key="preset.key"
                  class="template-chip"
                  :class="{ active: selectedPreset === preset.key }"
                  @click="handleSelectPreset(preset.key)"
                >
                  {{ preset.label }}
                </button>
              </div>
              <div class="template-actions">
                <NInput
                  v-model:value="newTemplateName"
                  size="small"
                  :placeholder="t('workbench.customizeLayoutDialog.templateNamePlaceholder')"
                  style="flex: 1"
                  @keyup.enter="handleSaveTemplate"
                />
                <NButton size="small" :disabled="!newTemplateName" @click="handleSaveTemplate">
                  {{ t('workbench.customizeLayoutDialog.saveTemplate') }}
                </NButton>
              </div>
              <div v-if="layoutStore.customTemplates.length > 0" class="saved-templates">
                <div
                  v-for="tmpl in layoutStore.customTemplates"
                  :key="tmpl.name"
                  class="saved-template-row"
                >
                  <button class="saved-template-name" @click="handleApplyCustomTemplate(tmpl)">
                    {{ tmpl.name }}
                  </button>
                  <button
                    class="saved-template-delete"
                    :title="t('workbench.customizeLayoutDialog.deleteTemplate')"
                    @click="handleDeleteTemplate(tmpl.name)"
                  >
                    <Trash2 :size="14" />
                  </button>
                </div>
              </div>
            </div>
          </div>

          <!-- 底部操作栏 -->
          <footer class="modal-footer">
            <div class="footer-left">
              <NButton size="small" :disabled="!layoutStore.canUndo" @click="layoutStore.undo()">
                <template #icon><Undo2 :size="14" /></template>
                {{ t('workbench.customizeLayoutDialog.undo') }}
              </NButton>
              <NButton size="small" :disabled="!layoutStore.canRedo" @click="layoutStore.redo()">
                <template #icon><Redo2 :size="14" /></template>
                {{ t('workbench.customizeLayoutDialog.redo') }}
              </NButton>
            </div>
            <div class="footer-right">
              <NButton size="small" @click="handleReset">
                {{ t('workbench.customizeLayoutDialog.resetDefault') }}
              </NButton>
              <NButton type="primary" size="small" @click="handleClose">
                {{ t('workbench.customizeLayoutDialog.done') }}
              </NButton>
            </div>
          </footer>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<script setup lang="ts">
import { X, Undo2, Redo2, Trash2 } from 'lucide-vue-next'
import { NButton, NInputNumber, NSwitch, NInput } from 'naive-ui'
import { ref, computed, onMounted, onUnmounted, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import {
  useLayoutStore,
  type LayoutTemplate,
} from '@/extensions/builtin/workbench/ui/stores/layout-store'

const { t } = useI18n()
const layoutStore = useLayoutStore()

const MIN_WIDTH = 200
const MAX_WIDTH = 600

const visible = computed(() => layoutStore.showCustomizeLayoutDialog)
const selectedPreset = ref<string | null>(null)
const newTemplateName = ref('')
const dialogOpened = ref(false)

interface PresetOption {
  key: string
  label: string
}

const presetOptions = computed<PresetOption[]>(() => [
  { key: 'default', label: t('workbench.customizeLayoutDialog.presetDefault') },
  { key: 'compact', label: t('workbench.customizeLayoutDialog.presetCompact') },
  { key: 'analysis', label: t('workbench.customizeLayoutDialog.presetAnalysis') },
])

watch(visible, val => {
  if (val && !dialogOpened.value) {
    dialogOpened.value = true
    layoutStore.loadCustomTemplates()
    selectedPreset.value = null
    newTemplateName.value = ''
  }
  if (!val) {
    dialogOpened.value = false
  }
})

function handleLeftEdgeToggle(value: boolean) {
  if (value) {
    layoutStore.expandLeftEdgeGroup()
  } else {
    layoutStore.collapseLeftEdgeGroup()
  }
}

function handleRightEdgeToggle(value: boolean) {
  if (value) {
    layoutStore.expandRightEdgeGroup()
  } else {
    layoutStore.collapseRightEdgeGroup()
  }
}

function handleLeftWidthChange(value: number | null) {
  if (value !== null && value >= MIN_WIDTH && value <= MAX_WIDTH) {
    layoutStore.setPrimarySideBarWidth(value)
    const api = layoutStore.dockviewApi
    if (api) {
      api.getEdgeGroup('left')?.setSize({ width: value })
    }
  }
}

function handleRightWidthChange(value: number | null) {
  if (value !== null && value >= MIN_WIDTH && value <= MAX_WIDTH) {
    layoutStore.setSecondarySideBarWidth(value)
    const api = layoutStore.dockviewApi
    if (api) {
      api.getEdgeGroup('right')?.setSize({ width: value })
    }
  }
}

function handleSelectPreset(key: string) {
  selectedPreset.value = key
  if (key === 'compact') {
    layoutStore.collapseLeftEdgeGroup()
    layoutStore.collapseRightEdgeGroup()
  } else if (key === 'analysis') {
    layoutStore.expandLeftEdgeGroup()
    layoutStore.expandRightEdgeGroup()
    const api = layoutStore.dockviewApi
    if (api) {
      api.getEdgeGroup('right')?.setSize({ width: 360 })
      layoutStore.setSecondarySideBarWidth(360)
    }
  } else {
    layoutStore.expandLeftEdgeGroup()
    layoutStore.expandRightEdgeGroup()
    const api = layoutStore.dockviewApi
    if (api) {
      api.getEdgeGroup('left')?.setSize({ width: 300 })
      api.getEdgeGroup('right')?.setSize({ width: 300 })
    }
    layoutStore.panelHeight = 250
    layoutStore.setPanelHeight(250)
  }
  layoutStore.menuBarVisible = true
  layoutStore.statusBarVisible = true
}

function handleSaveTemplate() {
  const name = newTemplateName.value.trim()
  if (!name) return
  layoutStore.saveCustomTemplate(name)
  newTemplateName.value = ''
}

function handleApplyCustomTemplate(tmpl: LayoutTemplate) {
  layoutStore.applyTemplate(tmpl)
  selectedPreset.value = null
}

function handleDeleteTemplate(name: string) {
  layoutStore.deleteCustomTemplate(name)
}

function handleReset() {
  layoutStore.expandLeftEdgeGroup()
  layoutStore.expandRightEdgeGroup()
  const api = layoutStore.dockviewApi
  if (api) {
    api.getEdgeGroup('left')?.setSize({ width: 300 })
    api.getEdgeGroup('right')?.setSize({ width: 300 })
  }
  layoutStore.setPrimarySideBarWidth(300)
  layoutStore.setSecondarySideBarWidth(300)
  layoutStore.menuBarVisible = true
  layoutStore.statusBarVisible = true
  selectedPreset.value = null
}

function handleClose() {
  layoutStore.pushSnapshot()
  layoutStore.closeCustomizeLayoutDialog()
}

function handleKeydown(e: KeyboardEvent) {
  if (!visible.value) return
  if (e.key === 'Escape') {
    handleClose()
  } else if (e.ctrlKey && e.key === 'z') {
    e.preventDefault()
    if (layoutStore.canUndo) layoutStore.undo()
  } else if (e.ctrlKey && (e.key === 'y' || (e.shiftKey && e.key === 'z'))) {
    e.preventDefault()
    if (layoutStore.canRedo) layoutStore.redo()
  }
}

onMounted(() => {
  window.addEventListener('keydown', handleKeydown)
})

onUnmounted(() => {
  window.removeEventListener('keydown', handleKeydown)
})
</script>

<style scoped>
.modal-overlay {
  position: fixed;
  inset: 0;
  background: var(--modal-overlay-bg);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 10000;
  backdrop-filter: blur(var(--spacing-xs));
}

.modal-container {
  width: 560px;
  max-width: 90vw;
  max-height: 85vh;
  background: var(--color-bg-primary);
  border-radius: var(--border-radius-md);
  box-shadow: var(--shadow-lg);
  display: flex;
  flex-direction: column;
  overflow: hidden;
  font-family: var(--font-sans);
}

.modal-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--spacing-md) var(--spacing-lg);
  border-bottom: 1px solid var(--color-border-subtle);
  background: var(--color-bg-secondary);
}

.header-title {
  font-size: var(--font-size-xl);
  font-weight: 500;
  line-height: 1.6;
  color: var(--color-text-primary);
  margin: 0;
}

.btn-close {
  width: 28px;
  height: 28px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  border: none;
  border-radius: var(--border-radius-sm);
  color: var(--color-text-secondary);
  cursor: pointer;
  transition:
    background 0.15s ease,
    color 0.15s ease;
}

.btn-close:hover {
  background: var(--color-hover);
  color: var(--color-text-primary);
}

.modal-body {
  flex: 1;
  overflow-y: auto;
  padding: var(--spacing-lg);
  display: flex;
  flex-direction: column;
  gap: var(--spacing-lg);
  background: var(--color-bg-primary);
}

/* 布局预览 */
.preview-section {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-sm);
}

.layout-preview {
  display: flex;
  gap: var(--spacing-xs);
  height: 60px;
}

.preview-col {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  border-radius: var(--border-radius-sm);
  border: 1px solid var(--color-border-subtle);
  font-size: var(--font-size-sm);
  gap: var(--spacing-xs);
  transition: all 0.25s ease;
}

.preview-left {
  background: var(--primary-lighter);
  flex: 1;
}

.preview-center {
  background: var(--color-bg-secondary);
  flex: 2;
  border-color: var(--primary-soft);
}

.preview-right {
  background: var(--primary-lighter);
  flex: 1;
}

.preview-col.collapsed {
  flex: 0 0 48px;
  min-width: 48px;
  opacity: 0.5;
  font-size: var(--font-size-xs);
}

.preview-label {
  font-weight: 500;
  color: var(--color-text-primary);
}

.preview-width {
  font-size: var(--font-size-xs);
  color: var(--color-text-muted);
}

.preview-chrome {
  display: flex;
  gap: var(--spacing-xs);
}

.chrome-indicator {
  flex: 1;
  height: 22px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: var(--font-size-xs);
  border-radius: var(--border-radius-sm);
  background: var(--color-bg-secondary);
  border: 1px solid var(--color-border-subtle);
  color: var(--color-text-muted);
  transition: all 0.2s;
}

.chrome-indicator.active {
  background: var(--primary-lighter);
  border-color: var(--primary-soft);
  color: var(--color-text-primary);
}

/* Edge Groups */
.edge-groups-section {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-sm);
}

.edge-group-card {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-sm);
  padding: var(--spacing-md);
  background: var(--color-bg-secondary);
  border-radius: var(--border-radius-sm);
  border: 1px solid var(--color-border-subtle);
}

.card-row {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
}

.card-label {
  font-size: var(--font-size-lg);
  color: var(--color-text-primary);
  min-width: 48px;
  font-weight: 500;
}

.width-label {
  font-size: var(--font-size-md);
  color: var(--color-text-secondary);
}

.switch-label {
  font-size: var(--font-size-sm);
  color: var(--color-text-muted);
}

.unit-label {
  font-size: var(--font-size-sm);
  color: var(--color-text-muted);
}

/* 模板选择 */
.templates-section {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-sm);
}

.template-chips {
  display: flex;
  gap: var(--spacing-sm);
  flex-wrap: wrap;
}

.template-chip {
  padding: var(--spacing-xs) var(--spacing-md);
  font-size: var(--font-size-md);
  border-radius: var(--border-radius-sm);
  border: 1px solid var(--color-border);
  background: var(--color-bg-secondary);
  color: var(--color-text-secondary);
  cursor: pointer;
  transition: all 0.2s;
  white-space: nowrap;
}

.template-chip:hover {
  border-color: var(--color-text-muted);
  color: var(--color-text-primary);
}

.template-chip.active {
  background: var(--primary-soft);
  border-color: var(--primary-color);
  color: var(--color-text-primary);
}

.template-actions {
  display: flex;
  gap: var(--spacing-sm);
  align-items: center;
}

.saved-templates {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-xs);
  margin-top: var(--spacing-xs);
}

.saved-template-row {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
}

.saved-template-name {
  flex: 1;
  text-align: left;
  padding: var(--spacing-xs) var(--spacing-sm);
  font-size: var(--font-size-md);
  border-radius: var(--border-radius-sm);
  background: transparent;
  border: 1px solid transparent;
  color: var(--color-text-secondary);
  cursor: pointer;
  transition: all 0.2s;
}

.saved-template-name:hover {
  background: var(--color-bg-secondary);
  border-color: var(--color-border-subtle);
  color: var(--color-text-primary);
}

.saved-template-delete {
  width: 24px;
  height: 24px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  border: none;
  border-radius: var(--border-radius-sm);
  color: var(--color-text-muted);
  cursor: pointer;
  opacity: 0;
  transition: all 0.15s;
}

.saved-template-row:hover .saved-template-delete {
  opacity: 1;
}

.saved-template-delete:hover {
  background: var(--brand-danger-soft);
  color: var(--brand-danger);
}

/* 页脚 */
.modal-footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--spacing-sm);
  padding: var(--spacing-md) var(--spacing-lg);
  border-top: 1px solid var(--color-border-subtle);
  background: var(--color-bg-secondary);
}

.footer-left,
.footer-right {
  display: flex;
  gap: var(--spacing-sm);
  align-items: center;
}

.section-title {
  font-size: var(--font-size-md);
  font-weight: 500;
  color: var(--color-text-secondary);
  letter-spacing: 0.5px;
}

/* 过渡 */
.modal-enter-active,
.modal-leave-active {
  transition: opacity 0.25s ease;
}

.modal-enter-from,
.modal-leave-to {
  opacity: 0;
}
</style>
