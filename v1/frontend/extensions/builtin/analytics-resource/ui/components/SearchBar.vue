<template>
  <div class="search-bar">
    <div class="search-input-wrapper">
      <span class="search-icon">🔍</span>
      <input
        ref="inputRef"
        v-model="localQuery"
        type="text"
        class="search-input"
        :placeholder="t('analyticsResource.searchPlaceholder')"
        @input="handleInput"
        @keydown="handleKeyDown"
        @focus="showHistory = searchHistory.length > 0 && !localQuery"
        @blur="handleBlur"
      />
      <button v-if="localQuery" class="clear-btn" @click="clearSearch"> ✕ </button>
    </div>

    <div v-if="showHistory && searchHistory.length > 0" class="history-dropdown">
      <div class="history-header">
        <span class="history-title">{{ t('analyticsResource.recentSearch') }}</span>
        <button class="history-clear-btn" @click="$emit('clear-history')">
          {{ t('analyticsResource.clear') }}
        </button>
      </div>
      <div
        v-for="item in searchHistory"
        :key="item"
        class="history-item"
        @mousedown.prevent="selectHistory(item)"
      >
        <span class="history-icon">🕐</span>
        <span class="history-text">{{ item }}</span>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, watch, onMounted, onUnmounted } from 'vue'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()

const props = withDefaults(
  defineProps<{
    modelValue?: string
    debounceMs?: number
    searchHistory?: string[]
  }>(),
  {
    modelValue: '',
    debounceMs: 300,
    searchHistory: () => [],
  }
)

const emit = defineEmits<{
  'update:modelValue': [value: string]
  search: [value: string]
  clear: []
  'clear-history': []
  'history-select': [query: string]
}>()

const inputRef = ref<HTMLInputElement | null>(null)
const localQuery = ref(props.modelValue)
const showHistory = ref(false)
let debounceTimer: ReturnType<typeof setTimeout> | null = null

watch(
  () => props.modelValue,
  newVal => {
    localQuery.value = newVal
  }
)

function handleInput() {
  if (debounceTimer) {
    clearTimeout(debounceTimer)
  }
  debounceTimer = setTimeout(() => {
    emit('update:modelValue', localQuery.value)
    emit('search', localQuery.value)
  }, props.debounceMs)
}

function clearSearch() {
  localQuery.value = ''
  emit('update:modelValue', '')
  emit('clear')
  inputRef.value?.focus()
}

function handleKeyDown(e: KeyboardEvent) {
  if (e.key === 'Escape') {
    showHistory.value = false
    clearSearch()
  }
}

function handleBlur() {
  setTimeout(() => {
    showHistory.value = false
  }, 200)
}

function selectHistory(query: string) {
  localQuery.value = query
  emit('update:modelValue', query)
  emit('search', query)
  emit('history-select', query)
  showHistory.value = false
  inputRef.value?.focus()
}

function focus() {
  inputRef.value?.focus()
}

function handleGlobalKeyDown(e: KeyboardEvent) {
  if ((e.ctrlKey || e.metaKey) && e.key === 'f') {
    e.preventDefault()
    focus()
  }
}

onMounted(() => {
  document.addEventListener('keydown', handleGlobalKeyDown)
})

onUnmounted(() => {
  document.removeEventListener('keydown', handleGlobalKeyDown)
  if (debounceTimer) {
    clearTimeout(debounceTimer)
  }
})

defineExpose({ focus })
</script>

<style scoped>
.search-bar {
  padding: var(--size-md) var(--size-lg);
  border-bottom: 1px solid var(--border-color);
}

.search-input-wrapper {
  position: relative;
  display: flex;
  align-items: center;
}

.search-icon {
  position: absolute;
  left: 12px;
  font-size: var(--font-size-lg);
  color: var(--text-tertiary);
  pointer-events: none;
}

.search-input {
  width: 100%;
  padding: 6px 36px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  background: var(--bg-secondary);
  font-size: var(--font-size-md);
  color: var(--text-primary);
  transition: all 0.2s;
  height: var(--height-input);
}

.search-input:focus {
  outline: none;
  border-color: var(--primary-color);
  background: var(--bg-primary);
  box-shadow: 0 0 0 2px var(--primary-light);
}

.search-input::placeholder {
  color: var(--text-tertiary);
}

.clear-btn {
  position: absolute;
  right: 8px;
  width: 24px;
  height: 24px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: none;
  background: transparent;
  color: var(--text-tertiary);
  cursor: pointer;
  border-radius: var(--radius-sm);
  transition: all 0.15s;
}

.clear-btn:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
}

.history-dropdown {
  position: absolute;
  top: 100%;
  left: var(--size-lg);
  right: var(--size-lg);
  margin-top: 4px;
  background: var(--bg-elevated);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-md);
  z-index: 100;
  max-height: 200px;
  overflow-y: auto;
}

.history-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: var(--size-sm) var(--size-md);
  border-bottom: 1px solid var(--border-color);
}

.history-title {
  font-size: var(--font-size-xs);
  font-weight: 600;
  color: var(--text-tertiary);
  text-transform: uppercase;
}

.history-clear-btn {
  background: none;
  border: none;
  font-size: var(--font-size-xs);
  color: var(--text-tertiary);
  cursor: pointer;
  padding: 2px 6px;
  border-radius: var(--radius-sm);
}

.history-clear-btn:hover {
  color: var(--danger-color);
  background: var(--bg-hover);
}

.history-item {
  display: flex;
  align-items: center;
  gap: var(--size-sm);
  padding: var(--size-sm) var(--size-md);
  cursor: pointer;
  transition: background 0.15s;
}

.history-item:hover {
  background: var(--bg-hover);
}

.history-icon {
  font-size: var(--font-size-sm);
  opacity: 0.5;
}

.history-text {
  font-size: var(--font-size-md);
  color: var(--text-secondary);
}
</style>
