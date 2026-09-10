/**
 * 数据库导航树拖拽支持
 *
 * 支持表/视图拖拽到工作台
 * 支持节点排序和重组
 */

import { ref } from 'vue'

import { NodeKeyEncoder } from '../types/virtual-tree'

import type { VirtualTreeNode } from '../types/virtual-tree'

export interface IDragData {
  /** 拖拽的节点 */
  node: VirtualTreeNode
  /** 节点类型 */
  nodeType: string
  /** 连接 ID */
  connectionId: string
  /** 数据库名称 */
  dbName?: string
  /** Schema 名称 */
  schemaName?: string
  /** 表名 */
  tableName?: string
  /** 视图名 */
  viewName?: string
  /** 列名 */
  columnName?: string
}

export interface IDropTarget {
  /** 目标区域 */
  zone: 'workbench' | 'editor' | 'tree'
  /** 目标节点（如果是树内拖拽） */
  targetNode?: VirtualTreeNode
  /** 插入位置 */
  position?: 'before' | 'after' | 'inside'
}

export const DRAG_DATA_TYPE = 'application/x-rdatastation-database-node'

export function useDragDrop() {
  const isDragging = ref(false)
  const dragData = ref<IDragData | null>(null)
  const dropTarget = ref<IDropTarget | null>(null)

  /**
   * 开始拖拽
   */
  function handleDragStart(node: VirtualTreeNode, event: DragEvent): void {
    const keyParts = NodeKeyEncoder.decode(node.key)
    if (keyParts.length === 0) return

    const nodeType = keyParts[0]
    const connectionId = keyParts[1]
    const dbName = keyParts[2]
    const schemaName = keyParts[3]

    const data: IDragData = {
      node,
      nodeType,
      connectionId,
      dbName,
      schemaName,
    }

    if (nodeType === 'table') {
      data.tableName = keyParts[4]
    } else if (nodeType === 'view') {
      data.viewName = keyParts[4]
    } else if (nodeType === 'column') {
      data.tableName = keyParts[4]
      data.columnName = keyParts[5]
    }

    dragData.value = data
    isDragging.value = true

    // eslint-disable-next-line @typescript-eslint/no-non-null-assertion
    event.dataTransfer!.setData(DRAG_DATA_TYPE, JSON.stringify(data))
    // eslint-disable-next-line @typescript-eslint/no-non-null-assertion
    event.dataTransfer!.effectAllowed = 'copy'
  }

  /**
   * 拖拽中
   */
  function handleDragOver(event: DragEvent): void {
    event.preventDefault()
    const dt = event.dataTransfer
    if (dt) {
      dt.dropEffect = 'copy'
    }
  }

  /**
   * 放置
   */
  function handleDrop(event: DragEvent, target: IDropTarget): IDragData | null {
    event.preventDefault()

    try {
      const dt = event.dataTransfer
      if (!dt) return null

      const dataStr = dt.getData(DRAG_DATA_TYPE)
      if (!dataStr) return null

      const data = JSON.parse(dataStr) as IDragData
      dropTarget.value = target

      return data
    } catch {
      return null
    } finally {
      isDragging.value = false
      dragData.value = null
    }
  }

  /**
   * 拖拽结束
   */
  function handleDragEnd(): void {
    isDragging.value = false
    dragData.value = null
    dropTarget.value = null
  }

  /**
   * 检查节点是否可拖拽
   */
  function isDraggable(node: VirtualTreeNode): boolean {
    const draggableTypes = ['table', 'view', 'column', 'database', 'schema']
    return draggableTypes.includes(node.type)
  }

  /**
   * 生成 SQL 片段（用于拖拽到编辑器）
   */
  function generateSqlFragment(data: IDragData): string {
    const { nodeType, dbName, schemaName, tableName, viewName, columnName } = data

    if (nodeType === 'table') {
      return `SELECT * FROM ${dbName}.${schemaName}.${tableName}`
    } else if (nodeType === 'view') {
      return `SELECT * FROM ${dbName}.${schemaName}.${viewName}`
    } else if (nodeType === 'column') {
      return `${tableName}.${columnName}`
    } else if (nodeType === 'catalog') {
      return `USE ${dbName}`
    } else if (nodeType === 'schema') {
      return `SET search_path TO ${schemaName}`
    }

    return ''
  }

  return {
    isDragging,
    dragData,
    dropTarget,
    handleDragStart,
    handleDragOver,
    handleDrop,
    handleDragEnd,
    isDraggable,
    generateSqlFragment,
  }
}
