import { beforeEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { EventsTile } from './EventsTile'
import { useAutoplayStatusStore } from '@/stores/autoplayStatusStore'
import { useNotifyStore } from '@/stores/notifyStore'
import type { AutoplayRecord } from '@/types'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}))

function record(id: number): AutoplayRecord {
  return {
    id,
    round: 'E1:0',
    created_at: id,
    action: { type: 'dahai', pai: '5m' },
    step: 'input',
    retry: 0,
    phase: 'succeeded',
    remaining_ms: null,
    reason: null,
  }
}

describe('EventsTile', () => {
  beforeEach(() => {
    useNotifyStore.setState({ events: [], responses: [], notifications: [] })
    useAutoplayStatusStore.setState({ version: 10, enabled: true, records: [record(1)] })
  })

  it('keeps the game-events tab selected by default and exposes autoplay records separately', async () => {
    render(<EventsTile bp="lg" />)
    const gameTab = screen.getByRole('tab', { name: 'autoplay_status.game_events' })
    const autoplayTab = screen.getByRole('tab', { name: 'autoplay_status.title' })
    expect(gameTab.getAttribute('data-state')).toBe('active')
    expect(autoplayTab.getAttribute('data-state')).toBe('inactive')
    expect(screen.getByText('tile.events_empty')).toBeTruthy()

    fireEvent.mouseDown(autoplayTab)
    await waitFor(() => {
      expect(gameTab.getAttribute('data-state')).toBe('inactive')
      expect(autoplayTab.getAttribute('data-state')).toBe('active')
    })
    expect(screen.getByText(/autoplay_status\.action\.dahai/)).toBeTruthy()
  })
})
