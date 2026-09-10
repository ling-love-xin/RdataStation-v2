/**
 * 分组管理逻辑
 *
 * 实现连接分组的创建、编辑、删除和管理功能
 */

import { ref, watch } from 'vue'

import { GROUP_COLORS, generateGroupId } from '../types/group'

import type { ConnectionGroup } from '../types/group'

const STORAGE_KEY = 'rdata-station-database-groups'

export function useGroupManager() {
  const groups = ref<ConnectionGroup[]>([])

  loadGroups()

  watch(
    groups,
    newGroups => {
      saveGroups(newGroups)
    },
    { deep: true }
  )

  function loadGroups() {
    try {
      const stored = localStorage.getItem(STORAGE_KEY)
      if (stored) {
        groups.value = JSON.parse(stored)
      }
    } catch {
      groups.value = []
    }
  }

  function saveGroups(groupsToSave: ConnectionGroup[]) {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(groupsToSave))
    } catch (error) {
      console.error('保存分组失败:', error)
    }
  }

  function createGroup(name: string, description?: string): ConnectionGroup {
    const colorIndex = groups.value.length % GROUP_COLORS.length
    const group: ConnectionGroup = {
      id: generateGroupId(),
      name,
      description,
      connectionIds: [],
      expanded: true,
      color: GROUP_COLORS[colorIndex],
      createdAt: Date.now(),
      updatedAt: Date.now(),
    }

    groups.value.push(group)
    return group
  }

  function updateGroup(
    groupId: string,
    updates: Partial<Pick<ConnectionGroup, 'name' | 'description' | 'color' | 'expanded'>>
  ) {
    const group = groups.value.find(g => g.id === groupId)
    if (group) {
      Object.assign(group, updates)
      group.updatedAt = Date.now()
    }
  }

  function deleteGroup(groupId: string) {
    const index = groups.value.findIndex(g => g.id === groupId)
    if (index !== -1) {
      groups.value.splice(index, 1)
    }
  }

  function addConnectionToGroup(groupId: string, connectionId: string) {
    const group = groups.value.find(g => g.id === groupId)
    if (group && !group.connectionIds.includes(connectionId)) {
      group.connectionIds.push(connectionId)
      group.updatedAt = Date.now()
    }
  }

  function removeConnectionFromGroup(groupId: string, connectionId: string) {
    const group = groups.value.find(g => g.id === groupId)
    if (group) {
      const index = group.connectionIds.indexOf(connectionId)
      if (index !== -1) {
        group.connectionIds.splice(index, 1)
        group.updatedAt = Date.now()
      }
    }
  }

  function moveConnectionToGroup(
    fromGroupId: string | null,
    toGroupId: string,
    connectionId: string
  ) {
    if (fromGroupId) {
      removeConnectionFromGroup(fromGroupId, connectionId)
    }
    addConnectionToGroup(toGroupId, connectionId)
  }

  function getGroupById(groupId: string): ConnectionGroup | undefined {
    return groups.value.find(g => g.id === groupId)
  }

  function getGroupsForConnection(connectionId: string): ConnectionGroup[] {
    return groups.value.filter(g => g.connectionIds.includes(connectionId))
  }

  function isConnectionInGroup(connectionId: string, groupId: string): boolean {
    const group = getGroupById(groupId)
    return group ? group.connectionIds.includes(connectionId) : false
  }

  function toggleGroupExpanded(groupId: string) {
    const group = groups.value.find(g => g.id === groupId)
    if (group) {
      group.expanded = !group.expanded
    }
  }

  function hasGroups(): boolean {
    return groups.value.length > 0
  }

  function clearAllGroups() {
    groups.value = []
  }

  return {
    groups,
    createGroup,
    updateGroup,
    deleteGroup,
    addConnectionToGroup,
    removeConnectionFromGroup,
    moveConnectionToGroup,
    getGroupById,
    getGroupsForConnection,
    isConnectionInGroup,
    toggleGroupExpanded,
    hasGroups,
    clearAllGroups,
  }
}
