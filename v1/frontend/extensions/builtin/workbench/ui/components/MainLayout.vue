<template>
  <div :class="['rdatastation-root', uiStore.isDark ? 'dark' : 'light']">
    <!-- DBeaver 风格的纯导航栏布局 -->
    <div class="dbeaver-layout">
      <!-- 左侧数据库导航栏 -->
      <div class="database-navigator-container">
        <DatabaseNavigator />
      </div>

      <!-- 右侧空白区域 -->
      <div class="right-panel">
        <!-- 工作台内容将从导航栏启动 -->
        <div class="empty-workbench">
          <div class="empty-content">
            <p class="empty-title">{{ t('workbench.welcomeToRdataStation') }}</p>
            <p class="empty-subtitle">{{ t('workbench.selectDbObjectToStart') }}</p>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'

import DatabaseNavigator from '@/extensions/builtin/database/ui/components/database-navigator.vue'
import { useUiStore } from '@/shared/stores/ui'

const { t } = useI18n()

// UI Store
const uiStore = useUiStore()
</script>

<style scoped>
.rdatastation-root {
  width: 100vw;
  height: 100vh;
  font-family: var(--font-sans);
  overflow: hidden;
}

.dbeaver-layout {
  display: flex;
  width: 100%;
  height: 100%;
}

/* 左侧数据库导航栏容器 */
.database-navigator-container {
  width: 250px;
  height: 100%;
  background-color: var(--bg-secondary);
  border-right: 1px solid var(--border-color);
  overflow: hidden;
  flex-shrink: 0;
}

/* 右侧空白区域 */
.right-panel {
  flex: 1;
  height: 100%;
  background-color: var(--bg-primary);
  overflow: hidden;
  display: flex;
  align-items: center;
  justify-content: center;
}

/* 空工作台样式 */
.empty-workbench {
  text-align: center;
  color: var(--text-tertiary);
  padding: 40px;
}

.empty-title {
  font-size: 20px;
  font-weight: 500;
  margin-bottom: 8px;
  color: var(--text-primary);
}

.empty-subtitle {
  font-size: 14px;
  color: var(--text-secondary);
}

/* 深色主题样式 */
.dark {
  --bg-primary: #1e1e1e;
  --bg-secondary: #1e1e1e;
  --border-color: #3c3c3c;
  --text-primary: #d4d4d4;
  --text-secondary: #969696;
  --text-tertiary: #6a6a6a;
}

/* 浅色主题样式 */
.light {
  --bg-primary: #ffffff;
  --bg-secondary: #f5f5f5;
  --border-color: #e0e0e0;
  --text-primary: #333333;
  --text-secondary: #666666;
  --text-tertiary: #999999;
}
</style>
