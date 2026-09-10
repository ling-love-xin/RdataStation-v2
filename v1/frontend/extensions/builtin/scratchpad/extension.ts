import { invoke } from '@tauri-apps/api/core'

import ScratchpadPanel from './ui/components/ScratchpadPanel.vue'

import type { ExtensionContext, ExtensionAPI, ExtensionModule, Disposable } from '../../core/types'

interface ScratchpadExtensionAPI extends ExtensionAPI {
  // eslint-disable-next-line @typescript-eslint/no-empty-object-type
  scratchpad: {}
}

let disposables: Disposable[] = []

const activate = (context: ExtensionContext): ScratchpadExtensionAPI => {
  // eslint-disable-next-line no-console
  console.log('[Scratchpad] Activating for project:', context.project.name)

  const panelDisposable = context.window.registerViewProvider('scratchpad', {
    component: ScratchpadPanel,
    title: '草稿箱',
    location: 'left',
    icon: 'FileText',
    order: 4,
  })

  let storeInitialized = false

  const initStore = async (projectPath: string): Promise<void> => {
    if (!projectPath) {
      // eslint-disable-next-line no-console
      console.debug('[Scratchpad] No project path, store init deferred')
      return
    }
    const maxRetries = 3
    for (let attempt = 1; attempt <= maxRetries; attempt++) {
      try {
        await invoke('init_scratchpad_store', { projectPath })
        storeInitialized = true
        // eslint-disable-next-line no-console
        console.log('[Scratchpad] Store initialized for:', projectPath)
        return
      } catch (e) {
        console.error(`[Scratchpad] Store init attempt ${attempt} failed:`, e)
        if (attempt < maxRetries) {
          await new Promise(resolve => setTimeout(resolve, 500 * attempt))
        }
      }
    }
    console.error('[Scratchpad] All store init attempts failed')
  }

  initStore(context.project.path).then(() => {
    // eslint-disable-next-line no-console
    console.log('[Scratchpad] Initial activation init complete')
  })

  const handleProjectSwitch = (event: Event): void => {
    const detail = (event as CustomEvent).detail as { project?: { path?: string } } | undefined
    const newPath = detail?.project?.path
    if (newPath) {
      // eslint-disable-next-line no-console
      console.log('[Scratchpad] Project switched to:', newPath)
      initStore(newPath).then(() => {
        // eslint-disable-next-line no-console
        console.log('[Scratchpad] Store re-initialized for project switch')
      })
    }
  }

  window.addEventListener('project-switched', handleProjectSwitch)

  disposables = [
    panelDisposable,
    {
      dispose: () => window.removeEventListener('project-switched', handleProjectSwitch),
    },
  ]

  return {
    version: '1.0.0',
    project: context.project,
    commands: context.commands,
    window: context.window,
    workspace: context.workspace,
    database: context.database,
    sqlEditor: context.sqlEditor,
    events: context.events,
    configuration: context.configuration,
    utils: context.utils,

    scratchpad: {
      isInitialized: () => storeInitialized,
    },

    dispose: () => {
      disposables.forEach(d => d.dispose())
    },
  }
}

const deactivate = async (): Promise<void> => {
  // eslint-disable-next-line no-console
  console.log('[Scratchpad] Deactivating')

  for (const d of disposables) {
    d.dispose()
  }
  // eslint-disable-next-line no-console
  console.log('[Scratchpad] Deactivated')
}

const extension: ExtensionModule = {
  activate: activate as (context: ExtensionContext) => ExtensionAPI,
  deactivate,
}

export default extension
