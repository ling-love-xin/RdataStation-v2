import { describe, it, expect, vi } from 'vitest'

// Mock editor-state before importing the service
vi.mock('../../manager/editor-state', () => ({
  activeFileInfo: { value: null },
  runtimeState: {
    startExecution: vi.fn(),
    finishExecution: vi.fn(),
    cancelExecution: vi.fn(),
  },
  SQL_LOG_TRUNCATE_LENGTH: 500,
}))

vi.mock('../../manager/instance-service', () => ({
  getEditorView: vi.fn(() => null),
}))

vi.mock('../../manager/result-set-manager', () => ({
  createResultSet: vi.fn(() => 'result_1'),
  setActiveResultIndex: vi.fn(),
}))

vi.mock('../sql-editor-service', () => ({
  formatSql: vi.fn(async (sql: string) => sql),
  validateSql: vi.fn(async () => []),
  executeDuckDBAccelerated: vi.fn(async () => ({})),
  generateAttachName: vi.fn((name: string) => `ext_${name}`),
  rewriteDuckDBSQL: vi.fn((sql: string) => sql),
  setErrorMarker: vi.fn(),
  clearErrorMarkers: vi.fn(),
}))

vi.mock('../cm-sql-extensions', () => ({
  setEditorDiagnostics: vi.fn(),
}))

import { runtimeState } from '../../manager/editor-state'
import {
  executeCurrentSQL,
  executeNewTabSQL,
  executeDuckDBAccelerated,
  cancelExecution,
  formatActiveSQL,
  validateActiveSQL,
  toggleComment,
} from '../sql-execution-service'

describe('sql-execution-service', () => {
  describe('module exports', () => {
    it('exports executeCurrentSQL', () => {
      expect(typeof executeCurrentSQL).toBe('function')
    })
    it('exports executeNewTabSQL', () => {
      expect(typeof executeNewTabSQL).toBe('function')
    })
    it('exports executeDuckDBAccelerated', () => {
      expect(typeof executeDuckDBAccelerated).toBe('function')
    })
    it('exports cancelExecution', () => {
      expect(typeof cancelExecution).toBe('function')
    })
    it('exports formatActiveSQL', () => {
      expect(typeof formatActiveSQL).toBe('function')
    })
    it('exports validateActiveSQL', () => {
      expect(typeof validateActiveSQL).toBe('function')
    })
    it('exports toggleComment', () => {
      expect(typeof toggleComment).toBe('function')
    })
  })

  describe('executeCurrentSQL', () => {
    it('returns early when no active file', async () => {
      // activeFileInfo.value is null from mock
      await expect(executeCurrentSQL()).resolves.toBeUndefined()
    })
  })

  describe('cancelExecution', () => {
    it('delegates to runtimeState.cancelExecution', () => {
      cancelExecution()
      expect(runtimeState.cancelExecution).toHaveBeenCalled()
    })
  })

  describe('executeNewTabSQL', () => {
    it('falls back to executeCurrentSQL when no active file', async () => {
      await expect(executeNewTabSQL()).resolves.toBeUndefined()
    })
  })

  describe('executeDuckDBAccelerated', () => {
    it('returns early when no active file', async () => {
      await expect(executeDuckDBAccelerated()).resolves.toBeUndefined()
    })
  })

  describe('formatActiveSQL', () => {
    it('returns early when no active file', async () => {
      await expect(formatActiveSQL()).resolves.toBeUndefined()
    })
  })

  describe('validateActiveSQL', () => {
    it('returns early when no active file', async () => {
      await expect(validateActiveSQL()).resolves.toBeUndefined()
    })
  })

  describe('toggleComment', () => {
    it('returns early when no active file', async () => {
      await expect(toggleComment()).resolves.toBeUndefined()
    })
  })
})
