import { ref, computed, watch } from 'vue'

import { useConnectionStore } from '@/extensions/builtin/connection/ui/stores/connection-store'
import { useRuntimeConnectionStore } from '@/extensions/builtin/connection/ui/stores/runtime-connection-store'
import type { Connection } from '@/shared/types'
import type { DatabaseType } from '@/shared/types/sql'

export interface ConnectionBindingOptions {
  initialConnectionId?: string
}

export function useConnectionBinding(_options: ConnectionBindingOptions = {}) {
  const { initialConnectionId } = _options

  const connectionStore = useConnectionStore()
  const runtimeConnectionStore = useRuntimeConnectionStore()

  const selectedConnection = ref(initialConnectionId || '')
  const runtimeConnId = ref<string>('')

  const connections = computed<Connection[]>(() => {
    return (connectionStore.connections as Connection[]) || []
  })

  const popselectOptions = computed(() => {
    return connections.value
      .filter(c => c.status === 'connected')
      .map(c => ({
        label: `${c.name} → ${c.dbType}${c.url ? ` / ${c.url}` : ''}`,
        value: c.connId,
      }))
  })

  const connectionInfoText = computed(() => {
    if (!selectedConnection.value) return ''
    const conn = connections.value.find(c => c.connId === selectedConnection.value)
    if (!conn) return ''
    return `${conn.name} → ${conn.dbType}${conn.url ? ` / ${conn.url}` : ''}`
  })

  const isDuckDbConnection = computed(() => {
    const conn = connections.value.find(c => c.connId === selectedConnection.value)
    return conn?.dbType === 'duckdb'
  })

  const currentDatabaseType = computed<DatabaseType | null>(() => {
    const conn = connections.value.find(c => c.connId === selectedConnection.value)
    return conn?.dbType ?? null
  })

  const currentDatabase = computed<string>(() => {
    const conn = connections.value.find(c => c.connId === selectedConnection.value)
    return conn?.url ?? ''
  })

  const currentConnectionName = computed<string>(() => {
    const conn = connections.value.find(c => c.connId === selectedConnection.value)
    return conn?.name ?? ''
  })

  async function ensureConnection(connId: string): Promise<boolean> {
    if (!connId) return false

    const runtimeIds = runtimeConnectionStore.runtimeConnectionIds
    if (runtimeIds.has(connId)) {
      runtimeConnId.value = connId
      return true
    }

    const conn = connectionStore.connections.find(c => c.connId === connId)
    if (!conn) {
      console.warn('[useConnectionBinding] Connection not found:', connId)
      return false
    }

    try {
      const runtimeId = await runtimeConnectionStore.establishFromConnection(conn)
      runtimeConnId.value = runtimeId ?? ''
      return true
    } catch (error) {
      console.warn('[useConnectionBinding] Failed to establish runtime connection:', error)
      if (conn.status === 'connected') {
        runtimeConnId.value = connId
        return true
      }
      return false
    }
  }

  async function waitForConnection(): Promise<boolean> {
    const connId = selectedConnection.value
    if (!connId) return false

    const runtimeIds = runtimeConnectionStore.runtimeConnectionIds
    if (runtimeIds.has(connId)) {
      runtimeConnId.value = connId
      return true
    }

    const TIMEOUT_MS = 10_000

    return new Promise<boolean>(resolve => {
      let settled = false

      const finish = (value: boolean) => {
        if (settled) return
        settled = true
        clearTimeout(timeoutId)
        stopWatch()
        resolve(value)
      }

      const checkConnection = (ids: Map<string, string>): boolean => {
        if (ids.has(connId)) {
          runtimeConnId.value = connId
          finish(true)
          return true
        }
        const conn = connectionStore.connections.find(c => c.connId === connId)
        if (conn && ids.has(conn.connId)) {
          runtimeConnId.value = conn.connId
          finish(true)
          return true
        }
        return false
      }

      const stopWatch = watch(
        () => runtimeConnectionStore.runtimeConnectionIds,
        newMap => {
          if (settled) return
          checkConnection(newMap)
        },
        { immediate: true }
      )

      setTimeout(() => {
        if (settled) return
        connectionStore.loadConnections().catch(() => {
          /* */
        })
      }, 1000)

      const timeoutId = setTimeout(() => {
        if (settled) return
        stopWatch()

        const fallbackIds = [...runtimeConnectionStore.runtimeConnectionIds.entries()]
        if (fallbackIds.length > 0) {
          runtimeConnId.value = fallbackIds[0][1]
          resolve(true)
          return
        }

        const firstConnected = connectionStore.connections.find(c => c.status === 'connected')
        if (firstConnected) {
          selectedConnection.value = firstConnected.connId
          runtimeConnectionStore
            .establishFromConnection(firstConnected)
            .then(runtimeId => {
              runtimeConnId.value = runtimeId ?? ''
              resolve(true)
            })
            .catch(() => {
              resolve(false)
            })
          return
        }

        resolve(false)
      }, TIMEOUT_MS)
    })
  }

  function onConnectionSelected(connId: string): void {
    selectedConnection.value = connId
  }

  return {
    selectedConnection,
    runtimeConnId,
    connections,
    popselectOptions,
    connectionInfoText,
    isDuckDbConnection,
    currentDatabaseType,
    currentDatabase,
    currentConnectionName,
    ensureConnection,
    waitForConnection,
    onConnectionSelected,
  }
}
