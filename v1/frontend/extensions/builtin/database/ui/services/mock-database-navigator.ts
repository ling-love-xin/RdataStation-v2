/**
 * Mock 数据库导航器服务
 *
 * 提供模拟的数据库导航数据
 */

import type { NavigatorNode } from '@/shared/types/databaseMeta'

/**
 * 加载 Mock 子节点
 */
export async function loadMockChildren(node: NavigatorNode): Promise<NavigatorNode[]> {
  // 模拟延迟
  await new Promise(resolve => setTimeout(resolve, 300))

  switch (node.type) {
    case 'connection':
      return [
        {
          id: `${node.id}-tables`,
          name: 'Tables',
          type: 'folder',
          connectionId: node.connectionId,
        },
        { id: `${node.id}-views`, name: 'Views', type: 'folder', connectionId: node.connectionId },
        {
          id: `${node.id}-indexes`,
          name: 'Indexes',
          type: 'folder',
          connectionId: node.connectionId,
        },
      ]
    case 'folder':
      if (node.name === 'Tables') {
        return [
          { id: `${node.id}-users`, name: 'users', type: 'table', connectionId: node.connectionId },
          {
            id: `${node.id}-orders`,
            name: 'orders',
            type: 'table',
            connectionId: node.connectionId,
          },
        ]
      }
      return []
    default:
      return []
  }
}

/**
 * 获取 Mock 连接列表
 */
export function getMockConnections(): NavigatorNode[] {
  return [
    {
      id: 'conn-1',
      name: 'Local MySQL',
      type: 'connection',
      metadata: { dbType: 'mysql', host: 'localhost', port: 3306 },
    },
    {
      id: 'conn-2',
      name: 'Production DB',
      type: 'connection',
      metadata: { dbType: 'postgres', host: 'prod-server', port: 5432 },
    },
  ]
}
