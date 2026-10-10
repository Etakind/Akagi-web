import { beforeEach, describe, expect, it, vi } from 'vitest'
import { render, waitFor } from '@testing-library/react'
import { useNotifyStore } from '@/stores/notifyStore'
import { useCaptureStore } from '@/stores/captureStore'

const tauri = vi.hoisted(() => ({
  listen: vi.fn(),
  invoke: vi.fn(),
}))

vi.mock('@/lib/tauri', () => ({
  HAS_TAURI: true,
  listen: tauri.listen,
  invoke: tauri.invoke,
}))

vi.mock('@/hooks/useAutoplayStatus', () => ({
  useAutoplayStatus: () => {},
}))

vi.mock('@/components/ui/sonner', () => ({
  toast: { info: vi.fn(), success: vi.fn(), warning: vi.fn(), error: vi.fn() },
}))

import { useTauriBridge } from './useTauriBridge'

type Listener = (payload: unknown) => void

function Harness() {
  useTauriBridge()
  return null
}

const recovery = (phase: 'inactive' | 'recovering' | 'ready' | 'error', epoch: number) => ({
  phase,
  method: 'reload' as const,
  reason: phase === 'error' ? 'cancelled' : null,
  epoch,
  can_recover: phase === 'error',
})

const advisory = (epoch: number) => ({
  type: 'dahai' as const,
  actor: 2,
  pai: '5m',
  tsumogiri: false,
  meta: {
    advisory_only: true,
    recovery_epoch: epoch,
    show: { title: 'Restored', items: [{ label: 'Manual discard', value: '90%' }] },
  },
})

const normal = {
  type: 'dahai' as const,
  actor: 2,
  pai: '5m',
  tsumogiri: false,
}

describe('useTauriBridge recovery advisory ordering', () => {
  let listeners: Map<string, Listener>

  beforeEach(() => {
    listeners = new Map()
    tauri.listen.mockReset().mockImplementation(async (event: string, callback: Listener) => {
      listeners.set(event, callback)
      return () => listeners.delete(event)
    })
    tauri.invoke.mockReset().mockImplementation(async (command: string) => {
      if (command === 'get_status') {
        return {
          config: {},
          log_dir: '',
          bot_status: { state: 'idle' },
          capture_status: { state: 'running', kind: 'chromium', descriptor: '' },
        }
      }
      if (command === 'get_game_recovery_status') return recovery('recovering', 7)
      if (command === 'get_analysis') return null
      if (command === 'list_game_history') return []
      if (command === 'get_game_snapshot' || command === 'get_mahgen_view') return null
      return undefined
    })
    useNotifyStore.setState({ events: [], responses: [], notifications: [] })
    useCaptureStore.setState({
      status: { state: 'stopped' },
      recovery: recovery('inactive', 0),
    })
  })

  it('buffers an advisory sent before Ready and releases it for the same epoch', async () => {
    const { unmount } = render(<Harness />)
    await waitFor(() => expect(listeners.has('bot-response')).toBe(true))
    await waitFor(() => expect(useCaptureStore.getState().recovery.phase).toBe('recovering'))

    listeners.get('bot-response')?.(advisory(7))
    expect(useNotifyStore.getState().responses).toHaveLength(0)

    listeners.get('game-recovery-status')?.(recovery('ready', 7))
    await waitFor(() => expect(useNotifyStore.getState().responses).toHaveLength(1))
    expect(useNotifyStore.getState().responses[0].meta?.advisory_only).toBe(true)

    listeners.get('mjai-event')?.({ type: 'tsumo', actor: 2, pai: '9p' })
    expect(useNotifyStore.getState().responses).toHaveLength(0)
    unmount()
  })

  it('shows an advisory immediately when Ready already arrived and keeps ordinary responses gated', async () => {
    const { unmount } = render(<Harness />)
    await waitFor(() => expect(listeners.has('bot-response')).toBe(true))
    listeners.get('game-recovery-status')?.(recovery('ready', 7))
    listeners.get('bot-response')?.(advisory(7))
    await waitFor(() => expect(useNotifyStore.getState().responses).toHaveLength(1))

    // A normal response that arrives while a later recovery is still pending
    // keeps the old bridge behavior and is not released by the advisory path.
    listeners.get('game-recovery-status')?.(recovery('recovering', 8))
    expect(useNotifyStore.getState().responses).toHaveLength(0)
    listeners.get('bot-response')?.(normal)
    expect(useNotifyStore.getState().responses).toHaveLength(0)

    listeners.get('game-recovery-status')?.(recovery('ready', 8))
    await waitFor(() => expect(useNotifyStore.getState().responses).toHaveLength(0))
    unmount()
  })

  it('drops a buffered advisory on cancellation or a newer recovery epoch', async () => {
    const { unmount } = render(<Harness />)
    await waitFor(() => expect(listeners.has('bot-response')).toBe(true))
    listeners.get('bot-response')?.(advisory(7))
    listeners.get('game-recovery-status')?.(recovery('error', 7))
    expect(useNotifyStore.getState().responses).toHaveLength(0)

    listeners.get('bot-response')?.(advisory(7))
    listeners.get('game-recovery-status')?.(recovery('ready', 8))
    await waitFor(() => expect(useNotifyStore.getState().responses).toHaveLength(0))
    unmount()
  })
})
