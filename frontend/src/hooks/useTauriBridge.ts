import { useEffect } from 'react'
import { useAutoplayStatus } from '@/hooks/useAutoplayStatus'
import { HAS_TAURI, invoke, listen } from '@/lib/tauri'
import type {
  AnalysisResult,
  BotResponse,
  BotStatus,
  CaptureStatus,
  GameRecoveryStatus,
  GameRecord,
  GameStateSnapshot,
  HistoryEvent,
  MahgenView,
  MjaiEvent,
  Notification,
  OverlayConfig,
  Snapshot,
} from '@/types'
import { useGameStore } from '@/stores/gameStore'
import { useAnalysisStore } from '@/stores/analysisStore'
import { useBotStore } from '@/stores/botStore'
import { useCaptureStore } from '@/stores/captureStore'
import { useNotifyStore } from '@/stores/notifyStore'
import { useConfigStore } from '@/stores/configStore'
import { useHistoryStore } from '@/stores/historyStore'
import { toast, type ToastSeverity } from '@/components/ui/sonner'

// Backend `Notification.level` ∈ {info,success,warn,error}; toast helper
// uses `warning`. Map across.
const TOAST_SEVERITY: Record<Notification['level'], ToastSeverity> = {
  info: 'info',
  success: 'success',
  warn: 'warning',
  error: 'error',
}

// One-shot bridge mounted from <App>. Subscribes to all Tauri events,
// hydrates initial state, and unsubscribes on unmount.
export function useTauriBridge() {
  useAutoplayStatus()
  useEffect(() => {
    if (!HAS_TAURI) return

    const unlistens: Array<() => void> = []
    let cancelled = false
    // Ready status and the advisory use different backend forwarding tasks.
    // Keep only the matching recovery's advisory until its Ready event arrives.
    let pendingAdvisory: BotResponse | null = null

    const refreshGame = async () => {
      const { epoch, phase } = useCaptureStore.getState().recovery
      if (phase !== 'ready' && phase !== 'inactive') return
      try {
        const [snap, view] = await Promise.all([
          invoke<GameStateSnapshot | null>('get_game_snapshot'),
          invoke<MahgenView | null>('get_mahgen_view'),
        ])
        if (cancelled || epoch !== useCaptureStore.getState().recovery.epoch) return
        useGameStore.getState().setGame(snap)
        useGameStore.getState().setView(view)
      } catch {
        /* ignore: backend may not be ready */
      }
    }

    ;(async () => {
      try {
        const status = await invoke<Snapshot>('get_status')
        if (cancelled) return
        useConfigStore.getState().setConfig(status.config)
        useConfigStore.getState().setLogDir(status.log_dir)
        useBotStore.getState().setStatus(status.bot_status)
        useCaptureStore.getState().set(status.capture_status)
      } catch {
        /* ignore */
      }
      await refreshGame()
      try {
        const a = await invoke<AnalysisResult | null>('get_analysis')
        if (!cancelled) useAnalysisStore.getState().set(a)
      } catch {
        /* ignore */
      }
      // Game history: load all records once at startup. The History
      // tab is the only consumer; loading lazily there forces a
      // round-trip on first nav, so do it eagerly while the bridge
      // is warm. Explicit large `limit` defends against any backend
      // version that still applies a default cap on `limit=0`.
      try {
        useHistoryStore.getState().setLoading(true)
        const records = await invoke<GameRecord[]>('list_game_history', {
          filter: null,
          limit: 1_000_000,
          offset: 0,
        })
        if (!cancelled) useHistoryStore.getState().setRecords(records)
      } catch {
        /* ignore: store stays empty */
      } finally {
        useHistoryStore.getState().setLoading(false)
      }
    })()

    const clearGame = () => {
      pendingAdvisory = null
      useGameStore.getState().setGame(null)
      useGameStore.getState().setView(null)
      useAnalysisStore.getState().set(null)
      useNotifyStore.getState().clearGame()
    }
    const recoveryStatus = (s: GameRecoveryStatus) => {
      const pending = pendingAdvisory
      useCaptureStore.getState().setRecovery(s)
      if (s.phase === 'ready') void refreshGame()
      else if (s.phase !== 'inactive') clearGame()
      pendingAdvisory = null
      if (pending) {
        const epoch = pending.meta?.recovery_epoch
        if (epoch === s.epoch && s.phase === 'ready') {
          useNotifyStore.getState().pushResponse(pending)
        } else if (typeof epoch === 'number' && (
          epoch > s.epoch || (epoch === s.epoch && (
            s.phase === 'recovering' || s.phase === 'waiting_round'
          ))
        )) {
          pendingAdvisory = pending
        }
      }
    }
    invoke<GameRecoveryStatus>('get_game_recovery_status').then(s => { if (!cancelled) recoveryStatus(s) }).catch(() => {})
    listen<GameRecoveryStatus>('game-recovery-status', recoveryStatus).then(u => unlistens.push(u))
    listen('game-invalidated', clearGame).then(u => unlistens.push(u))

    listen<MjaiEvent>('mjai-event', (e) => {
      pendingAdvisory = null
      useNotifyStore.getState().pushEvent(e)
      void refreshGame()
    }).then((u) => unlistens.push(u))

    listen<AnalysisResult>('analysis-result', (a) => {
      const phase = useCaptureStore.getState().recovery.phase
      if (phase === 'ready' || phase === 'inactive') useAnalysisStore.getState().set(a)
    }).then((u) => unlistens.push(u))

    listen<BotStatus>('bot-status', (s) => {
      useBotStore.getState().setStatus(s)
    }).then((u) => unlistens.push(u))

    listen<CaptureStatus>('capture-status', (s) => {
      useCaptureStore.getState().set(s)
    }).then((u) => unlistens.push(u))

    listen<BotResponse>('bot-response', (r) => {
      const { phase, epoch } = useCaptureStore.getState().recovery
      if (r.meta?.advisory_only === true) {
        const advisoryEpoch = r.meta.recovery_epoch
        if (typeof advisoryEpoch !== 'number' || advisoryEpoch < epoch) return
        if (advisoryEpoch === epoch && phase === 'ready') {
          useNotifyStore.getState().pushResponse(r)
        } else if (advisoryEpoch > epoch || phase === 'recovering' || phase === 'waiting_round') {
          pendingAdvisory = r
        }
        return
      }
      if (phase === 'ready' || phase === 'inactive') useNotifyStore.getState().pushResponse(r)
    }).then((u) => unlistens.push(u))

    // The overlay window can turn itself off (its × button). Mirror that back
    // into the config store so the Game page's toggle doesn't keep claiming the
    // overlay is open.
    listen<OverlayConfig>('overlay-config', (o) => {
      useConfigStore.getState().setOverlay(o)
    }).then((u) => unlistens.push(u))

    listen<HistoryEvent>('history-recorded', (ev) => {
      if (ev.kind === 'recorded') {
        useHistoryStore.getState().prepend(ev.record)
      } else if (ev.kind === 'deleted') {
        useHistoryStore.getState().remove(ev.id)
      }
    }).then((u) => unlistens.push(u))

    listen<boolean>('autoplay-enabled', (enabled) => {
      useConfigStore.getState().setAutoplayEnabled(enabled)
    }).then((u) => unlistens.push(u))

    listen<Notification>('notify', (n) => {
      useNotifyStore.getState().pushToast(n)
      toast[TOAST_SEVERITY[n.level]](n.title, {
        description: n.body,
        id: n.id,
        ...(n.sticky ? { duration: Infinity } : {}),
      })
    }).then((u) => unlistens.push(u))

    return () => {
      cancelled = true
      unlistens.forEach((u) => u())
    }
  }, [])
}
