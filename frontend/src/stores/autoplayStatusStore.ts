import { create } from 'zustand'
import type { AutoplayRecord, AutoplayStatusUpdate } from '@/types'

type AutoplayStatusStore = {
  version: number
  enabled: boolean
  records: AutoplayRecord[]
  merge: (update: AutoplayStatusUpdate) => void
}

// Records are runtime-only: each webview hydrates from the backend, not storage.
export const useAutoplayStatusStore = create<AutoplayStatusStore>((set) => ({
  version: -1,
  enabled: false,
  records: [],
  merge: (update) => set((state) => {
    // An equal-version full snapshot may repair a missed earlier delta.
    if (update.version < state.version || (update.version === state.version && !update.reset)) return state
    const records = new Map((update.reset ? [] : state.records).map((record) => [record.id, record]))
    for (const record of update.records) records.set(record.id, record)
    return {
      version: update.version,
      enabled: update.enabled,
      records: [...records.values()].sort((a, b) => a.id - b.id),
    }
  }),
}))
