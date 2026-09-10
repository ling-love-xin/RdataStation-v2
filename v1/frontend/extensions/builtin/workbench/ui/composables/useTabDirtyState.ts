import { reactive } from 'vue'

const dirtyPanels = reactive(new Set<string>())

export function useTabDirtyState() {
  function setDirty(panelId: string, dirty: boolean) {
    if (dirty) {
      dirtyPanels.add(panelId)
    } else {
      dirtyPanels.delete(panelId)
    }
  }

  function isDirty(panelId: string): boolean {
    return dirtyPanels.has(panelId)
  }

  function clearPanel(panelId: string) {
    dirtyPanels.delete(panelId)
  }

  return {
    dirtyPanels,
    setDirty,
    isDirty,
    clearPanel,
  }
}
