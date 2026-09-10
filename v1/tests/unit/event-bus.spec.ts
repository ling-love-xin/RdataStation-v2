import { describe, it, expect, afterEach, vi } from 'vitest'

import { EventBus } from '../../src/extensions/builtin/database/ui/composables/use-event-bus'

describe('EventBus', () => {
  const eventBus = new EventBus()

  afterEach(() => {
    eventBus.clear()
  })

  describe('on/off', () => {
    it('should register and unregister event listeners', () => {
      const handler = vi.fn()

      eventBus.on('test-event', handler)
      eventBus.emit('test-event', 'test-data')
      expect(handler).toHaveBeenCalledWith('test-data')

      eventBus.off('test-event', handler)
      eventBus.emit('test-event', 'test-data-2')
      expect(handler).toHaveBeenCalledTimes(1)
    })

    it('should support multiple listeners for the same event', () => {
      const handler1 = vi.fn()
      const handler2 = vi.fn()

      eventBus.on('test-event', handler1)
      eventBus.on('test-event', handler2)
      eventBus.emit('test-event', 'data')

      expect(handler1).toHaveBeenCalledWith('data')
      expect(handler2).toHaveBeenCalledWith('data')
    })
  })

  describe('emit', () => {
    it('should emit events with data', () => {
      const handler = vi.fn()
      eventBus.on('emit-test', handler)

      eventBus.emit('emit-test', { id: 1, name: 'test' })
      expect(handler).toHaveBeenCalledWith({ id: 1, name: 'test' })
    })

    it('should emit events without data', () => {
      const handler = vi.fn()
      eventBus.on('no-data-event', handler)

      eventBus.emit('no-data-event')
      expect(handler).toHaveBeenCalled()
    })
  })

  describe('once', () => {
    it('should only trigger handler once', () => {
      const handler = vi.fn()
      eventBus.once('once-event', handler)

      eventBus.emit('once-event', 'first')
      eventBus.emit('once-event', 'second')

      expect(handler).toHaveBeenCalledTimes(1)
      expect(handler).toHaveBeenCalledWith('first')
    })
  })

  describe('clearEvent', () => {
    it('should remove all listeners for a specific event', () => {
      const handler1 = vi.fn()
      const handler2 = vi.fn()

      eventBus.on('multi-event', handler1)
      eventBus.on('multi-event', handler2)
      eventBus.on('other-event', vi.fn())

      eventBus.clearEvent('multi-event')
      eventBus.emit('multi-event', 'data')

      expect(handler1).not.toHaveBeenCalled()
      expect(handler2).not.toHaveBeenCalled()
    })

    it('should remove all listeners when clear is called', () => {
      const handler1 = vi.fn()
      const handler2 = vi.fn()

      eventBus.on('event1', handler1)
      eventBus.on('event2', handler2)

      eventBus.clear()
      eventBus.emit('event1', 'data')
      eventBus.emit('event2', 'data')

      expect(handler1).not.toHaveBeenCalled()
      expect(handler2).not.toHaveBeenCalled()
    })
  })

  describe('getSubscriptionCount', () => {
    it('should return count when event has listeners', () => {
      eventBus.on('check-event', () => {})
      expect(eventBus.getSubscriptionCount('check-event')).toBe(1)
    })

    it('should return 0 when event has no listeners', () => {
      expect(eventBus.getSubscriptionCount('no-listeners-event')).toBe(0)
    })
  })
})
