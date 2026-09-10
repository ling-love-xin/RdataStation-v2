import { createRouter, createWebHistory } from 'vue-router'

import { useProjectStore } from '@/core/project/stores/project'

const router = createRouter({
  history: createWebHistory(),
  routes: [
    {
      path: '/',
      component: () => import('@/app/MainLayout.vue'),
      children: [
        {
          path: '',
          name: 'ProjectSelect',
          component: () => import('@/extensions/builtin/workbench/ui/views/ProjectSelectView.vue'),
          meta: { title: '选择项目' },
        },
        {
          path: 'workbench',
          name: 'Workbench',
          component: () => import('@/extensions/builtin/workbench/ui/views/WorkbenchView.vue'),
          meta: { title: '工作台', requiresProject: true },
        },
      ],
    },
  ],
})

// 路由守卫
router.beforeEach(async (to, _from, next) => {
  const projectStore = useProjectStore()

  // 开发模式：自动加载默认项目
  if (import.meta.env.DEV && !projectStore.hasProject) {
    await projectStore.loadLastProject()
  }

  // 检查是否是首次启动（没有 recentProjects）
  const hasRecentProjects = localStorage.getItem('recentProjects')

  // 如果是访问首页
  if (to.path === '/') {
    // 如果有最近项目或开发模式，说明不是首次启动，直接进工作台
    if (hasRecentProjects || import.meta.env.DEV) {
      // 尝试加载上次项目
      await projectStore.loadLastProject()
      if (projectStore.hasProject) {
        next('/workbench')
        return
      }
    }
    // 首次启动或没有项目，显示项目选择页面
    next()
    return
  }

  // 检查是否需要项目
  if (to.meta.requiresProject && !projectStore.hasProject) {
    // 尝试加载上次打开的项目
    await projectStore.loadLastProject()
    if (!projectStore.hasProject) {
      next('/')
      return
    }
  }

  next()
})

export default router
