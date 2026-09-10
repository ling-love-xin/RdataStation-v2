import {
  WorkbenchEvent,
  dispatchWorkbenchEvent,
} from '@/extensions/builtin/workbench/ui/constants/workbench-events'
import type {
  Disposable,
  ExtensionContext,
  ExtensionAPI,
  ExtensionModule,
} from '@/extensions/core/types'

const activate = (context: ExtensionContext): ExtensionAPI => {
  // eslint-disable-next-line no-console
  console.log('[Settings] Activating for project:', context.project.name)

  const openSettingsDisposable = context.commands.registerCommand('settings.open', () => {
    dispatchWorkbenchEvent(WorkbenchEvent.OpenSettings)
  })

  const disposables: Disposable[] = [openSettingsDisposable]

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

    dispose: () => {
      disposables.forEach(d => d.dispose())
    },
  }
}

const deactivate = (): void => {
  // eslint-disable-next-line no-console
  console.log('[Settings] Deactivated')
}

const extension: ExtensionModule = {
  activate: activate as (context: ExtensionContext) => ExtensionAPI,
  deactivate,
}

export default extension
