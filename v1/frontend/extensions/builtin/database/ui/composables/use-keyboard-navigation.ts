/**
 * 虚拟树键盘导航
 *
 * 支持方向键、Enter、Space 等键盘操作
 */

import { type Ref } from 'vue'

import type { VirtualTreeNode } from '../types/virtual-tree'

export interface UseKeyboardNavigationOptions {
  /** 扁平化的节点数组 */
  nodes: Ref<VirtualTreeNode[]>
  /** 选中的节点 key */
  selectedKey: Ref<string | null>
  /** 选择节点的回调 */
  onSelect: (node: VirtualTreeNode) => void
  /** 展开/收起节点的回调 */
  onToggle: (node: VirtualTreeNode) => void
}

export function useKeyboardNavigation(options: UseKeyboardNavigationOptions) {
  const { nodes, selectedKey, onSelect, onToggle } = options

  /**
   * 处理键盘事件
   */
  function handleKeydown(event: KeyboardEvent) {
    if (!selectedKey.value) {
      // 如果没有选中节点，按 Enter 或 Space 选中第一个节点
      if (event.key === 'Enter' || event.key === ' ') {
        event.preventDefault()
        if (nodes.value.length > 0) {
          selectNode(nodes.value[0])
        }
      }
      return
    }

    const currentIndex = nodes.value.findIndex(n => n.key === selectedKey.value)
    if (currentIndex === -1) return

    const currentNode = nodes.value[currentIndex]

    switch (event.key) {
      case 'ArrowDown':
        event.preventDefault()
        // 选中下一个可见节点
        if (currentIndex < nodes.value.length - 1) {
          selectNode(nodes.value[currentIndex + 1])
        }
        break

      case 'ArrowUp':
        event.preventDefault()
        // 选中上一个可见节点
        if (currentIndex > 0) {
          selectNode(nodes.value[currentIndex - 1])
        }
        break

      case 'ArrowRight':
        event.preventDefault()
        // 如果节点未展开，则展开；否则选中第一个子节点
        if (!currentNode.isExpanded && !currentNode.isLeaf) {
          onToggle(currentNode)
        } else if (currentNode.isExpanded && currentIndex < nodes.value.length - 1) {
          const nextNode = nodes.value[currentIndex + 1]
          if (nextNode.level > currentNode.level) {
            selectNode(nextNode)
          }
        }
        break

      case 'ArrowLeft':
        event.preventDefault()
        // 如果节点已展开，则收起；否则选中父节点
        if (currentNode.isExpanded) {
          onToggle(currentNode)
        } else if (currentNode.parentId) {
          const parentNode = nodes.value.find(n => n.key === currentNode.parentId)
          if (parentNode) {
            selectNode(parentNode)
          }
        }
        break

      case 'Enter':
        event.preventDefault()
        // 双击效果：展开/收起
        onToggle(currentNode)
        break

      case ' ':
        event.preventDefault()
        // 空格：选择节点
        onSelect(currentNode)
        break

      case 'Home':
        event.preventDefault()
        // 选中第一个节点
        if (nodes.value.length > 0) {
          selectNode(nodes.value[0])
        }
        break

      case 'End':
        event.preventDefault()
        // 选中最后一个节点
        if (nodes.value.length > 0) {
          selectNode(nodes.value[nodes.value.length - 1])
        }
        break
    }
  }

  /**
   * 选中节点
   */
  function selectNode(node: VirtualTreeNode) {
    selectedKey.value = node.key
    onSelect(node)
  }

  return {
    handleKeydown,
  }
}
