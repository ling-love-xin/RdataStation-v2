<template>
  <div :class="['editor-toolbar', `toolbar-${toolbarPosition}`, `mode-${editorMode}`]">
    <!-- 模式指示器 -->
    <div
      :class="['tb-mode-indicator', editorMode]"
      title="切换编辑器模式"
      @click="showModeMenu = !showModeMenu"
    >
      <span class="mode-dot" />
      <span class="mode-label">{{ modeLabel }}</span>
      <span :class="['mode-chevron', { open: showModeMenu }]">&#9660;</span>
      <div v-if="showModeMenu" class="tb-mode-menu" @click.stop>
        <div
          v-for="m in modeOptions"
          :key="m.key"
          :class="['mode-menu-item', { active: editorMode === m.key }]"
          @click="switchMode(m.key)"
        >
          <span :class="['mode-menu-dot', m.key]" />
          <span>{{ m.label }}</span>
          <span class="mode-menu-desc">{{ m.desc }}</span>
        </div>
      </div>
    </div>
    <span class="tb-sep" />

    <div class="toolbar-group">
      <!-- 执行/运行 -->
      <NTooltip trigger="hover">
        <template #trigger>
          <NButton
            quaternary
            size="small"
            :class="['toolbar-btn', { run: true, executing: props.executing }]"
            :disabled="props.executing"
            @click="$emit('execute')"
          >
            <Play :size="16" />
          </NButton>
        </template>
        {{ runLabel }} (Ctrl+Enter)
      </NTooltip>

      <!-- 执行到新标签 -->
      <NTooltip v-if="editorMode !== 'code'" trigger="hover">
        <template #trigger>
          <NButton quaternary size="small" class="toolbar-btn" @click="$emit('executeNew')">
            <Plus :size="16" />
          </NButton>
        </template>
        {{ $t('sqlEditor.executeNew') }}
      </NTooltip>

      <!-- 批量执行 (分析模式) -->
      <NTooltip v-if="editorMode === 'analysis'" trigger="hover">
        <template #trigger>
          <NButton quaternary size="small" class="toolbar-btn" @click="$emit('executeBatch')">
            <ListChecks :size="16" />
          </NButton>
        </template>
        {{ $t('sqlEditor.executeBatch') }}
      </NTooltip>

      <!-- DuckDB 加速 -->
      <NTooltip v-if="editorMode !== 'code' && isDuckDb" trigger="hover">
        <template #trigger>
          <NButton quaternary size="small" class="toolbar-btn" @click="$emit('duckdbExecute')">
            <Zap :size="16" />
          </NButton>
        </template>
        {{ $t('sqlEditor.duckdbAccelerate') }}
      </NTooltip>

      <div class="toolbar-divider" />

      <!-- 格式化 (所有模式) -->
      <NTooltip trigger="hover">
        <template #trigger>
          <NButton quaternary size="small" class="toolbar-btn" @click="$emit('format')">
            <AlignLeft :size="16" />
          </NButton>
        </template>
        {{ formatLabel }} (Ctrl+Shift+F)
      </NTooltip>

      <!-- 验证语法 (SQL/分析模式) -->
      <NTooltip v-if="editorMode !== 'code'" trigger="hover">
        <template #trigger>
          <NButton quaternary size="small" class="toolbar-btn" @click="$emit('validate')">
            <Sparkles :size="16" />
          </NButton>
        </template>
        {{ $t('sqlEditor.validate') }}
      </NTooltip>

      <!-- 方言转译 (分析模式) -->
      <NTooltip v-if="editorMode === 'analysis'" trigger="hover">
        <template #trigger>
          <NButton quaternary size="small" class="toolbar-btn" @click="$emit('transpile')">
            <ArrowLeftRight :size="16" />
          </NButton>
        </template>
        {{ $t('sqlEditor.transpile') }}
      </NTooltip>

      <!-- 执行计划 (分析模式) -->
      <NTooltip v-if="editorMode === 'analysis'" trigger="hover">
        <template #trigger>
          <NButton quaternary size="small" class="toolbar-btn" @click="$emit('explain')">
            <FileSearch :size="16" />
          </NButton>
        </template>
        {{ $t('sqlEditor.explain') }}
      </NTooltip>

      <!-- 保存为片段 (分析模式) -->
      <NTooltip v-if="editorMode === 'analysis'" trigger="hover">
        <template #trigger>
          <NButton quaternary size="small" class="toolbar-btn" @click="$emit('saveSnippet')">
            <Star :size="16" />
          </NButton>
        </template>
        {{ $t('sqlEditor.saveSnippet') }}
      </NTooltip>

      <!-- 更多 (SQL模式溢出菜单) -->
      <template v-if="editorMode === 'sql'">
        <div class="toolbar-divider" />
        <div class="tb-overflow-wrap">
          <NButton
            quaternary
            size="small"
            class="toolbar-btn"
            title="更多"
            @click="showOverflow = !showOverflow"
          >
            <Ellipsis :size="16" />
          </NButton>
          <div v-if="showOverflow" class="tb-overflow-menu" @click.stop>
            <div class="ov-item" @click="$emit('explain'); showOverflow = false">
              <FileSearch :size="13" /> {{ $t('sqlEditor.explain') }}
            </div>
            <div class="ov-item" @click="$emit('transpile'); showOverflow = false">
              <ArrowLeftRight :size="13" /> {{ $t('sqlEditor.transpile') }}
            </div>
            <div class="ov-item" @click="$emit('saveSnippet'); showOverflow = false">
              <Star :size="13" /> {{ $t('sqlEditor.saveSnippet') }}
            </div>
          </div>
        </div>
      </template>

      <div class="toolbar-divider" />

      <!-- 迷你地图 (所有模式) -->
      <NTooltip trigger="hover">
        <template #trigger>
          <NButton quaternary size="small" class="toolbar-btn" @click="$emit('toggleMinimap')">
            <Map :size="16" />
          </NButton>
        </template>
        {{ $t('sqlEditor.toggleMinimap') }}
      </NTooltip>

      <!-- 编辑器设置 (所有模式) -->
      <NTooltip trigger="hover">
        <template #trigger>
          <NButton quaternary size="small" class="toolbar-btn" @click="$emit('toggleSettings')">
            <Settings :size="16" />
          </NButton>
        </template>
        {{ $t('sqlEditor.editorSettings') }}
      </NTooltip>
    </div>

    <div class="toolbar-spacer" />

    <!-- 连接选择器 -->
    <div v-if="toolbarPosition === 'top'" class="toolbar-group">
      <select
        class="tb-conn-select"
        :value="selectedConnection"
        @change="$emit('connectionChange', ($event.target as HTMLSelectElement).value)"
      >
        <option v-for="c in connectionOptions" :key="c.value" :value="c.value">
          {{ c.label }}
        </option>
      </select>
    </div>
  </div>
</template>

<script setup lang="ts">
import {
  Play,
  Plus,
  Zap,
  Sparkles,
  ArrowLeftRight,
  AlignLeft,
  ListChecks,
  FileSearch,
  Star,
  Map,
  Settings,
  Ellipsis,
} from 'lucide-vue-next'
import { NButton, NTooltip } from 'naive-ui'
import { computed, ref, onMounted, onUnmounted } from 'vue'

interface Props {
  toolbarPosition: 'top' | 'left' | 'right'
  isDuckDb: boolean
  showAdvanced?: boolean
  editorMode?: 'sql' | 'analysis' | 'code'
  executing?: boolean
  connectionOptions?: Array<{ label: string; value: string }>
  selectedConnection?: string
}

const props = withDefaults(defineProps<Props>(), {
  showAdvanced: true,
  editorMode: 'sql',
  executing: false,
  connectionOptions: () => [
    { label: 'MySQL / localhost', value: 'mysql' },
    { label: 'PostgreSQL / prod', value: 'pg' },
    { label: 'SQLite / local.db', value: 'sqlite' },
    { label: 'DuckDB (本地)', value: 'duckdb' },
  ],
  selectedConnection: 'mysql',
})

interface Emits {
  (e: 'execute'): void
  (e: 'executeNew'): void
  (e: 'executeBatch'): void
  (e: 'duckdbExecute'): void
  (e: 'format'): void
  (e: 'validate'): void
  (e: 'transpile'): void
  (e: 'explain'): void
  (e: 'saveSnippet'): void
  (e: 'toggleMinimap'): void
  (e: 'toggleSettings'): void
  (e: 'modeChange', mode: 'sql' | 'analysis' | 'code'): void
  (e: 'connectionChange', connId: string): void
}

const emit = defineEmits<Emits>()

const showModeMenu = ref(false)
const showOverflow = ref(false)

const modeOptions = [
  { key: 'sql' as const, label: 'SQL', desc: '查询编辑' },
  { key: 'analysis' as const, label: '分析', desc: '数据探索' },
  { key: 'code' as const, label: '代码', desc: '通用编辑' },
]

const modeLabel = computed(() => {
  const found = modeOptions.find(m => m.key === props.editorMode)
  return found?.label ?? 'SQL'
})

const runLabel = computed(() => {
  switch (props.editorMode) {
    case 'sql': return '执行 (Ctrl+Enter)'
    case 'analysis': return '执行分析 (Ctrl+Enter)'
    case 'code': return '运行 (Ctrl+Enter)'
    default: return '执行 (Ctrl+Enter)'
  }
})

const formatLabel = computed(() => {
  switch (props.editorMode) {
    case 'sql': return '格式化 SQL (Ctrl+Shift+F)'
    case 'analysis': return '格式化 (Ctrl+Shift+F)'
    case 'code': return '格式化代码 (Ctrl+Shift+F)'
    default: return '格式化 (Ctrl+Shift+F)'
  }
})

function switchMode(mode: 'sql' | 'analysis' | 'code') {
  showModeMenu.value = false
  emit('modeChange', mode)
}

function onDocumentClick(e: MouseEvent) {
  const target = e.target as HTMLElement
  if (!target.closest('.tb-mode-indicator')) {
    showModeMenu.value = false
  }
  if (!target.closest('.tb-overflow-wrap')) {
    showOverflow.value = false
  }
}

onMounted(() => {
  document.addEventListener('click', onDocumentClick)
})

onUnmounted(() => {
  document.removeEventListener('click', onDocumentClick)
})
</script>

<style scoped>
.editor-toolbar {
  display: flex;
  align-items: center;
  padding: 4px 8px;
  background: var(--bg-secondary, #252526);
  border-bottom: 1px solid var(--border-color, #3e3e42);
  gap: 2px;
  transition: background 0.35s ease;
  user-select: none;
}

/* 模式配色 */
.editor-toolbar.mode-sql { background: #1e2a36; }
.editor-toolbar.mode-analysis { background: #1a2633; }
.editor-toolbar.mode-code { background: #1f1a2e; }

.editor-toolbar.toolbar-left,
.editor-toolbar.toolbar-right {
  flex-direction: column;
  border-bottom: none;
  padding: 8px 4px;
}

.editor-toolbar.toolbar-left {
  border-right: 1px solid var(--border-color, #3e3e42);
}

.editor-toolbar.toolbar-right {
  border-left: 1px solid var(--border-color, #3e3e42);
}

/* ─── 模式指示器 ─── */
.tb-mode-indicator {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 0 8px;
  height: 28px;
  border-radius: 5px;
  font-size: 11px;
  font-weight: 600;
  cursor: pointer;
  position: relative;
  white-space: nowrap;
  flex-shrink: 0;
  transition: all 0.2s ease;
}
.tb-mode-indicator.sql { color: #4fc3f7; background: rgba(0,120,212,0.12); border: 1px solid rgba(0,120,212,0.25); }
.tb-mode-indicator.analysis { color: #64b5f6; background: rgba(14,99,156,0.12); border: 1px solid rgba(14,99,156,0.25); }
.tb-mode-indicator.code { color: #ce93d8; background: rgba(104,33,122,0.12); border: 1px solid rgba(104,33,122,0.25); }
.tb-mode-indicator:hover { filter: brightness(1.3); }

.mode-dot {
  width: 7px; height: 7px; border-radius: 50%; flex-shrink: 0;
}
.tb-mode-indicator.sql .mode-dot { background: #007acc; }
.tb-mode-indicator.analysis .mode-dot { background: #0e639c; }
.tb-mode-indicator.code .mode-dot { background: #68217a; }

.mode-chevron {
  font-size: 8px; opacity: 0.5; margin-left: 2px; transition: transform 0.2s;
}
.mode-chevron.open { transform: rotate(180deg); }

/* 模式下拉菜单 */
.tb-mode-menu {
  position: absolute; top: 32px; left: 0; z-index: 100; background: #252526;
  border: 1px solid #3c3c3c; border-radius: 7px; padding: 4px 0;
  min-width: 170px; box-shadow: 0 6px 20px rgba(0,0,0,0.5);
}
.mode-menu-item {
  padding: 8px 14px; font-size: 12px; color: #ccc; cursor: pointer;
  display: flex; align-items: center; gap: 10px; transition: background 0.08s;
}
.mode-menu-item:hover { background: #094771; color: #fff; }
.mode-menu-item.active { background: rgba(0,120,212,0.2); }
.mode-menu-dot { width: 8px; height: 8px; border-radius: 50%; flex-shrink: 0; }
.mode-menu-dot.sql { background: #007acc; }
.mode-menu-dot.analysis { background: #0e639c; }
.mode-menu-dot.code { background: #68217a; }
.mode-menu-desc { font-size: 10px; color: #666; margin-left: auto; }

.tb-sep {
  width: 1px; height: 18px; background: var(--border-color, #3e3e42); margin: 0 4px;
}

.toolbar-group {
  display: flex;
  align-items: center;
  gap: 2px;
}

.toolbar-left .toolbar-group,
.toolbar-right .toolbar-group {
  flex-direction: column;
}

.toolbar-btn {
  color: var(--text-secondary, #858585);
}

.toolbar-btn:hover {
  color: var(--text-primary, #cccccc);
  background: var(--bg-hover, #2d2d30);
}

.toolbar-btn.run {
  color: #007acc;
  background: rgba(0,120,212,0.12);
}
.toolbar-btn.run:hover {
  background: rgba(0,120,212,0.3);
}
.toolbar-btn.executing {
  background: rgba(240,192,64,0.2);
  color: #f0c040;
  animation: pulse 1.2s ease-in-out infinite;
}

@keyframes pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.4; }
}

.toolbar-divider {
  width: 1px;
  height: 16px;
  background: var(--border-color, #3e3e42);
  margin: 0 6px;
}

.toolbar-spacer { flex: 1; }

.tb-conn-select {
  font-size: 11px; color: var(--text-primary, #ccc); background: var(--bg-primary, #1e1e1e);
  border: 1px solid var(--border-color, #3e3e42); border-radius: 3px; padding: 2px 8px;
  cursor: pointer; outline: none; font-family: inherit; max-width: 180px;
}
.tb-conn-select:hover, .tb-conn-select:focus { border-color: #007acc; }

.toolbar-left .toolbar-divider,
.toolbar-right .toolbar-divider {
  width: 16px;
  height: 1px;
  margin: 6px 0;
}

/* 溢出菜单 */
.tb-overflow-wrap { position: relative; }
.tb-overflow-menu {
  position: absolute; top: 32px; right: 0; z-index: 100; background: #252526;
  border: 1px solid #3c3c3c; border-radius: 7px; padding: 4px 0;
  min-width: 160px; box-shadow: 0 6px 20px rgba(0,0,0,0.5);
}
.ov-item {
  padding: 6px 14px; font-size: 11px; color: #ccc; cursor: pointer;
  display: flex; align-items: center; gap: 8px; transition: background 0.08s;
}
.ov-item:hover { background: #094771; color: #fff; }
</style>