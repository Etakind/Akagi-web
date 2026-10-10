import { beforeEach, describe, expect, it } from 'vitest'
import { useNotifyStore } from './notifyStore'

const normal = {
  type: 'dahai' as const,
  actor: 2,
  pai: '5m',
  tsumogiri: false,
}

const advisory = {
  ...normal,
  meta: { advisory_only: true },
}

describe('notifyStore restored advisory lifecycle', () => {
  beforeEach(() => {
    useNotifyStore.setState({ events: [], responses: [], notifications: [] })
  })

  it('clears only the manual advisory when a new live event arrives', () => {
    const store = useNotifyStore.getState()
    store.pushResponse(advisory)
    store.pushResponse(normal)

    store.pushEvent({ type: 'tsumo', actor: 2, pai: '9p' })

    expect(useNotifyStore.getState().responses.map((response) => response.meta)).toEqual([undefined])
    expect(useNotifyStore.getState().events).toHaveLength(1)
  })

  it('does not clear an ordinary live response', () => {
    useNotifyStore.getState().pushResponse(normal)

    useNotifyStore.getState().pushEvent({ type: 'dahai', actor: 0, pai: '1m', tsumogiri: false })

    expect(useNotifyStore.getState().responses).toHaveLength(1)
    expect(useNotifyStore.getState().responses[0].type).toBe('dahai')
  })
})
