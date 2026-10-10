import { create } from 'zustand'
import type { BotResponse, MjaiEvent, Notification } from '@/types'

type Tagged<T> = T & { _ts: number; _seq: number }

type NotifyStore = {
  events: Tagged<MjaiEvent>[]
  responses: Tagged<BotResponse>[]
  notifications: Tagged<Notification>[]
  pushEvent: (e: MjaiEvent) => void
  pushResponse: (r: BotResponse) => void
  pushToast: (n: Notification) => void
  clearToasts: () => void
  clearGame: () => void
}

const MAX_EVENTS = 100
const MAX_RESPONSES = 80
const MAX_TOASTS = 50

let seq = 0
const tag = <T>(v: T): Tagged<T> =>
  ({ ...v, _ts: Date.now(), _seq: ++seq }) as Tagged<T>

const trim = <T>(arr: T[], max: number) =>
  arr.length > max ? arr.slice(arr.length - max) : arr

export const useNotifyStore = create<NotifyStore>((set) => ({
  events: [],
  responses: [],
  notifications: [],

  pushEvent: (e) =>
    set((s) => ({ events: trim([...s.events, tag(e)], MAX_EVENTS) })),

  pushResponse: (r) =>
    set((s) => ({ responses: trim([...s.responses, tag(r)], MAX_RESPONSES) })),

  pushToast: (n) =>
    set((s) => {
      // dedupe by id (replace prior toast with same id)
      const filtered = n.id ? s.notifications.filter((t) => t.id !== n.id) : s.notifications
      return { notifications: trim([...filtered, tag(n)], MAX_TOASTS) }
    }),

  clearToasts: () => set({ notifications: [] }),
  clearGame: () => set({ events: [], responses: [] }),
}))
