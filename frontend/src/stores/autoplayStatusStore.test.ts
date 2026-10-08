import { beforeEach, describe, expect, it } from 'vitest'
import { useAutoplayStatusStore } from './autoplayStatusStore'
import type { AutoplayRecord, AutoplayStatusUpdate } from '@/types'

function record(id: number, phase: AutoplayRecord['phase'] = 'preparing'): AutoplayRecord {
  return {
    id,
    round: 'E1:0',
    created_at: id,
    action: { type: 'dahai', pai: id === 1 ? '5mr' : '5m' },
    step: 'input',
    retry: 0,
    phase,
    remaining_ms: null,
    reason: null,
  }
}

function update(version: number, records: AutoplayRecord[], reset = false, enabled = true): AutoplayStatusUpdate {
  return { version, reset, enabled, records }
}

describe('autoplay status store', () => {
  beforeEach(() => {
    useAutoplayStatusStore.setState({ version: -1, enabled: false, records: [] })
  })

  it('hydrates a snapshot and upserts later records by id without losing history', () => {
    const merge = useAutoplayStatusStore.getState().merge
    merge(update(10, [record(2), record(1)], true))
    expect(useAutoplayStatusStore.getState().records.map((r) => r.id)).toEqual([1, 2])

    merge(update(11, [record(2, 'succeeded'), record(3)]))
    const state = useAutoplayStatusStore.getState()
    expect(state.version).toBe(11)
    expect(state.records.map((r) => r.id)).toEqual([1, 2, 3])
    expect(state.records.find((r) => r.id === 2)?.phase).toBe('succeeded')
  })

  it('ignores stale revisions, including a stale reset snapshot', () => {
    const merge = useAutoplayStatusStore.getState().merge
    merge(update(4, [record(1)], true, true))
    merge(update(3, [record(8)], true, false))
    expect(useAutoplayStatusStore.getState()).toMatchObject({
      version: 4,
      enabled: true,
      records: [record(1)],
    })
  })

  it('reopens from a newer reset snapshot and clears records from the prior game', () => {
    const merge = useAutoplayStatusStore.getState().merge
    merge(update(20, [record(1), record(2)], true, true))
    merge(update(21, [record(3)], true, false))
    expect(useAutoplayStatusStore.getState()).toMatchObject({
      version: 21,
      enabled: false,
      records: [record(3)],
    })
  })

  it('lets an equal-version full snapshot repair a missed delta', () => {
    const merge = useAutoplayStatusStore.getState().merge
    merge(update(30, [record(1), record(2)], true, true))
    merge(update(30, [record(2, 'succeeded')], true, true))
    expect(useAutoplayStatusStore.getState().records).toEqual([record(2, 'succeeded')])
  })
})
