/**
 * 拖拽排序逻辑
 *
 * 实现节点的拖拽排序功能，支持连接和分组的拖拽
 */

import { ref } from 'vue'

export interface DragState {
  isDragging: boolean
  draggedNodeId: string | null
  draggedNodeType: 'connection' | 'group' | null
  targetNodeId: string | null
  targetNodeType: 'connection' | 'group' | null
  dropPosition: 'before' | 'after' | 'inside' | null
}

export function useDragSort() {
  const dragState = ref<DragState>({
    isDragging: false,
    draggedNodeId: null,
    draggedNodeType: null,
    targetNodeId: null,
    targetNodeType: null,
    dropPosition: null,
  })

  function startDrag(nodeId: string, nodeType: 'connection' | 'group') {
    dragState.value = {
      isDragging: true,
      draggedNodeId: nodeId,
      draggedNodeType: nodeType,
      targetNodeId: null,
      targetNodeType: null,
      dropPosition: null,
    }
  }

  function updateDrag(
    targetNodeId: string | null,
    targetNodeType: 'connection' | 'group' | null,
    position: 'before' | 'after' | 'inside' | null
  ) {
    if (!dragState.value.isDragging) return

    dragState.value.targetNodeId = targetNodeId
    dragState.value.targetNodeType = targetNodeType
    dragState.value.dropPosition = position
  }

  function endDrag() {
    const state = { ...dragState.value }
    dragState.value = {
      isDragging: false,
      draggedNodeId: null,
      draggedNodeType: null,
      targetNodeId: null,
      targetNodeType: null,
      dropPosition: null,
    }
    return state
  }

  function cancelDrag() {
    dragState.value = {
      isDragging: false,
      draggedNodeId: null,
      draggedNodeType: null,
      targetNodeId: null,
      targetNodeType: null,
      dropPosition: null,
    }
  }

  return {
    dragState,
    startDrag,
    updateDrag,
    endDrag,
    cancelDrag,
  }
}
