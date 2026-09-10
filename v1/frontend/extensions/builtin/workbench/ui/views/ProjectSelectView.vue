<template>
  <div class="project-select-view" :class="{ dark: uiStore.isDark }">
    <div class="main-content">
      <!-- 左侧介绍区 -->
      <div class="left-panel">
        <div class="brand">
          <div class="logo">
            <Database :size="48" />
          </div>
          <h1 class="brand-title">RdataStation</h1>
          <p class="brand-subtitle">{{ t('workbench.nextGenDbTool') }}</p>
        </div>

        <div class="features">
          <div class="feature-item">
            <div class="feature-icon">
              <Zap :size="24" />
            </div>
            <div class="feature-text">
              <h3>{{ t('workbench.highPerformance') }}</h3>
              <p>{{ t('workbench.duckdbLocalEngine') }}</p>
            </div>
          </div>
          <div class="feature-item">
            <div class="feature-icon">
              <Shield :size="24" />
            </div>
            <div class="feature-text">
              <h3>{{ t('workbench.safeReliable') }}</h3>
              <p>{{ t('workbench.localStorageControl') }}</p>
            </div>
          </div>
          <div class="feature-item">
            <div class="feature-icon">
              <Layers :size="24" />
            </div>
            <div class="feature-text">
              <h3>{{ t('workbench.multiDbSupport') }}</h3>
              <p>{{ t('workbench.supportedDatabases') }}</p>
            </div>
          </div>
        </div>
      </div>

      <!-- 右侧操作区 -->
      <div class="right-panel">
        <div class="action-cards">
          <div
            class="action-card primary"
            :class="{ disabled: isOpeningProject }"
            @click="!isOpeningProject && (showNewProjectModal = true)"
          >
            <div class="action-icon">
              <FolderPlus :size="32" />
            </div>
            <div class="action-content">
              <h2>{{ t('workbench.newProject') }}</h2>
              <p>{{ t('workbench.createWorkspace') }}</p>
            </div>
            <ChevronRight :size="20" />
          </div>

          <div
            class="action-card"
            :class="{ disabled: isOpeningProject }"
            @click="!isOpeningProject && handleOpenExistingProject()"
          >
            <div class="action-icon">
              <Loader v-if="isOpeningProject" :size="32" class="spin" />
              <FolderOpen v-else :size="32" />
            </div>
            <div class="action-content">
              <h2>{{ t('workbench.openProject') }}</h2>
              <p>{{ t('workbench.browseOpenProject') }}</p>
            </div>
            <ChevronRight :size="20" />
          </div>
        </div>

        <!-- 最近项目 -->
        <div class="recent-section">
          <div class="recent-header">
            <h3>{{ t('workbench.recentlyOpened') }}</h3>
          </div>

          <div v-if="projectStore.recentProjects.length === 0" class="recent-empty">
            <FolderX :size="32" />
            <p>{{ t('workbench.noRecentProjects') }}</p>
          </div>

          <div v-else class="recent-list">
            <div
              v-for="project in projectStore.recentProjects"
              :key="project.id"
              class="recent-item"
              :class="{ disabled: isOpeningProject }"
              @click="!isOpeningProject && handleOpenRecentProject(project.id)"
            >
              <div class="recent-icon">
                <Database :size="18" />
              </div>
              <div class="recent-info">
                <span class="recent-name">{{ project.name }}</span>
                <span class="recent-path">{{ project.path }}</span>
              </div>
              <span class="recent-time">{{ formatTime(project.updatedAt) }}</span>
            </div>
          </div>
        </div>
      </div>
    </div>

    <NewProjectModal
      :visible="showNewProjectModal"
      @confirm="handleCreateProject"
      @cancel="showNewProjectModal = false"
    />

    <InvalidProjectDialog
      :visible="showInvalidProjectDialog"
      :selected-path="invalidPath"
      @browse="handleBrowseAgain"
      @close="showInvalidProjectDialog = false"
    />
  </div>
</template>

<script setup lang="ts">
import {
  Database,
  FolderPlus,
  FolderOpen,
  FolderX,
  ChevronRight,
  Zap,
  Shield,
  Layers,
  Loader,
} from 'lucide-vue-next'
import { useMessage } from 'naive-ui'
import { ref, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'

import { useProjectStore } from '@/core/project/stores/project'
import InvalidProjectDialog from '@/extensions/builtin/workbench/ui/components/title-bar/InvalidProjectDialog.vue'
import NewProjectModal from '@/extensions/builtin/workbench/ui/components/title-bar/NewProjectModal.vue'
import { useUiStore } from '@/shared/stores/ui'
import { useAppStore } from '@/stores/useAppStore'

const { t } = useI18n()
const router = useRouter()
const message = useMessage()
const uiStore = useUiStore()
const projectStore = useProjectStore()

const showNewProjectModal = ref(false)
const showInvalidProjectDialog = ref(false)
const invalidPath = ref('')
const isOpeningProject = ref(false)

function formatTime(dateStr: string): string {
  const date = new Date(dateStr)
  const now = new Date()
  const diff = now.getTime() - date.getTime()
  const days = Math.floor(diff / (1000 * 60 * 60 * 24))

  if (days === 0) {
    const hours = Math.floor(diff / (1000 * 60 * 60))
    if (hours === 0) {
      const minutes = Math.floor(diff / (1000 * 60))
      return minutes === 0 ? t('workbench.justNow') : t('workbench.minutesAgo', { count: minutes })
    }
    return t('workbench.hoursAgo', { count: hours })
  } else if (days === 1) {
    return t('workbench.yesterday')
  } else if (days < 7) {
    return t('workbench.daysAgo', { count: days })
  } else {
    return date.toLocaleDateString()
  }
}

async function handleOpenExistingProject() {
  if (isOpeningProject.value) return
  isOpeningProject.value = true
  try {
    const { open } = await import('@tauri-apps/plugin-dialog')
    const selected = await open({
      directory: true,
      multiple: false,
      title: t('workbench.selectProjectFolder'),
    })

    if (selected && typeof selected === 'string') {
      const project = await projectStore.openProject(selected)
      if (project) {
        await enterWorkbench(project.id, project.path)
      } else {
        invalidPath.value = selected
        showInvalidProjectDialog.value = true
      }
    }
  } catch (error) {
    console.error('打开项目失败:', error)
    message.error(t('workbench.openProjectFailed'))
  } finally {
    isOpeningProject.value = false
  }
}

async function handleBrowseAgain() {
  showInvalidProjectDialog.value = false
  await handleOpenExistingProject()
}

async function handleOpenRecentProject(projectId: string) {
  if (isOpeningProject.value) return
  isOpeningProject.value = true
  try {
    await projectStore.switchProject(projectId)
    const p = projectStore.currentProject
    if (p) {
      await enterWorkbench(p.id, p.path)
    }
  } catch (error) {
    console.error('切换项目失败:', error)
    message.error(t('workbench.switchProjectFailed'))
  } finally {
    isOpeningProject.value = false
  }
}

async function handleCreateProject(name: string, path: string, description?: string) {
  isOpeningProject.value = true
  try {
    const project = await projectStore.createProject(name, path, description)
    if (project) {
      showNewProjectModal.value = false
      await enterWorkbench(project.id, project.path)
      message.success(t('workbench.createProjectSuccess'))
    }
  } catch (error) {
    console.error('创建项目失败:', error)
    message.error(t('workbench.createProjectFailed'))
  } finally {
    isOpeningProject.value = false
  }
}

async function enterWorkbench(projectId: string, projectPath: string) {
  const appStore = useAppStore()
  await appStore.openProject(projectPath)
  router.push('/workbench')
}

onMounted(async () => {
  await projectStore.loadRecentProjects()
})
</script>

<style scoped>
.project-select-view {
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 100vh;
  padding: var(--spacing-xl);
  background:
    radial-gradient(ellipse at 20% 50%, var(--brand-accent-soft) 0%, transparent 60%),
    radial-gradient(ellipse at 80% 20%, var(--primary-soft) 0%, transparent 50%),
    var(--color-bg-primary);
  user-select: none;
}

.main-content {
  display: flex;
  gap: 0;
  max-width: 960px;
  width: 100%;
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border-subtle);
  border-radius: 16px;
  box-shadow: var(--shadow-lg);
  overflow: hidden;
}

/* 左侧品牌区 */
.left-panel {
  flex: 1;
  display: flex;
  flex-direction: column;
  justify-content: center;
  gap: var(--spacing-xl);
  padding: var(--spacing-xl);
  background: var(--color-bg-secondary);
}

.logo {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 72px;
  height: 72px;
  border-radius: var(--border-radius-md);
  background: linear-gradient(135deg, var(--brand-accent) 0%, var(--primary-color) 100%);
  color: #fff;
  margin-bottom: var(--spacing-md);
}

.brand-title {
  font-size: 28px;
  font-weight: 700;
  color: var(--color-text-primary);
  margin: var(--spacing-sm) 0;
  letter-spacing: -0.5px;
}

.brand-subtitle {
  font-size: var(--font-size-md);
  color: var(--color-text-secondary);
  line-height: 1.5;
  margin: 0;
}

.features {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-md);
  margin-top: var(--spacing-sm);
}

.feature-item {
  display: flex;
  align-items: flex-start;
  gap: var(--spacing-md);
  padding: var(--spacing-sm);
  border-radius: var(--border-radius-sm);
  transition: background 0.15s ease;
}

.feature-item:hover {
  background: var(--color-hover);
}

.feature-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 36px;
  height: 36px;
  border-radius: var(--border-radius-sm);
  background: var(--brand-accent-soft);
  color: var(--brand-accent);
  flex-shrink: 0;
}

.feature-text h3 {
  font-size: var(--font-size-md);
  font-weight: 600;
  color: var(--color-text-primary);
  margin: 0 0 2px;
}

.feature-text p {
  font-size: var(--font-size-sm);
  color: var(--color-text-muted);
  margin: 0;
  line-height: 1.5;
}

/* 右侧操作区 */
.right-panel {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: var(--spacing-md);
  min-width: 0;
  padding: var(--spacing-lg);
  background: var(--color-bg-elevated);
}

.action-cards {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-md);
}

.action-card {
  display: flex;
  align-items: center;
  gap: var(--spacing-md);
  padding: var(--spacing-lg);
  border-radius: var(--border-radius-md);
  border: 1px solid var(--color-border);
  background: var(--color-bg-secondary);
  cursor: pointer;
  transition: all 0.15s ease;
}

.action-card:hover {
  border-color: var(--brand-accent);
  background: var(--brand-accent-soft);
}

.action-card.disabled {
  opacity: 0.5;
  pointer-events: none;
  cursor: not-allowed;
}

.action-card.primary {
  background: linear-gradient(
    135deg,
    var(--primary-color) 0%,
    color-mix(in srgb, var(--primary-color) 80%, var(--brand-accent)) 100%
  );
  border: none;
  color: #fff;
}

.action-card.primary:hover {
  box-shadow: 0 4px 16px var(--brand-accent-soft);
}

.action-card.primary .action-content h2,
.action-card.primary .action-content p {
  color: #fff;
}

.action-card.primary .action-icon {
  background: rgba(255, 255, 255, 0.2);
  border-radius: var(--border-radius-sm);
  color: #fff;
}

.action-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 44px;
  height: 44px;
  border-radius: var(--border-radius-sm);
  background: var(--brand-accent-soft);
  color: var(--brand-accent);
  flex-shrink: 0;
}

.action-content {
  flex: 1;
  min-width: 0;
}

.action-content h2 {
  font-size: var(--font-size-md);
  font-weight: 600;
  color: var(--color-text-primary);
  margin: 0 0 4px;
}

.action-content p {
  font-size: var(--font-size-sm);
  color: var(--color-text-muted);
  margin: 0;
}

/* 最近项目 */
.recent-section {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  background: var(--color-bg-secondary);
  border: 1px solid var(--color-border-subtle);
  border-radius: var(--border-radius-md);
  padding: var(--spacing-md);
}

.recent-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: var(--spacing-sm);
  padding: 0 var(--spacing-sm);
}

.recent-header h3 {
  font-size: var(--font-size-sm);
  font-weight: 600;
  color: var(--color-text-secondary);
  text-transform: uppercase;
  letter-spacing: 0.5px;
  margin: 0;
}

.recent-empty {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: var(--spacing-sm);
  padding: var(--spacing-lg);
  color: var(--color-text-muted);
}

.recent-empty p {
  font-size: var(--font-size-sm);
  margin: 0;
}

.recent-list {
  flex: 1;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.recent-item {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  padding: var(--spacing-sm) var(--spacing-sm);
  border-radius: var(--border-radius-sm);
  cursor: pointer;
  transition: background 0.15s ease;
}

.recent-item:hover {
  background: var(--color-hover);
}

.recent-item.disabled {
  opacity: 0.4;
  pointer-events: none;
  cursor: not-allowed;
}

.recent-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--color-text-muted);
  flex-shrink: 0;
  width: 28px;
  height: 28px;
  border-radius: 4px;
  background: var(--color-bg-tertiary);
}

.recent-info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
}

.recent-name {
  font-size: var(--font-size-sm);
  font-weight: 500;
  color: var(--color-text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.recent-path {
  font-size: var(--font-size-xs);
  color: var(--color-text-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-family: var(--font-mono);
  direction: rtl;
  text-align: left;
}

.recent-time {
  font-size: var(--font-size-xs);
  color: var(--color-text-muted);
  flex-shrink: 0;
}

/* 响应式 */
@media (max-width: 720px) {
  .project-select-view {
    padding: var(--spacing-md);
  }

  .main-content {
    flex-direction: column;
    gap: 0;
  }

  .left-panel {
    padding: var(--spacing-lg);
  }

  .right-panel {
    padding: var(--spacing-md);
  }
}

@keyframes spin {
  from {
    transform: rotate(0deg);
  }
  to {
    transform: rotate(360deg);
  }
}

.spin {
  animation: spin 1s linear infinite;
}
</style>
