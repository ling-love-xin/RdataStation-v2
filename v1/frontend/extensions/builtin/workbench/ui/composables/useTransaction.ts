import { createDiscreteApi } from 'naive-ui'
import { ref, type Ref } from 'vue'

import * as queryService from '@/extensions/builtin/query/ui/services/query'

export function useTransaction(runtimeConnId: Ref<string>) {
  const { message } = createDiscreteApi(['message'])
  const inTransaction = ref(false)

  async function beginTransaction(): Promise<void> {
    const connId = runtimeConnId.value
    if (!connId) {
      message.warning('No active connection')
      return
    }
    try {
      const status = await queryService.beginTransaction(connId)
      inTransaction.value = status.is_in_transaction
    } catch {
      message.error('Failed to begin transaction')
    }
  }

  async function commitTransaction(): Promise<void> {
    const connId = runtimeConnId.value
    if (!connId) return
    try {
      await queryService.commitTransaction(connId)
      inTransaction.value = false
      message.success('Transaction committed')
    } catch {
      message.error('Failed to commit transaction')
    }
  }

  async function rollbackTransaction(): Promise<void> {
    const connId = runtimeConnId.value
    if (!connId) return
    try {
      await queryService.rollbackTransaction(connId)
      inTransaction.value = false
      message.success('Transaction rolled back')
    } catch {
      message.error('Failed to rollback transaction')
    }
  }

  return {
    inTransaction,
    beginTransaction,
    commitTransaction,
    rollbackTransaction,
  }
}
