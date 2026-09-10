<template>
  <Teleport to="body">
    <Transition name="dialog">
      <div
        v-if="visible"
        ref="overlayRef"
        tabindex="-1"
        class="dialog-overlay"
        @click.self="handleClose"
        @keydown.escape="handleClose"
      >
        <div class="dialog-container" role="alertdialog" aria-labelledby="dialog-title">
          <header class="dialog-header">
            <div class="dialog-icon">
              <AlertTriangle :size="28" />
            </div>
          </header>
          <div class="dialog-body">
            <h2 id="dialog-title" class="dialog-title">{{
              t('workbench.invalidProjectFolderTitle')
            }}</h2>
            <p class="dialog-desc">{{ t('workbench.invalidProjectFolderDesc') }}</p>
            <p class="dialog-path">{{ selectedPath }}</p>
          </div>
          <footer class="dialog-footer">
            <button class="btn-browse" @click="handleBrowse">
              <FolderOpen :size="16" />
              <span>{{ t('workbench.browseAgain') }}</span>
            </button>
            <button class="btn-close" @click="handleClose">
              {{ t('common.close') }}
            </button>
          </footer>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<script setup lang="ts">
import { AlertTriangle, FolderOpen } from 'lucide-vue-next'
import { ref, watch, nextTick } from 'vue'
import { useI18n } from 'vue-i18n'

interface Props {
  visible: boolean
  selectedPath: string
}

const props = defineProps<Props>()

const emit = defineEmits<{
  browse: []
  close: []
}>()

const { t } = useI18n()
const overlayRef = ref<HTMLElement | null>(null)

watch(
  () => props.visible,
  async val => {
    if (val) {
      await nextTick()
      overlayRef.value?.focus()
    }
  }
)

function handleBrowse() {
  emit('browse')
}

function handleClose() {
  emit('close')
}
</script>

<style>
.dialog-overlay {
  position: fixed;
  inset: 0;
  z-index: 1100;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--modal-overlay-bg, rgba(0, 0, 0, 0.55));
  backdrop-filter: blur(12px);
}

.dialog-container {
  width: 420px;
  max-width: 92vw;
  max-height: 85vh;
  background: var(--modal-surface-bg, var(--color-bg-elevated, #ffffff));
  border: 1px solid var(--color-border);
  border-radius: 12px;
  box-shadow: var(--shadow-lg, 0 8px 32px rgba(0, 0, 0, 0.3));
  overflow: hidden;
  display: flex;
  flex-direction: column;
}

.dialog-header {
  display: flex;
  justify-content: center;
  padding: 28px 24px 0;
}

.dialog-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 56px;
  height: 56px;
  border-radius: 50%;
  background: rgba(253, 203, 110, 0.15);
  color: var(--brand-warning, #fdcb6e);
}

.dialog-body {
  padding: 16px 24px 24px;
  text-align: center;
}

.dialog-title {
  margin: 0 0 12px;
  font-size: var(--font-size-lg, 16px);
  font-weight: 600;
  color: var(--color-text-primary);
}

.dialog-desc {
  margin: 0 0 12px;
  font-size: var(--font-size-sm, 13px);
  color: var(--color-text-secondary);
  line-height: 1.6;
}

.dialog-path {
  margin: 0;
  padding: 8px 12px;
  font-size: 12px;
  font-family: var(--font-mono, 'JetBrains Mono', monospace);
  color: var(--color-text-muted);
  background: var(--color-bg-secondary);
  border-radius: 6px;
  word-break: break-all;
  max-height: 48px;
  overflow: hidden;
  text-overflow: ellipsis;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
}

.dialog-footer {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
  padding: 16px 24px;
  border-top: 1px solid var(--color-border-subtle);
  background: var(--color-bg-tertiary);
}

.btn-browse {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 8px 16px;
  border: none;
  border-radius: 6px;
  background: var(--brand-accent, #e17055);
  color: #fff;
  font-size: var(--font-size-sm, 13px);
  font-weight: 500;
  cursor: pointer;
  transition: opacity 0.15s;
}

.btn-browse:hover {
  opacity: 0.9;
}

.btn-close {
  padding: 8px 16px;
  border: 1px solid var(--color-border);
  border-radius: 6px;
  background: transparent;
  color: var(--color-text-secondary);
  font-size: var(--font-size-sm, 13px);
  cursor: pointer;
  transition: background 0.15s;
}

.btn-close:hover {
  background: var(--color-hover);
}

.dialog-enter-active,
.dialog-leave-active {
  transition: opacity 0.2s ease;
}

.dialog-enter-from,
.dialog-leave-to {
  opacity: 0;
}

.dialog-enter-active .dialog-container {
  transition: transform 0.2s ease;
}

.dialog-enter-from .dialog-container {
  transform: scale(0.95);
}
</style>
