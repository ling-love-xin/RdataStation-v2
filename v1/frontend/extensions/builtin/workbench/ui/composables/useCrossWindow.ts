import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { emit } from '@tauri-apps/api/event'

export enum CrossWindowEvent {
  PopoutTransfer = 'editor:popout-transfer',
  MergeTransfer = 'editor:merge-transfer',
  StateSync = 'editor:state-sync',
  WindowReady = 'editor:window-ready',
}

export interface PopoutPayload {
  filePath: string
  fileName: string
  language: string
  content: string
  stateJSON?: Record<string, unknown>
  connectionId: string
  databaseName: string
}

export interface MergePayload {
  filePath: string
  content: string
  stateJSON?: Record<string, unknown>
  isDirty: boolean
}

export interface StateSyncPayload {
  filePath: string
  content: string
  isDirty: boolean
  cursorLine: number
  cursorCol: number
}

export function sendPopoutTransfer(payload: PopoutPayload): void {
  emit(CrossWindowEvent.PopoutTransfer, payload).catch(e => {
    console.warn('[CrossWindow] Failed to send popout transfer:', e)
  })
}

export function sendMergeTransfer(payload: MergePayload): void {
  emit(CrossWindowEvent.MergeTransfer, payload).catch(e => {
    console.warn('[CrossWindow] Failed to send merge transfer:', e)
  })
}

export function sendWindowReady(): void {
  emit(CrossWindowEvent.WindowReady, undefined).catch(e => {
    console.warn('[CrossWindow] Failed to send window ready:', e)
  })
}

export function sendStateSync(payload: StateSyncPayload): void {
  emit(CrossWindowEvent.StateSync, payload).catch(e => {
    console.warn('[CrossWindow] Failed to send state sync:', e)
  })
}

export function listenPopoutTransfer(
  handler: (payload: PopoutPayload) => void
): Promise<UnlistenFn> {
  return listen<PopoutPayload>(CrossWindowEvent.PopoutTransfer, event => {
    handler(event.payload)
  })
}

export function listenMergeTransfer(handler: (payload: MergePayload) => void): Promise<UnlistenFn> {
  return listen<MergePayload>(CrossWindowEvent.MergeTransfer, event => {
    handler(event.payload)
  })
}

export function listenWindowReady(handler: () => void): Promise<UnlistenFn> {
  return listen(CrossWindowEvent.WindowReady, () => {
    handler()
  })
}

export function listenStateSync(handler: (payload: StateSyncPayload) => void): Promise<UnlistenFn> {
  return listen<StateSyncPayload>(CrossWindowEvent.StateSync, event => {
    handler(event.payload)
  })
}
