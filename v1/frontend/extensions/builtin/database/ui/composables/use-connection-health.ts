import { invoke } from '@tauri-apps/api/core'
import { ref, onUnmounted } from 'vue'

export interface ConnectionHealth {
  status: 'online' | 'offline' | 'busy'
  latency: number
  lastCheck: Date | null
  poolSize: number
  activeConnections: number
  errorMessage: string | null
}

export function useConnectionHealth() {
  const healthMap = ref<Map<string, ConnectionHealth>>(new Map())
  const checkInterval = ref<ReturnType<typeof setInterval> | null>(null)
  const isMonitoring = ref(false)

  function getHealth(connectionId: string): ConnectionHealth | null {
    return healthMap.value.get(connectionId) || null
  }

  function isConnectionHealthy(connectionId: string): boolean {
    const health = healthMap.value.get(connectionId)
    return health?.status === 'online'
  }

  async function checkConnectionHealth(connectionId: string): Promise<ConnectionHealth> {
    const startTime = Date.now()

    try {
      await invoke('ping_connection', { connId: connectionId })

      const latency = Date.now() - startTime

      const health: ConnectionHealth = {
        status: 'online',
        latency,
        lastCheck: new Date(),
        poolSize: 0,
        activeConnections: 0,
        errorMessage: null,
      }

      healthMap.value.set(connectionId, health)
      return health
    } catch (error) {
      const health: ConnectionHealth = {
        status: 'offline',
        latency: 0,
        lastCheck: new Date(),
        poolSize: 0,
        activeConnections: 0,
        errorMessage: error instanceof Error ? error.message : 'Unknown error',
      }

      healthMap.value.set(connectionId, health)
      return health
    }
  }

  async function checkAllConnectionsHealth(connectionIds: string[]) {
    await Promise.all(connectionIds.map(id => checkConnectionHealth(id)))
  }

  function startMonitoring(connectionIds: string[], intervalMs = 30000) {
    stopMonitoring()

    isMonitoring.value = true
    checkAllConnectionsHealth(connectionIds)

    checkInterval.value = setInterval(() => {
      checkAllConnectionsHealth(connectionIds)
    }, intervalMs)
  }

  function stopMonitoring() {
    if (checkInterval.value) {
      clearInterval(checkInterval.value)
      checkInterval.value = null
    }
    isMonitoring.value = false
  }

  function clearHealth(connectionId?: string) {
    if (connectionId) {
      healthMap.value.delete(connectionId)
    } else {
      healthMap.value.clear()
    }
  }

  onUnmounted(() => {
    stopMonitoring()
  })

  return {
    healthMap,
    isMonitoring,
    getHealth,
    isConnectionHealthy,
    checkConnectionHealth,
    checkAllConnectionsHealth,
    startMonitoring,
    stopMonitoring,
    clearHealth,
  }
}
