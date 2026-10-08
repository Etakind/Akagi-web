import { describe, expect, it, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import type { AutoplayRecord } from '@/types'
import { autoplayCountdown } from '@/lib/autoplayStatus'
import { AutoplayRecordRow } from './AutoplayRecordRow'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, values?: { number?: string; code?: string; count?: number }) => {
      if (key === 'autoplay_status.tile.m') return `${values?.number ?? ''}m`
      if (key === 'autoplay_status.tile.red') return ' red'
      return key
    },
  }),
}))

function record(overrides: Partial<AutoplayRecord> = {}): AutoplayRecord {
  return {
    id: 1,
    round: 'E1:0',
    created_at: 1,
    action: { type: 'dahai', pai: '5mr' },
    step: 'input',
    retry: 0,
    phase: 'scheduled',
    remaining_ms: 1,
    reason: null,
    ...overrides,
  }
}

describe('AutoplayRecordRow', () => {
  it('rounds countdowns upward and never renders zero seconds', () => {
    expect(autoplayCountdown(1)).toBe('0.1s')
    expect(autoplayCountdown(99)).toBe('0.1s')
    expect(autoplayCountdown(101)).toBe('0.2s')
  })

  it('shows red tile information and the live countdown for a scheduled action', () => {
    render(<AutoplayRecordRow record={record({ remaining_ms: 101 })} />)
    expect(screen.getByText('autoplay_status.action.dahai 5m red')).toBeTruthy()
    expect(screen.getByText('0.2s')).toBeTruthy()
    expect(screen.queryByText('0.0s')).toBeNull()
  })

  it('does not display a stale zero countdown as a future timer', () => {
    render(<AutoplayRecordRow record={record({ remaining_ms: 0 })} />)
    expect(screen.getByText('autoplay_status.phase.preparing')).toBeTruthy()
    expect(screen.queryByText('0.0s')).toBeNull()
  })

  it('marks both text lines red while unconfirmed and restores them after late success', () => {
    const { container, rerender } = render(
      <AutoplayRecordRow record={record({ phase: 'unconfirmed', reason: 'feedback_timeout', remaining_ms: null })} />,
    )
    const lines = () => Array.from(container.firstElementChild?.children ?? [])
    expect(lines()).toHaveLength(2)
    expect(container.firstElementChild?.className).toMatch(/text-red/)
    expect(lines()[1].className).toMatch(/text-red/)

    rerender(<AutoplayRecordRow record={record({ phase: 'succeeded', reason: 'server_confirmed', remaining_ms: null })} />)
    expect(container.firstElementChild?.className).not.toMatch(/text-red/)
    expect(lines()[1].className).not.toMatch(/text-red/)
  })
})
