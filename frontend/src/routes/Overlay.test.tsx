import { beforeEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { Overlay } from './Overlay'
import { useAutoplayStatusStore } from '@/stores/autoplayStatusStore'
import type { AppConfig, AutoplayRecord, AutoplayStatusUpdate } from '@/types'

const tauri = vi.hoisted(() => ({
  listen: vi.fn(),
  invoke: vi.fn(),
}))

vi.mock('@/lib/tauri', () => ({
  HAS_TAURI: true,
  listen: tauri.listen,
  invoke: tauri.invoke,
}))

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

const config = (eventCount: number, fontSize = 14): AppConfig => {
  const overlay = {
    enabled: true,
    top_n: 3,
    event_count: eventCount,
    font_size: fontSize,
    opacity: 0.95,
    always_on_top: true,
  }
  return {
  general: { first_run_completed: true },
  logging: { dir: '.', level: 'info', all_level: 'info' },
  platform: { kind: 'Majsoul' },
  bot: { enabled: false, active_4p: 'none', active_3p: 'none' },
  capture: {
    mode: 'chromium',
    enabled: false,
    unavailable_reason: null,
    http: { record_all: false, bodies: false, max_body_bytes: 0, static_assets: false },
    chromium: {
      executable: '',
      user_data_dir: '',
      start_url: '',
      cft_channel: 'Stable',
      force_cft: false,
      extra_args: [],
    },
  },
  autoplay: {
    enabled: true,
    majsoul: {
      pre_click_delay_min_ms: 0,
      pre_click_delay_max_ms: 0,
      inter_click_delay_ms: 0,
      hover_delay_ms: 0,
      click_hold_ms: 0,
      verify_input_ms: 0,
      click_retries: 0,
      reload_after_failures: 0,
      dealer_first_discard_extra_delay_ms: 0,
    },
    delay: {
      mode: 'legacy',
      min_delay_ms: 0,
      min_button_delay_ms: 0,
      distribution: 'uniform',
      lognormal: {},
      bank_on_long_thought: false,
      riichi_extra_ms: 0,
      kan_extra_ms: 0,
      safety_margin_ms: 0,
      bank_use_fraction: 0,
      bank_max_single_ms: 0,
      no_budget_cap_ms: 0,
    },
  },
    overlay: overlay as AppConfig['overlay'],
  }
}

function status(records: AutoplayRecord[]): AutoplayStatusUpdate {
  return { version: 1, reset: true, enabled: true, records }
}

describe('Overlay autoplay records', () => {
  beforeEach(() => {
    tauri.listen.mockReset()
    tauri.invoke.mockReset()
    tauri.listen.mockResolvedValue(() => {})
    tauri.invoke.mockImplementation(async (command: string) => {
      if (command === 'get_config') return config(2)
      if (command === 'get_autoplay_status') return status([])
      return undefined
    })
    useAutoplayStatusStore.setState({ version: -1, enabled: true, records: [] })
  })

  it('renders only the latest configured rows and closing does not erase history', async () => {
    const allRecords = [record(1), record(2), record(3), record(4)]
    useAutoplayStatusStore.setState({ version: 2, enabled: true, records: allRecords })
    render(<Overlay />)

    const section = await waitFor(() => {
      const heading = screen.getByRole('heading', { name: 'autoplay_status.title' })
      return heading.closest('section') as HTMLElement
    })
    expect(section.querySelectorAll('div.h-10')).toHaveLength(2)

    fireEvent.click(screen.getByRole('button', { name: 'overlay.close' }))
    expect(tauri.invoke).toHaveBeenCalledWith('set_overlay_enabled', { enabled: false })
    expect(useAutoplayStatusStore.getState().records).toHaveLength(4)
  })

  it('clamps the visible row count to the one-to-ten compatibility range', async () => {
    tauri.invoke.mockImplementation(async (command: string) => {
      if (command === 'get_config') return config(99)
      if (command === 'get_autoplay_status') return status([])
      return undefined
    })
    useAutoplayStatusStore.setState({ version: 2, enabled: true, records: Array.from({ length: 12 }, (_, i) => record(i + 1)) })
    render(<Overlay />)
    const section = await waitFor(() => screen.getByRole('heading', { name: 'autoplay_status.title' }).closest('section') as HTMLElement)
    expect(section.style.height).toBe('428px')
    expect(section.querySelectorAll('div.h-10')).toHaveLength(10)
  })

  it('scales only the overlay text and event height from the configured font size', async () => {
    tauri.invoke.mockImplementation(async (command: string) => {
      if (command === 'get_config') return config(2, 21)
      if (command === 'get_autoplay_status') return status([])
      return undefined
    })
    useAutoplayStatusStore.setState({ version: 2, enabled: true, records: [record(1), record(2)] })
    const { container } = render(<Overlay />)
    const section = await waitFor(() => screen.getByRole('heading', { name: 'autoplay_status.title' }).closest('section') as HTMLElement)
    const root = container.firstElementChild as HTMLElement
    expect(root.style.getPropertyValue('--overlay-scale')).toBe('1.5')
    expect(section.style.height).toBe('162px')
    expect(document.documentElement.style.fontSize).toBe('')
    expect(document.body.style.fontSize).toBe('')
  })
})
