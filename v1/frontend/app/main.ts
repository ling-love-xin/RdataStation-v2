import { invoke } from '@tauri-apps/api/core'
import { createPinia } from 'pinia'
import { createApp } from 'vue'

import { builtinExtensions } from '@/core/builtin-extensions'
import { extensionHost } from '@/core/extension-host'
import { panelRegistry } from '@/core/panel-registry'
import IconTab from '@/extensions/builtin/workbench/ui/components/IconTab.vue'
import MinimalEditorTab from '@/extensions/builtin/workbench/ui/components/MinimalEditorTab.vue'
import PanelHeaderActions from '@/extensions/builtin/workbench/ui/components/PanelHeaderActions.vue'
import i18n from '@/shared/plugins/i18n'
import { useAppStore } from '@/stores/useAppStore'

import App from './App.vue'
import router from './router'

import '@/shared/styles/global.css'
import 'dockview-vue/dist/styles/dockview.css'
import '@/shared/styles/dockview-brand.css'
import '@/shared/styles/ag-grid-theme.css'
const app = createApp(App)

const pinia = createPinia()
app.use(pinia)
app.use(router)
app.use(i18n)

async function main() {
  const appStore = useAppStore()
  await appStore.initialize()
  appStore.applyTheme()

  try {
    const apiVersion = await invoke<{ version: string }>('get_api_version')
    const expectedVersion = '1.0.0'
    if (apiVersion.version !== expectedVersion) {
      console.warn(
        `[Main] API version mismatch: frontend expects ${expectedVersion}, backend returns ${apiVersion.version}`
      )
    } else {
      // eslint-disable-next-line no-console
      console.log(`[Main] API version check passed: ${apiVersion.version}`)
    }
  } catch {
    console.warn('[Main] Failed to check API version, continuing...')
  }

  try {
    await extensionHost.activateExtensions(builtinExtensions, {
      id: 'default',
      name: 'Default Project',
      path: '',
      description: '',
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    })
    // eslint-disable-next-line no-console
    console.log('[Main] All builtin extensions activated')
  } catch (error) {
    console.error('[Main] Failed to activate extensions:', error)
  }

  const panels = panelRegistry.getAll()
  for (const panel of panels) {
    if (panel.component) {
      try {
        app.component(panel.id, panel.component as unknown as Parameters<typeof app.component>[1])
        // eslint-disable-next-line no-console
        console.log(`[Main] Registered global component: ${panel.id}`)
      } catch (e) {
        console.warn(`[Main] Failed to register component '${panel.id}':`, e)
      }
    }
  }
  // eslint-disable-next-line no-console
  console.log(`[Main] Registered ${panels.length} panel components globally`)

  app.component('PanelHeaderActions', PanelHeaderActions)
  // eslint-disable-next-line no-console
  console.log('[Main] Registered panelHeaderActions component')

  app.component('IconTab', IconTab)
  // eslint-disable-next-line no-console
  console.log('[Main] Registered iconTab component')

  app.component('MinimalEditorTab', MinimalEditorTab)
  // eslint-disable-next-line no-console
  console.log('[Main] Registered minimalEditorTab component')

  app.mount('#app')

  const recent = appStore.recentProjects
  if (recent.length > 0) {
    const lastProject = recent[0]
    // eslint-disable-next-line no-console
    console.log('[Main] Opening last project:', lastProject)
    const result = await appStore.openProject(lastProject)
    if (!result.success) {
      console.warn('[Main] Failed to open last project:', result.error)
    } else {
      // eslint-disable-next-line no-console
      console.log('[Main] Opened last project:', lastProject)
    }
  }
}

main()
