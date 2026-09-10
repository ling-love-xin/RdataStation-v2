<template>
  <div class="empty-workbench-panel">
    <div class="welcome-content">
      <div class="welcome-icon">
        <Database :size="48" />
      </div>
      <h2 class="welcome-title">{{ t('workbench.welcomeTitle') }}</h2>
      <p class="welcome-desc">{{ t('workbench.welcomeDesc') }}</p>

      <div class="action-buttons">
        <NButton type="primary" size="large" @click="handleNewConnection">
          <template #icon>
            <Plug :size="16" />
          </template>
          {{ t('workbench.newConnection') }}
        </NButton>
        <NButton size="large" @click="handleNewQuery">
          <template #icon>
            <FileText :size="16" />
          </template>
          {{ t('workbench.newQuery') }}
        </NButton>
      </div>

      <div v-if="recentProjects.length > 0" class="recent-projects">
        <h3>{{ t('workbench.recentProjects') }}</h3>
        <div class="project-list">
          <div
            v-for="project in recentProjects.slice(0, 5)"
            :key="project.id"
            class="project-item"
            @click="switchProject(project)"
          >
            <div class="project-info">
              <FolderOpen :size="16" class="project-icon" />
              <span class="project-name">{{ project.name }}</span>
            </div>
            <span class="project-path">{{ project.path }}</span>
          </div>
        </div>
      </div>

      <div class="quick-links">
        <h3>{{ t('workbench.quickStart') }}</h3>
        <ul>
          <li @click="handleNewConnection">{{ t('workbench.createFirstConnection') }}</li>
          <li @click="handleNewQuery">{{ t('workbench.openSqlEditor') }}</li>
          <li>{{ t('workbench.browseObjects') }}</li>
        </ul>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { Database, Plug, FileText, FolderOpen } from 'lucide-vue-next'
import { NButton, createDiscreteApi } from 'naive-ui'
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'

import { useProjectStore } from '@/core/project/stores/project'
import {
  WorkbenchEvent,
  dispatchWorkbenchEvent,
} from '@/extensions/builtin/workbench/ui/constants/workbench-events'

const { t } = useI18n()
const projectStore = useProjectStore()
const { message } = createDiscreteApi(['message'])

const recentProjects = computed(() => projectStore.recentProjects || [])

const handleNewQuery = () => {
  window.dispatchEvent(
    new CustomEvent('open-sql-editor', {
      detail: { connectionId: '', databaseName: '', sql: '' },
    })
  )
}

const handleNewConnection = () => {
  dispatchWorkbenchEvent(WorkbenchEvent.NewConnection)
}

const switchProject = async (project: { id: string; name: string; path: string }) => {
  try {
    await projectStore.switchProject(project.id)
  } catch (error) {
    console.error('切换项目失败:', error)
    message.error(t('common.switchProjectFailed'))
  }
}
</script>

<style scoped>
.empty-workbench-panel {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
  padding: 40px;
  overflow-y: auto;
}

.welcome-content {
  text-align: center;
  max-width: 500px;
  width: 100%;
}

.welcome-icon {
  width: 80px;
  height: 80px;
  margin: 0 auto 24px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 16px;
  background: linear-gradient(135deg, var(--primary-color) 0%, var(--primary-hover, #0ea5e9) 100%);
  color: white;
}

.welcome-title {
  margin: 0 0 8px 0;
  font-size: 24px;
  font-weight: 600;
  color: var(--text-primary);
}

.welcome-desc {
  margin: 0 0 32px 0;
  font-size: 14px;
  color: var(--text-secondary);
}

.action-buttons {
  display: flex;
  gap: 12px;
  justify-content: center;
  margin-bottom: 32px;
}

.recent-projects {
  text-align: left;
  padding: 16px;
  background: var(--bg-secondary);
  border-radius: 8px;
  border: 1px solid var(--border-color);
  margin-bottom: 24px;
}

.recent-projects h3 {
  margin: 0 0 12px 0;
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary);
}

.project-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.project-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 12px;
  border-radius: 6px;
  cursor: pointer;
  transition: all 0.2s;
}

.project-item:hover {
  background: var(--bg-hover);
}

.project-info {
  display: flex;
  align-items: center;
  gap: 8px;
}

.project-icon {
  color: var(--primary-color);
}

.project-name {
  font-size: 13px;
  color: var(--text-primary);
  font-weight: 500;
}

.project-path {
  font-size: 11px;
  color: var(--text-tertiary);
  max-width: 200px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.quick-links {
  text-align: left;
  padding: 16px;
  background: var(--bg-secondary);
  border-radius: 8px;
  border: 1px solid var(--border-color);
}

.quick-links h3 {
  margin: 0 0 12px 0;
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary);
}

.quick-links ul {
  margin: 0;
  padding: 0;
  list-style: none;
}

.quick-links li {
  padding: 8px 12px;
  font-size: 13px;
  color: var(--text-secondary);
  cursor: pointer;
  border-radius: 4px;
  transition: all 0.2s;
}

.quick-links li:hover {
  background: var(--bg-hover);
  color: var(--primary-color);
}
</style>
