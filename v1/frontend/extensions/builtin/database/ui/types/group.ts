/**
 * 分组类型定义
 */

export interface ConnectionGroup {
  id: string
  name: string
  description?: string
  connectionIds: string[]
  expanded: boolean
  color?: string
  createdAt: number
  updatedAt: number
}

export interface GroupState {
  groups: ConnectionGroup[]
  expandedGroups: Set<string>
}

export const GROUP_COLORS = [
  '#4f46e5',
  '#3b82f6',
  '#10b981',
  '#f59e0b',
  '#ef4444',
  '#8b5cf6',
  '#ec4899',
  '#06b6d4',
]

export function generateGroupId(): string {
  return `group_${Date.now()}_${Math.random().toString(36).substr(2, 9)}`
}
