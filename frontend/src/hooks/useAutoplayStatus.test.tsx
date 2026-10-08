import { beforeEach, describe, expect, it, vi } from 'vitest'
import { render, waitFor } from '@testing-library/react'
import type { AutoplayRecord, AutoplayStatusUpdate } from '@/types'

const tauri = vi.hoisted(() => ({
  listen: vi.fn(),
  invoke: vi.fn(),
}))

vi.mock('@/lib/tauri', () => ({
  HAS_TAURI: true,
  listen: tauri.listen,
  invoke: tauri.invoke,
}))

import { useAutoplayStatus } from './useAutoplayStatus'
import { useAutoplayStatusStore } from '@/stores/autoplayStatusStore'

type Listener = (update: AutoplayStatusUpdate) => void

function record(id: number): AutoplayRecord {
  return {
    id,
    round: 'E1:0',
    created_at: id,
    action: { type: 'none' },
    step: 'input',
    retry: 0,
    phase: 'awaiting_feedback',
    remaining_ms: null,
    reason: null,
  }
}

function Harness() {
  useAutoplayStatus()
  return null
}

describe('useAutoplayStatus', () => {
  beforeEach(() => {
    useAutoplayStatusStore.setState({ version: -1, enabled: false, records: [] })
    tauri.listen.mockReset()
    tauri.invoke.mockReset()
  })

  it('subscribes before the snapshot and buffers a live update during hydration', async () => {
    let listener: Listener | undefined
    let resolveSnapshot: ((update: AutoplayStatusUpdate) => void) | undefined
    const unlisten = vi.fn()
    tauri.listen.mockImplementation(async (_name: string, cb: Listener) => {
      listener = cb
      return unlisten
    })
    tauri.invoke.mockImplementation(() => new Promise<AutoplayStatusUpdate>((resolve) => {
      resolveSnapshot = resolve
    }))

    const { unmount } = render(<Harness />)
    await waitFor(() => expect(listener).toBeDefined())
    expect(tauri.listen.mock.invocationCallOrder[0]).toBeLessThan(tauri.invoke.mock.invocationCallOrder[0])

    listener?.({ version: 7, reset: false, enabled: true, records: [record(2)] })
    resolveSnapshot?.({ version: 6, reset: true, enabled: true, records: [record(1)] })

    await waitFor(() => {
      expect(useAutoplayStatusStore.getState().records.map((r) => r.id)).toEqual([1, 2])
    })
    expect(useAutoplayStatusStore.getState().version).toBe(7)

    unmount()
    expect(unlisten).toHaveBeenCalledOnce()
  })

  it('merges only newer live revisions after hydration and cleans up on reopen', async () => {
    let listener: Listener | undefined
    const unlisten = vi.fn()
    tauri.listen.mockImplementation(async (_name: string, cb: Listener) => {
      listener = cb
      return unlisten
    })
    tauri.invoke.mockResolvedValue({ version: 10, reset: true, enabled: true, records: [record(1)] })

    const { unmount } = render(<Harness />)
    await waitFor(() => expect(useAutoplayStatusStore.getState().version).toBe(10))
    listener?.({ version: 9, reset: false, enabled: false, records: [record(9)] })
    expect(useAutoplayStatusStore.getState().records.map((r) => r.id)).toEqual([1])

    listener?.({ version: 11, reset: false, enabled: true, records: [record(2)] })
    expect(useAutoplayStatusStore.getState().records.map((r) => r.id)).toEqual([1, 2])
    unmount()
    expect(unlisten).toHaveBeenCalledOnce()
  })
})

