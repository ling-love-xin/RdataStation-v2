import { ref } from 'vue'

import { useNotification } from './use-notification'
import { useDatabaseNavigatorStore } from '../stores/database-navigator-store'

import type { TreeOption } from 'naive-ui'

export function useBatchOperations() {
  const navigatorStore = useDatabaseNavigatorStore()
  const { success, error, warning } = useNotification()

  const selectedNodes = ref<string[]>([])
  const isBatchMode = ref(false)

  function toggleBatchMode() {
    isBatchMode.value = !isBatchMode.value
    if (!isBatchMode.value) {
      selectedNodes.value = []
    }
  }

  function toggleNodeSelection(nodeKey: string) {
    const index = selectedNodes.value.indexOf(nodeKey)
    if (index === -1) {
      selectedNodes.value.push(nodeKey)
    } else {
      selectedNodes.value.splice(index, 1)
    }
  }

  function selectAll(connectionId: string, treeData: TreeOption[]) {
    const allKeys: string[] = []

    const collectKeys = (nodes: TreeOption[]) => {
      nodes.forEach(node => {
        const key = node.key as string
        if (key.startsWith(`conn_${connectionId}`)) {
          allKeys.push(key)
        }
        if (node.children && node.children.length > 0) {
          collectKeys(node.children)
        }
      })
    }

    collectKeys(treeData)
    selectedNodes.value = allKeys
  }

  function clearSelection() {
    selectedNodes.value = []
  }

  async function batchRefresh(connectionIds: string[]) {
    if (connectionIds.length === 0) {
      warning('请选择要刷新的连接')
      return
    }

    let successCount = 0
    let failCount = 0

    for (const connId of connectionIds) {
      try {
        await navigatorStore.refreshMetadata(connId)
        successCount++
      } catch (e) {
        failCount++
        console.error(`刷新连接 ${connId} 失败:`, e)
      }
    }

    if (failCount === 0) {
      success('批量刷新完成', `成功刷新 ${successCount} 个连接`)
    } else {
      error('批量刷新部分失败', `成功 ${successCount} 个，失败 ${failCount} 个`)
    }
  }

  async function batchDisconnect(connectionIds: string[]) {
    if (connectionIds.length === 0) {
      warning('请选择要断开的连接')
      return
    }

    for (const connId of connectionIds) {
      await navigatorStore.disconnectConnection(connId)
    }

    success('批量断开完成', `已断开 ${connectionIds.length} 个连接`)
    selectedNodes.value = []
  }

  function getSelectedConnectionIds(): string[] {
    return selectedNodes.value.filter(key => key.startsWith('conn_')).map(key => key.split('_')[1])
  }

  return {
    selectedNodes,
    isBatchMode,
    toggleBatchMode,
    toggleNodeSelection,
    selectAll,
    clearSelection,
    batchRefresh,
    batchDisconnect,
    getSelectedConnectionIds,
  }
}
