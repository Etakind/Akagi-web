import type { AppConfig } from '@/types'

export function appConfigFixture(): AppConfig {
  const eventCount = 4, fontSize = 14
  const overlay = {
    enabled: true,
    top_n: 3,
    event_count: eventCount,
    font_size: fontSize,
    opacity: 0.95,
    always_on_top: true,
  }
  return {
  general: { first_run_completed: true, overlay_defaults_revision: 1 },
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
