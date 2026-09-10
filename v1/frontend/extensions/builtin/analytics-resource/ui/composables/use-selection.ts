import type { Ref } from 'vue'

export function useSelection(
  selectedResources: Ref<string[]>,
  selectedScope: Ref<string | null>,
  selectedType: Ref<string | null>,
  selectedFolderId: Ref<string | null>
) {
  function selectResource(id: string, multiple = false) {
    if (multiple) {
      const index = selectedResources.value.indexOf(id)
      if (index !== -1) {
        selectedResources.value.splice(index, 1)
      } else {
        selectedResources.value.push(id)
      }
    } else {
      selectedResources.value = [id]
    }
  }

  function clearSelection() {
    selectedResources.value = []
  }

  function selectScope(scope: string | null) {
    selectedScope.value = scope
  }

  function selectType(type: string | null) {
    selectedType.value = type
  }

  function selectFolder(folderId: string | null) {
    selectedFolderId.value = folderId
  }

  return {
    selectedResources,
    selectedScope,
    selectedType,
    selectedFolderId,
    selectResource,
    clearSelection,
    selectScope,
    selectType,
    selectFolder,
  }
}
