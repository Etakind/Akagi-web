import { useEffect } from 'react'
import { HAS_TAURI, invoke, listen } from '@/lib/tauri'
import { useAutoplayStatusStore } from '@/stores/autoplayStatusStore'
import type { AutoplayStatusUpdate } from '@/types'

/** Subscribe first, buffer while hydrating, then merge by version and ID. */
export function useAutoplayStatus() {
  useEffect(() => {
    if (!HAS_TAURI) return
    let cancelled = false
    let hydrated = false
    let unlisten: (() => void) | undefined
    const pending: AutoplayStatusUpdate[] = []
    const merge = useAutoplayStatusStore.getState().merge

    void (async () => {
      try {
        const unsubscribe = await listen<AutoplayStatusUpdate>('autoplay-status', (update) => {
          if (cancelled) return
          if (!hydrated) pending.push(update)
          else merge(update)
        })
        if (cancelled) { unsubscribe(); return }
        unlisten = unsubscribe
        const snapshot = await invoke<AutoplayStatusUpdate>('get_autoplay_status')
        if (!cancelled) merge(snapshot)
      } catch {
        // Keep the listener: live observations can still recover after startup.
      } finally {
        if (!cancelled) {
          hydrated = true
          pending.sort((a, b) => a.version - b.version).forEach(merge)
        }
      }
    })()

    return () => { cancelled = true; unlisten?.() }
  }, [])
}
