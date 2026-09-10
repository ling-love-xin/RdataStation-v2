<template>
  <div
    class="sql-toolbar"
    :class="[`toolbar-${position}`, { 'is-pinned': isPinned, 'is-collapsed': isCollapsed }]"
  >
    <!-- Pin 按钮 -->
    <button
      class="toolbar-pin-btn"
      :class="`pin-${position}`"
      :title="isPinned ? t('sqlToolbar.unpin') : t('sqlToolbar.pin')"
      @click="togglePin"
    >
      <Pin :size="12" :class="{ 'pin-active': isPinned }" />
    </button>

    <!-- 工具栏内容 -->
    <div v-show="!isCollapsed || isPinned" class="toolbar-inner">
      <!-- 上下工具栏：水平布局 -->
      <template v-if="!isVertical">
        <div class="toolbar-row">
          <!-- 左侧按钮组 - 执行相关 -->
          <div class="toolbar-section">
            <button
              v-for="btn in executeButtons"
              :key="btn.id"
              class="toolbar-btn"
              :class="{ 'btn-primary': btn.primary, 'btn-danger': btn.danger }"
              :title="btn.title"
              @click="handleClick(btn.action)"
            >
              <component :is="getIcon(btn.icon)" :size="14" />
              <span v-if="showLabels" class="btn-label">{{ btn.label }}</span>
            </button>
          </div>

          <!-- 分隔线 -->
          <div class="toolbar-divider" />

          <!-- 中间按钮组 - 编辑操作 -->
          <div class="toolbar-section">
            <button
              v-for="btn in editButtons"
              :key="btn.id"
              class="toolbar-btn"
              :title="btn.title"
              @click="handleClick(btn.action)"
            >
              <component :is="getIcon(btn.icon)" :size="14" />
              <span v-if="showLabels" class="btn-label">{{ btn.label }}</span>
            </button>
          </div>

          <!-- 分隔线 -->
          <div class="toolbar-divider" />

          <!-- 文件操作 -->
          <div class="toolbar-section">
            <button
              v-for="btn in fileButtons"
              :key="btn.id"
              class="toolbar-btn"
              :title="btn.title"
              @click="handleClick(btn.action)"
            >
              <component :is="getIcon(btn.icon)" :size="14" />
              <span v-if="showLabels" class="btn-label">{{ btn.label }}</span>
            </button>
          </div>

          <!-- 弹性空间 -->
          <div class="toolbar-spacer" />

          <!-- 右侧按钮组 - 导航和工具 -->
          <div class="toolbar-section">
            <button
              v-for="btn in navigateButtons"
              :key="btn.id"
              class="toolbar-btn"
              :title="btn.title"
              @click="handleClick(btn.action)"
            >
              <component :is="getIcon(btn.icon)" :size="14" />
              <span v-if="showLabels" class="btn-label">{{ btn.label }}</span>
            </button>
          </div>

          <!-- 分隔线 -->
          <div class="toolbar-divider" />

          <!-- 工具和历史 -->
          <div class="toolbar-section">
            <button
              v-for="btn in toolButtons"
              :key="btn.id"
              class="toolbar-btn"
              :title="btn.title"
              @click="handleClick(btn.action)"
            >
              <component :is="getIcon(btn.icon)" :size="14" />
              <span v-if="showLabels" class="btn-label">{{ btn.label }}</span>
            </button>
          </div>

          <!-- 设置按钮 -->
          <div class="toolbar-section">
            <button
              class="toolbar-btn"
              :title="t('sqlToolbar.editorSettings')"
              @click="$emit('showSettings')"
            >
              <Settings :size="14" />
            </button>
          </div>
        </div>
      </template>

      <!-- 左右工具栏：垂直布局 -->
      <template v-else>
        <div class="toolbar-col">
          <!-- 执行按钮组 -->
          <div class="btn-group-vertical">
            <button
              v-for="btn in executeButtons"
              :key="btn.id"
              class="toolbar-btn btn-vertical"
              :class="{ 'btn-primary': btn.primary, 'btn-danger': btn.danger }"
              :title="btn.title"
              @click="handleClick(btn.action)"
            >
              <component :is="getIcon(btn.icon)" :size="14" />
            </button>
          </div>

          <!-- 分隔线 -->
          <div class="btn-divider-vertical" />

          <!-- 编辑按钮组 -->
          <div class="btn-group-vertical">
            <button
              v-for="btn in editButtons"
              :key="btn.id"
              class="toolbar-btn btn-vertical"
              :title="btn.title"
              @click="handleClick(btn.action)"
            >
              <component :is="getIcon(btn.icon)" :size="14" />
            </button>
          </div>

          <!-- 弹性空间 -->
          <div class="toolbar-spacer-vertical" />

          <!-- 文件和工具 -->
          <div class="btn-group-vertical">
            <button
              v-for="btn in [...fileButtons, ...toolButtons]"
              :key="btn.id"
              class="toolbar-btn btn-vertical"
              :title="btn.title"
              @click="handleClick(btn.action)"
            >
              <component :is="getIcon(btn.icon)" :size="14" />
            </button>
          </div>

          <!-- 设置按钮 -->
          <button
            class="toolbar-btn btn-vertical"
            :title="t('sqlToolbar.editorSettings')"
            @click="$emit('showSettings')"
          >
            <Settings :size="14" />
          </button>
        </div>
      </template>
    </div>

    <!-- 悬浮触发区（收起时） -->
    <div
      v-if="isCollapsed && !isPinned"
      class="toolbar-trigger"
      :class="`trigger-${position}`"
      @mouseenter="isCollapsed = false"
    />
  </div>
</template>

<script setup lang="ts">
import {
  Play,
  Square,
  FileCode,
  AlignLeft,
  GitBranch,
  Check,
  RotateCcw,
  ToggleLeft,
  Download,
  Upload,
  Filter,
  Search,
  Plus,
  FileText,
  Save,
  FolderOpen,
  Settings,
  History,
  BookOpen,
  Keyboard,
  Terminal,
  Zap,
  Pin,
} from 'lucide-vue-next'
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()

const props = defineProps<{
  position: 'left' | 'right' | 'top' | 'bottom'
  showLabels?: boolean
}>()

const emit = defineEmits<{
  (e: 'execute'): void
  (e: 'executeSelected'): void
  (e: 'explain'): void
  (e: 'stop'): void
  (e: 'format'): void
  (e: 'comment'): void
  (e: 'uppercase'): void
  (e: 'lowercase'): void
  (e: 'newTab'): void
  (e: 'openFile'): void
  (e: 'saveFile'): void
  (e: 'find'): void
  (e: 'replace'): void
  (e: 'goto'): void
  (e: 'showHistory'): void
  (e: 'showFavorites'): void
  (e: 'showSnippets'): void
  (e: 'showSettings'): void
  (e: 'togglePosition'): void
}>()

// Pin 状态
const isPinned = ref(true)
const isCollapsed = ref(false)

// 是否为垂直布局
const isVertical = computed(() => props.position === 'left' || props.position === 'right')

// 切换 Pin 状态
const togglePin = () => {
  isPinned.value = !isPinned.value
  if (!isPinned.value) {
    // 取消固定时，延迟后自动收起
    setTimeout(() => {
      if (!isPinned.value) {
        isCollapsed.value = true
      }
    }, 3000)
  }
}

// 图标映射
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const iconMap: Record<string, any> = {
  Play,
  Square,
  FileCode,
  AlignLeft,
  GitBranch,
  Check,
  RotateCcw,
  ToggleLeft,
  Download,
  Upload,
  Filter,
  Search,
  Plus,
  FileText,
  Save,
  FolderOpen,
  Settings,
  History,
  BookOpen,
  Keyboard,
  Terminal,
  Zap,
}

const getIcon = (name: string) => iconMap[name] || Play

// 执行按钮组
const executeButtons = [
  {
    id: 'execute',
    icon: 'Play',
    label: t('sqlToolbar.execute'),
    title: t('sqlToolbar.executeSql'),
    action: 'execute',
    primary: true,
  },
  {
    id: 'executeSelected',
    icon: 'FileCode',
    label: t('sqlToolbar.executeSelected'),
    title: t('sqlToolbar.executeSelectedSql'),
    action: 'executeSelected',
  },
  {
    id: 'explain',
    icon: 'GitBranch',
    label: t('sqlToolbar.explainPlan'),
    title: t('sqlToolbar.explainExecutionPlan'),
    action: 'explain',
  },
  {
    id: 'stop',
    icon: 'Square',
    label: t('sqlToolbar.stop'),
    title: t('sqlToolbar.stopExecution'),
    action: 'stop',
    danger: true,
  },
]

// 编辑按钮组
const editButtons = [
  {
    id: 'format',
    icon: 'AlignLeft',
    label: t('sqlToolbar.format'),
    title: t('sqlToolbar.formatSql'),
    action: 'format',
  },
  {
    id: 'comment',
    icon: 'Terminal',
    label: t('sqlToolbar.comment'),
    title: t('sqlToolbar.toggleComment'),
    action: 'comment',
  },
  {
    id: 'uppercase',
    icon: 'Zap',
    label: t('sqlToolbar.uppercase'),
    title: t('sqlToolbar.toUppercase'),
    action: 'uppercase',
  },
  {
    id: 'lowercase',
    icon: 'Zap',
    label: t('sqlToolbar.lowercase'),
    title: t('sqlToolbar.toLowercase'),
    action: 'lowercase',
  },
]

// 文件按钮组
const fileButtons = [
  {
    id: 'newTab',
    icon: 'Plus',
    label: t('sqlToolbar.newScript'),
    title: t('sqlToolbar.newScriptTooltip'),
    action: 'newTab',
  },
  {
    id: 'openFile',
    icon: 'FolderOpen',
    label: t('sqlToolbar.openFile'),
    title: t('sqlToolbar.openFileTooltip'),
    action: 'openFile',
  },
  {
    id: 'saveFile',
    icon: 'Save',
    label: t('sqlToolbar.save'),
    title: t('sqlToolbar.saveTooltip'),
    action: 'saveFile',
  },
]

// 导航按钮组
const navigateButtons = [
  {
    id: 'find',
    icon: 'Search',
    label: t('sqlToolbar.find'),
    title: t('sqlToolbar.findTooltip'),
    action: 'find',
  },
  {
    id: 'replace',
    icon: 'FileText',
    label: t('sqlToolbar.replace'),
    title: t('sqlToolbar.replaceTooltip'),
    action: 'replace',
  },
  {
    id: 'goto',
    icon: 'Keyboard',
    label: t('sqlToolbar.goto'),
    title: t('sqlToolbar.gotoTooltip'),
    action: 'goto',
  },
]

// 工具按钮组
const toolButtons = [
  {
    id: 'showHistory',
    icon: 'History',
    label: t('sqlToolbar.history'),
    title: t('sqlToolbar.historyTooltip'),
    action: 'showHistory',
  },
  {
    id: 'showFavorites',
    icon: 'BookOpen',
    label: t('sqlToolbar.favorites'),
    title: t('sqlToolbar.favoritesTooltip'),
    action: 'showFavorites',
  },
  {
    id: 'showSnippets',
    icon: 'FileText',
    label: t('sqlToolbar.snippets'),
    title: t('sqlToolbar.snippetsTooltip'),
    action: 'showSnippets',
  },
]

// 处理按钮点击
const handleClick = (action: string) => {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  emit(action as any)
}
</script>

<style scoped>
/* 工具栏基础样式 */
.sql-toolbar {
  position: relative;
  background: var(--bg-secondary);
  transition: all 0.2s ease;
  z-index: 100;
}

/* 顶部工具栏 */
.sql-toolbar.toolbar-top {
  border-bottom: 1px solid var(--border-color);
  height: 36px;
}

.sql-toolbar.toolbar-top.is-collapsed {
  height: 3px;
  overflow: hidden;
}

/* 底部工具栏 */
.sql-toolbar.toolbar-bottom {
  border-top: 1px solid var(--border-color);
  height: 36px;
}

.sql-toolbar.toolbar-bottom.is-collapsed {
  height: 3px;
  overflow: hidden;
}

/* 左侧工具栏 */
.sql-toolbar.toolbar-left {
  border-right: 1px solid var(--border-color);
  width: 38px;
}

.sql-toolbar.toolbar-left.is-collapsed {
  width: 3px;
  overflow: hidden;
}

/* 右侧工具栏 */
.sql-toolbar.toolbar-right {
  border-left: 1px solid var(--border-color);
  width: 38px;
}

.sql-toolbar.toolbar-right.is-collapsed {
  width: 3px;
  overflow: hidden;
}

/* 工具栏内部 */
.toolbar-inner {
  height: 100%;
  padding: 2px 6px;
  box-sizing: border-box;
}

/* Pin 按钮 */
.toolbar-pin-btn {
  position: absolute;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 14px;
  height: 14px;
  padding: 0;
  background: var(--bg-tertiary);
  border: 1px solid var(--border-color);
  border-radius: 2px;
  color: var(--text-secondary);
  cursor: pointer;
  opacity: 0;
  transition: all 0.15s;
  z-index: 10;
}

.sql-toolbar:hover .toolbar-pin-btn {
  opacity: 1;
}

.toolbar-pin-btn:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
}

.pin-active {
  color: var(--primary-color);
  transform: rotate(-45deg);
}

/* Pin 按钮位置 */
.pin-top {
  right: 6px;
  top: 50%;
  transform: translateY(-50%);
}

.pin-bottom {
  right: 6px;
  top: 50%;
  transform: translateY(-50%);
}

.pin-left {
  right: 3px;
  top: 6px;
}

.pin-right {
  left: 3px;
  top: 6px;
}

/* 悬浮触发区 */
.toolbar-trigger {
  position: absolute;
  background: transparent;
  cursor: pointer;
  z-index: 5;
}

.trigger-top {
  bottom: -6px;
  left: 0;
  right: 0;
  height: 6px;
}

.trigger-bottom {
  top: -6px;
  left: 0;
  right: 0;
  height: 6px;
}

.trigger-left {
  right: -6px;
  top: 0;
  bottom: 0;
  width: 6px;
}

.trigger-right {
  left: -6px;
  top: 0;
  bottom: 0;
  width: 6px;
}

/* 水平布局 */
.toolbar-row {
  display: flex;
  align-items: center;
  height: 100%;
  gap: 4px;
}

.toolbar-section {
  display: flex;
  align-items: center;
  gap: 1px;
}

.toolbar-spacer {
  flex: 1;
}

.toolbar-divider {
  width: 1px;
  height: 20px;
  background: var(--border-color);
  margin: 0 4px;
}

/* 垂直布局 */
.toolbar-col {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
  padding: 4px 0;
  height: 100%;
  box-sizing: border-box;
}

.btn-group-vertical {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 1px;
}

.toolbar-spacer-vertical {
  flex: 1;
}

.btn-divider-vertical {
  width: 20px;
  height: 1px;
  background: var(--border-color);
  margin: 4px 0;
}

/* 按钮 */
.toolbar-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 3px;
  padding: 4px 8px;
  background: transparent;
  border: none;
  border-radius: 3px;
  color: var(--text-secondary);
  cursor: pointer;
  transition: all 0.12s ease;
  font-size: 11px;
  white-space: nowrap;
  height: 28px;
}

.toolbar-btn:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
}

.toolbar-btn:active {
  background: var(--bg-tertiary);
}

/* 主要按钮 */
.btn-primary {
  background: var(--primary-color);
  color: var(--bg-primary);
}

.btn-primary:hover {
  background: var(--primary-hover);
}

/* 危险按钮 */
.btn-danger {
  color: var(--danger-color);
}

.btn-danger:hover {
  background: var(--danger-color);
  color: var(--bg-primary);
}

/* 垂直按钮 */
.btn-vertical {
  width: 30px;
  height: 30px;
  padding: 0;
}

/* 按钮标签 */
.btn-label {
  font-size: 11px;
}

/* 收起状态指示条 */
.sql-toolbar.is-collapsed::before {
  content: '';
  position: absolute;
  background: var(--primary-color);
  opacity: 0.4;
  transition: opacity 0.15s;
}

.sql-toolbar.is-collapsed:hover::before {
  opacity: 0.8;
}

.toolbar-top.is-collapsed::before,
.toolbar-bottom.is-collapsed::before {
  left: 50%;
  transform: translateX(-50%);
  width: 32px;
  height: 2px;
  border-radius: 1px;
}

.toolbar-top.is-collapsed::before {
  bottom: 0;
}

.toolbar-bottom.is-collapsed::before {
  top: 0;
}

.toolbar-left.is-collapsed::before,
.toolbar-right.is-collapsed::before {
  top: 50%;
  transform: translateY(-50%);
  width: 2px;
  height: 32px;
  border-radius: 1px;
}

.toolbar-left.is-collapsed::before {
  right: 0;
}

.toolbar-right.is-collapsed::before {
  left: 0;
}
</style>
