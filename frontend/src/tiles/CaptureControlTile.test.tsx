import { beforeEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { useCaptureStore } from '@/stores/captureStore'
import { CaptureControlTile } from './CaptureControlTile'

const invoke = vi.hoisted(() => vi.fn())
vi.mock('@/lib/tauri', () => ({ invoke }))
vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key }) }))
vi.mock('@/components/TileFrame', () => ({ TileFrame: ({ children }: { children: React.ReactNode }) => <section>{children}</section> }))

beforeEach(() => {
  invoke.mockReset().mockResolvedValue(undefined)
  useCaptureStore.setState({ status: { state: 'running', kind: 'chromium', descriptor: '' }, recovery: { phase: 'missing', method: null, reason: 'initial_state_missing', epoch: 1, can_recover: true } })
})

describe('midgame recovery entry', () => {
  it('explains the bounded reconnect/reload and invokes recovery once', async () => {
    render(<CaptureControlTile bp="lg" />)
    expect(screen.getByRole('status').textContent).toBe('recovery.phase.missing')
    expect(screen.getByText('recovery.description')).not.toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'recovery.button' }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('recover_majsoul_game'))
    expect(invoke).toHaveBeenCalledTimes(1)
  })
  it('disables the button while restoring', () => {
    useCaptureStore.getState().setRecovery({ phase: 'recovering', method: 'reconnect', reason: null, epoch: 2, can_recover: false })
    render(<CaptureControlTile bp="lg" />)
    expect(screen.getByRole('button', { name: 'recovery.button' }).hasAttribute('disabled')).toBe(true)
  })
  it('hides recovery when no game needs it', () => {
    useCaptureStore.getState().setRecovery({ phase: 'ready', method: 'reload', reason: null, epoch: 3, can_recover: false })
    render(<CaptureControlTile bp="lg" />)
    expect(screen.queryByRole('button', { name: 'recovery.button' })).toBeNull()
  })
})
