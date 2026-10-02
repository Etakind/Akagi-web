import { create } from 'zustand'

// Frontend-only UI preferences kept out of the Tauri-owned `AppConfig` since
// they don't affect any backend behavior. Sidebar collapsed/hover state lives
// in `useSidebar` (own zustand+persist store ported from shadcn-ui-sidebar).
const SCALE_KEY = 'akagi.ui.scale'

// One-time flag: has the user seen the dashboard onboarding hint (drag /
// resize / remove / add tiles)? Deliberately NOT reset by "Reset Layout" —
// this tracks "has seen the tutorial", not layout state.
const ONBOARDED_KEY = 'akagi.dashboard.onboarded'


export const SCALE_MIN = 0.7
export const SCALE_MAX = 1.5
export const SCALE_STEP = 0.05
export const SCALE_DEFAULT = 1.0

function clampScale(v: number): number {
  if (!Number.isFinite(v)) return SCALE_DEFAULT
  return Math.min(SCALE_MAX, Math.max(SCALE_MIN, v))
}

function loadScale(): number {
  if (typeof localStorage === 'undefined') return SCALE_DEFAULT
  try {
    const raw = localStorage.getItem(SCALE_KEY)
    if (!raw) return SCALE_DEFAULT
    return clampScale(parseFloat(raw))
  } catch {
    return SCALE_DEFAULT
  }
}

function loadFlag(key: string): boolean {
  if (typeof localStorage === 'undefined') return false
  try {
    return localStorage.getItem(key) === '1'
  } catch {
    return false
  }
}


type UiPrefsStore = {
  scale: number
  setScale: (v: number) => void
  resetScale: () => void
  /** Whether the dashboard onboarding hint has been dismissed at least once. */
  dashboardOnboarded: boolean
  markDashboardOnboarded: () => void

}

export const useUiPrefsStore = create<UiPrefsStore>((set) => ({
  scale: loadScale(),
  setScale: (v) => {
    const scale = clampScale(v)
    try {
      localStorage.setItem(SCALE_KEY, String(scale))
    } catch {
      /* quota — ignore */
    }
    set({ scale })
  },
  resetScale: () => {
    try {
      localStorage.setItem(SCALE_KEY, String(SCALE_DEFAULT))
    } catch {
      /* quota — ignore */
    }
    set({ scale: SCALE_DEFAULT })
  },
  dashboardOnboarded: loadFlag(ONBOARDED_KEY),
  markDashboardOnboarded: () => {
    try {
      localStorage.setItem(ONBOARDED_KEY, '1')
    } catch {
      /* quota — ignore */
    }
    set({ dashboardOnboarded: true })
  },
}))
