/**
 * useVirtualTree 虚拟树控制器单元测试
 *
 * 测试 setRootNodes、toggleNode、selectNode、clearAll 等核心功能。
 */
import { describe, expect, it, vi } from 'vitest'

import { useVirtualTree } from '../use-virtual-tree'

import type { VirtualTreeNode } from '../../types/virtual-tree'

// ==================== 测试工具函数 ====================

function createNode(key: string, overrides: Partial<VirtualTreeNode> = {}): VirtualTreeNode {
  return {
    key,
    level: 0,
    isExpanded: false,
    isLeaf: false,
    label: key,
    type: 'connection',
    data: {},
    parentId: null,
    childCount: 0,
    ...overrides,
  }
}

function createRootNodes(count: number): VirtualTreeNode[] {
  return Array.from({ length: count }, (_, i) => createNode(`root-${i}`, { label: `Root ${i}` }))
}

function createChildNodes(parentKey: string, count: number, level: number): VirtualTreeNode[] {
  return Array.from({ length: count }, (_, i) =>
    createNode(`${parentKey}/child-${i}`, {
      label: `Child ${i} of ${parentKey}`,
      level,
      parentId: parentKey,
    })
  )
}

// ==================== 测试套件 ====================

describe('useVirtualTree', () => {
  describe('setRootNodes', () => {
    it('设置根节点', () => {
      const { setRootNodes, flatNodes, rootKeys } = useVirtualTree({
        onLoadChildren: vi.fn(),
      })

      const nodes = createRootNodes(3)
      setRootNodes(nodes)

      expect(flatNodes.value).toHaveLength(3)
      expect(rootKeys.value).toEqual(['root-0', 'root-1', 'root-2'])
      expect(flatNodes.value[0].label).toBe('Root 0')
      expect(flatNodes.value[1].label).toBe('Root 1')
      expect(flatNodes.value[2].label).toBe('Root 2')
    })

    it('恢复展开状态', async () => {
      const onLoadChildren = vi.fn().mockResolvedValue(createChildNodes('root-0', 2, 1))
      const { setRootNodes, toggleNode, flatNodes } = useVirtualTree({
        onLoadChildren,
      })

      // 第一轮：设置根节点并展开 root-0
      const nodes = createRootNodes(3)
      setRootNodes(nodes)
      await toggleNode(flatNodes.value[0])

      // 展开后应有 5 个节点（3 根 + 2 子）
      expect(flatNodes.value).toHaveLength(5)

      // 第二轮：重新设置根节点，之前展开的节点应恢复展开状态
      const newNodes = createRootNodes(3)
      setRootNodes(newNodes)

      // 之前展开的节点应标记为展开但未加载
      const restoredNode = flatNodes.value[0]
      expect(restoredNode.isExpanded).toBe(true)
      expect(restoredNode.isLoaded).toBe(false)
      expect(restoredNode.childCount).toBe(0)

      // 未展开的节点保持原样
      expect(flatNodes.value[1].isExpanded).toBe(false)
      expect(flatNodes.value[2].isExpanded).toBe(false)
    })
  })

  describe('toggleNode', () => {
    it('展开节点并加载子节点', async () => {
      const onLoadChildren = vi.fn().mockResolvedValue(createChildNodes('root-0', 2, 1))
      const { setRootNodes, toggleNode, flatNodes } = useVirtualTree({
        onLoadChildren,
      })

      setRootNodes(createRootNodes(2))
      const node = flatNodes.value[0]

      await toggleNode(node)

      // 应调用 onLoadChildren
      expect(onLoadChildren).toHaveBeenCalledTimes(1)
      expect(onLoadChildren).toHaveBeenCalledWith(
        expect.objectContaining({ key: 'root-0', isExpanded: true })
      )

      // 子节点应插入到正确位置
      expect(flatNodes.value).toHaveLength(4)
      expect(flatNodes.value[0].key).toBe('root-0')
      expect(flatNodes.value[1].key).toBe('root-0/child-0')
      expect(flatNodes.value[2].key).toBe('root-0/child-1')
      expect(flatNodes.value[3].key).toBe('root-1')
    })

    it('收起节点', async () => {
      const onLoadChildren = vi.fn().mockResolvedValue(createChildNodes('root-0', 2, 1))
      const { setRootNodes, toggleNode, flatNodes } = useVirtualTree({
        onLoadChildren,
      })

      setRootNodes(createRootNodes(2))

      // 展开
      await toggleNode(flatNodes.value[0])
      expect(flatNodes.value).toHaveLength(4)

      // 收起
      await toggleNode(flatNodes.value[0])
      expect(flatNodes.value).toHaveLength(2)
      expect(flatNodes.value[0].isExpanded).toBe(false)
    })

    it('叶子节点不展开', async () => {
      const onLoadChildren = vi.fn()
      const { setRootNodes, toggleNode, flatNodes } = useVirtualTree({
        onLoadChildren,
      })

      const leafNode = createNode('leaf-0', { isLeaf: true })
      setRootNodes([leafNode])

      await toggleNode(flatNodes.value[0])

      // 叶子节点不应触发加载
      expect(onLoadChildren).not.toHaveBeenCalled()
      expect(flatNodes.value[0].isExpanded).toBe(false)
    })

    it('加载中不重复调用', async () => {
      // 使用一个永不 resolve 的 promise 来模拟加载中状态
      let resolvePromise: (value: VirtualTreeNode[]) => void = () => {}
      const onLoadChildren = vi.fn().mockImplementation(
        () =>
          new Promise<VirtualTreeNode[]>(resolve => {
            resolvePromise = resolve
          })
      )
      const { setRootNodes, toggleNode, flatNodes } = useVirtualTree({
        onLoadChildren,
      })

      setRootNodes(createRootNodes(2))

      // 第一次展开（不 await，让它处于加载中）
      const togglePromise = toggleNode(flatNodes.value[0])

      // 第二次展开同一节点，应被阻止
      await toggleNode(flatNodes.value[0])

      // resolve 第一次加载
      resolvePromise(createChildNodes('root-0', 1, 1))
      await togglePromise

      // onLoadChildren 只应被调用一次
      expect(onLoadChildren).toHaveBeenCalledTimes(1)
    })

    it('已加载子节点后直接展开不重新加载', async () => {
      const onLoadChildren = vi.fn().mockResolvedValue(createChildNodes('root-0', 2, 1))
      const { setRootNodes, toggleNode, flatNodes } = useVirtualTree({
        onLoadChildren,
      })

      setRootNodes(createRootNodes(2))

      // 第一次展开：加载子节点
      await toggleNode(flatNodes.value[0])
      expect(onLoadChildren).toHaveBeenCalledTimes(1)
      expect(flatNodes.value).toHaveLength(4)
      expect(flatNodes.value[0].isLoaded).toBe(true)
      expect(flatNodes.value[0].childCount).toBe(2)

      // 收起
      await toggleNode(flatNodes.value[0])
      expect(flatNodes.value).toHaveLength(2)
      expect(flatNodes.value[0].isExpanded).toBe(false)
      // isLoaded 和 childCount 在收起时保持不变
      expect(flatNodes.value[0].isLoaded).toBe(true)
      expect(flatNodes.value[0].childCount).toBe(2)

      // 再次展开：isLoaded=true && childCount>0，不重新调用 onLoadChildren
      await toggleNode(flatNodes.value[0])
      expect(onLoadChildren).toHaveBeenCalledTimes(1)
      expect(flatNodes.value[0].isExpanded).toBe(true)
    })
  })

  describe('childCount', () => {
    it('在加载子节点后更新', async () => {
      const onLoadChildren = vi.fn().mockResolvedValue(createChildNodes('root-0', 5, 1))
      const { setRootNodes, toggleNode, flatNodes } = useVirtualTree({
        onLoadChildren,
      })

      setRootNodes(createRootNodes(1))

      // 展开前 childCount 为 0
      expect(flatNodes.value[0].childCount).toBe(0)

      await toggleNode(flatNodes.value[0])

      // 展开后 childCount 应为 5
      expect(flatNodes.value[0].childCount).toBe(5)
    })
  })

  describe('selectNode', () => {
    it('选中节点', () => {
      const onSelect = vi.fn()
      const { setRootNodes, selectNode, flatNodes, selectedKey, selectedNode } = useVirtualTree({
        onLoadChildren: vi.fn(),
        onSelect,
      })

      setRootNodes(createRootNodes(3))

      selectNode(flatNodes.value[1])

      expect(selectedKey.value).toBe('root-1')
      expect(selectedNode.value).not.toBeNull()
      expect(selectedNode.value!.key).toBe('root-1')
      expect(onSelect).toHaveBeenCalledWith(flatNodes.value[1])
    })
  })

  describe('clearAll', () => {
    it('清除所有节点', async () => {
      const onLoadChildren = vi.fn().mockResolvedValue(createChildNodes('root-0', 2, 1))
      const {
        setRootNodes,
        toggleNode,
        selectNode,
        clearAll,
        flatNodes,
        rootKeys,
        selectedKey,
        selectedNode,
      } = useVirtualTree({ onLoadChildren })

      setRootNodes(createRootNodes(3))
      await toggleNode(flatNodes.value[0])
      selectNode(flatNodes.value[1])

      // 确认有数据
      expect(flatNodes.value.length).toBeGreaterThan(0)
      expect(rootKeys.value.length).toBeGreaterThan(0)
      expect(selectedKey.value).not.toBeNull()

      clearAll()

      expect(flatNodes.value).toEqual([])
      expect(rootKeys.value).toEqual([])
      expect(selectedKey.value).toBeNull()
      expect(selectedNode.value).toBeNull()
    })
  })

  describe('collapse and re-expand', () => {
    it('收起再展开后子节点恢复', async () => {
      const onLoadChildren = vi.fn().mockResolvedValue(createChildNodes('root-0', 3, 1))
      const { setRootNodes, toggleNode, flatNodes } = useVirtualTree({ onLoadChildren })

      setRootNodes(createRootNodes(1))
      const rootNode = flatNodes.value[0]

      // 第一次展开
      await toggleNode(rootNode)
      expect(flatNodes.value).toHaveLength(4) // 1 root + 3 children
      expect(onLoadChildren).toHaveBeenCalledTimes(1)

      // 收起
      await toggleNode(flatNodes.value[0])
      expect(flatNodes.value).toHaveLength(1) // only root
      expect(flatNodes.value[0].isExpanded).toBe(false)

      // 重新展开：子节点应从 nodeMap 恢复，不重新加载
      await toggleNode(flatNodes.value[0])
      expect(flatNodes.value).toHaveLength(4) // 1 root + 3 children restored
      expect(onLoadChildren).toHaveBeenCalledTimes(1) // 不再调用 onLoadChildren
    })

    it('多次收起展开后子节点保持正确', async () => {
      const onLoadChildren = vi.fn().mockResolvedValue(createChildNodes('root-0', 2, 1))
      const { setRootNodes, toggleNode, flatNodes } = useVirtualTree({ onLoadChildren })

      setRootNodes(createRootNodes(1))

      // 展开 → 收起 → 展开 → 收起 → 展开
      await toggleNode(flatNodes.value[0])
      expect(flatNodes.value).toHaveLength(3)
      expect(onLoadChildren).toHaveBeenCalledTimes(1)

      await toggleNode(flatNodes.value[0])
      expect(flatNodes.value).toHaveLength(1)

      await toggleNode(flatNodes.value[0])
      expect(flatNodes.value).toHaveLength(3)
      expect(onLoadChildren).toHaveBeenCalledTimes(1) // 仍然只调用一次

      await toggleNode(flatNodes.value[0])
      expect(flatNodes.value).toHaveLength(1)

      await toggleNode(flatNodes.value[0])
      expect(flatNodes.value).toHaveLength(3)
      expect(onLoadChildren).toHaveBeenCalledTimes(1) // 始终只调用一次
    })
  })

  describe('maxNodes', () => {
    it('上限阻止加载', async () => {
      const onLoadChildren = vi.fn().mockResolvedValue(createChildNodes('root-0', 10, 1))
      const { setRootNodes, toggleNode, flatNodes } = useVirtualTree({
        onLoadChildren,
        maxNodes: 5, // 设置很小的上限
      })

      // 创建 3 个根节点，加上 10 个子节点 = 13 > 5
      setRootNodes(createRootNodes(3))
      await toggleNode(flatNodes.value[0])

      // 子节点不应被加载（3 + 10 > 5）
      expect(flatNodes.value).toHaveLength(3)
      // 节点应恢复为未展开状态
      expect(flatNodes.value[0].isExpanded).toBe(false)
      expect(flatNodes.value[0].isLoading).toBe(false)
    })
  })
})
