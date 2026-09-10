import { describe, it, expect } from 'vitest'

import { bindParams } from '../sql-editor-service'

describe('bindParams', () => {
  it('replaces named parameters with quoted values', () => {
    const result = bindParams('SELECT * FROM users WHERE id = :id', { id: '123' })
    expect(result).toBe("SELECT * FROM users WHERE id = '123'")
  })

  it('replaces multiple parameters', () => {
    const result = bindParams('INSERT INTO t (a, b) VALUES (:x, :y)', { x: 'hello', y: 'world' })
    expect(result).toBe("INSERT INTO t (a, b) VALUES ('hello', 'world')")
  })

  it('escapes single quotes in values', () => {
    const result = bindParams('SELECT * FROM t WHERE name = :name', {
      name: "O'Brien",
    })
    expect(result).toBe("SELECT * FROM t WHERE name = 'O''Brien'")
  })

  it('handles repeated parameter names', () => {
    const result = bindParams('SELECT :x, :x, :x', { x: 'val' })
    expect(result).toBe("SELECT 'val', 'val', 'val'")
  })

  it('returns unchanged SQL when no values to bind', () => {
    const result = bindParams('SELECT 1', {})
    expect(result).toBe('SELECT 1')
  })

  it('matches exact parameter names (partial match safety)', () => {
    const result = bindParams('SELECT :id, :id_name', { id: '1' })
    expect(result).toBe("SELECT '1', :id_name")
  })

  it('handles value with special regex characters', () => {
    const result = bindParams('WHERE col = :val', { val: 'a(b' })
    expect(result).toBe("WHERE col = 'a(b'")
  })
})
