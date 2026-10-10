import { useEffect, useState, type CSSProperties } from 'react'
import { useTranslation } from 'react-i18next'
import { X } from 'lucide-react'
import { BotShowList } from '@/components/BotShowList'
import { AutoplayRecordRow } from '@/components/AutoplayRecordRow'
import { useAutoplayStatus } from '@/hooks/useAutoplayStatus'
import { useAutoplayStatusStore } from '@/stores/autoplayStatusStore'
import { pickShow, visibleItems } from '@/lib/botShow'
import { invoke, listen } from '@/lib/tauri'
import type { AppConfig, BotResponse, GameRecoveryStatus, OverlayConfig, ShowMeta } from '@/types'

// The always-on-top overlay window's entire UI. Mounted by main.tsx instead of
// the router when the window label is `overlay` — no sidebar, no statusbar, no
// route tree, and deliberately no `useTauriBridge`: this window subscribes only
// to suggestions, their invalidation, and overlay configuration.
//
// The window itself is transparent; the rounded card below is the only thing
// that gets drawn. Everything inside the card is `pointer-events: none` so that
// a mousedown anywhere lands on the drag region and moves the window — except
// the close button, which opts back in.

// Only used if `get_config` fails — a blank overlay would be worse than a
// default-looking one. Mirrors `OverlayConfig::default()`.
const FALLBACK: OverlayConfig = {
  enabled: true,
  top_n: 3,
  event_count: 4,
  font_size: 14,
  opacity: 0.95,
  always_on_top: true,
}

export function Overlay() {
  useAutoplayStatus()
  const autoplay = useAutoplayStatusStore()
  const { t } = useTranslation()
  const [show, setShow] = useState<ShowMeta | null>(null)
  const [advisory, setAdvisory] = useState(false)
  const [cfg, setCfg] = useState<OverlayConfig>(FALLBACK)

  useEffect(() => {
    const unlistens: Array<() => void> = []
    let cancelled = false
    let showingAdvisory = false
    let recovery: GameRecoveryStatus | null = null
    let pendingAdvisory: BotResponse | null = null
    const clearAdvisory = () => {
      pendingAdvisory = null
      if (!showingAdvisory) return
      showingAdvisory = false
      setAdvisory(false)
      setShow(null)
    }

    // The window is opened by the backend, so config is already on disk;
    // `overlay-config` keeps us in step with later edits in Settings.
    invoke<AppConfig>('get_config')
      .then((c) => {
        if (!cancelled) setCfg(c.overlay)
      })
      .catch(() => {
        /* keep the fallback — a blank overlay would be worse than a default one */
      })

    const showResponse = (r: BotResponse) => {
      const s = pickShow(r.meta)
      // Responses without a `show` block (e.g. a bare `none`) must not wipe
      // the last real suggestion off the screen.
      if (s) {
        showingAdvisory = r.meta?.advisory_only === true
        setAdvisory(showingAdvisory)
        setShow(s)
      }
    }
    const recoveryStatus = (s: GameRecoveryStatus) => {
      recovery = s
      const pending = pendingAdvisory
      if (s.phase !== 'ready') clearAdvisory()
      pendingAdvisory = null
      if (!pending) return
      const epoch = pending.meta?.recovery_epoch
      if (epoch === s.epoch && s.phase === 'ready') showResponse(pending)
      else if (typeof epoch === 'number' && (
        epoch > s.epoch || (epoch === s.epoch && (
          s.phase === 'recovering' || s.phase === 'waiting_round'
        ))
      )) pendingAdvisory = pending
    }
    invoke<GameRecoveryStatus>('get_game_recovery_status')
      .then(s => { if (!cancelled) recoveryStatus(s) }).catch(() => {})
    listen<BotResponse>('bot-response', (r) => {
      if (r.meta?.advisory_only !== true) {
        showResponse(r)
        return
      }
      const epoch = r.meta.recovery_epoch
      if (typeof epoch !== 'number' || (recovery && epoch < recovery.epoch)) return
      if (recovery && epoch === recovery.epoch && recovery.phase === 'ready') showResponse(r)
      else if (!recovery || epoch > recovery.epoch ||
        recovery.phase === 'recovering' || recovery.phase === 'waiting_round') pendingAdvisory = r
    }).then((u) => unlistens.push(u))

    listen('mjai-event', clearAdvisory).then((u) => unlistens.push(u))
    listen('game-invalidated', clearAdvisory).then((u) => unlistens.push(u))
    listen<GameRecoveryStatus>('game-recovery-status', recoveryStatus).then((u) => unlistens.push(u))

    listen<OverlayConfig>('overlay-config', (c) => setCfg(c)).then((u) => unlistens.push(u))

    return () => {
      cancelled = true
      unlistens.forEach((u) => u())
    }
  }, [])

  const items = visibleItems(show, cfg.top_n)
  const eventCount = Math.min(10, Math.max(1, cfg.event_count ?? 4))
  const records = autoplay.records.slice(-eventCount).reverse()
  // Keep these base heights in sync with ipc::overlay; CSS pixels are logical pixels.
  const scale = Math.min(24, Math.max(12, cfg.font_size ?? 14)) / 14
  const smallText = { fontSize: 12 * scale, lineHeight: `${16 * scale}px` }

  // Closing the overlay persists `enabled = false`, so it stays closed across
  // restarts. Settings is the way back.
  const close = () => {
    void invoke('set_overlay_enabled', { enabled: false }).catch(() => {})
  }

  return (
    <div
      data-tauri-drag-region
      className="h-screen w-screen overflow-hidden p-1 select-none cursor-grab active:cursor-grabbing"
      style={{ '--overlay-scale': scale, padding: 4 * scale } as CSSProperties}
    >
      <div
        data-tauri-drag-region
        className="flex h-full w-full flex-col overflow-hidden rounded-lg border border-border bg-background shadow-lg [&_*]:pointer-events-none"
        style={{ opacity: cfg.opacity }}
      >
        <header
          data-tauri-drag-region
          className="flex shrink-0 items-center gap-1 px-2 pt-1.5 pb-1"
          style={{ gap: 4 * scale, padding: `${6 * scale}px ${8 * scale}px ${4 * scale}px` }}
        >
          <span
            className="flex-1 truncate text-[11px] font-medium text-muted-foreground"
            style={{ fontSize: 11 * scale, lineHeight: `${16.5 * scale}px` }}
          >
            {show?.title ?? t('tile.bot_show_default_title')}
          </span>
          <button
            type="button"
            onClick={close}
            aria-label={t('overlay.close')}
            title={t('overlay.close')}
            className="shrink-0 cursor-pointer text-muted-foreground hover:text-foreground"
            style={{ pointerEvents: 'auto' }}
          >
            <X className="size-3.5" style={{ width: 14 * scale, height: 14 * scale }} />
          </button>
        </header>

        {advisory && (
          <p className="shrink-0 px-2 text-amber-500" style={smallText}>
            {t('recovery.advisory_only')}
          </p>
        )}

        {/* min-h-0 is what gives this a definite height to hand down: without it
            a flex child refuses to shrink below its content, the rows never get
            a height to split, and the list collapses to hugging its content. */}
        <div className="min-h-0 flex-1 overflow-hidden px-1.5 pb-1.5" style={{ padding: `0 ${6 * scale}px ${6 * scale}px` }}>
          {items.length === 0 ? (
            <span className="px-1 text-xs text-muted-foreground" style={smallText}>{t('overlay.empty')}</span>
          ) : (
            <BotShowList items={items} variant="overlay" />
          )}
        </div>
        {autoplay.enabled && (
          <section
            className="shrink-0 overflow-hidden border-t border-border px-2"
            style={{ height: (28 + eventCount * 40) * scale, paddingInline: 8 * scale }}
          >
            <h2
              className="h-7 text-[11px] leading-7 font-medium text-muted-foreground"
              style={{ height: 28 * scale, fontSize: 11 * scale, lineHeight: `${28 * scale}px` }}
            >
              {t('autoplay_status.title')}
            </h2>
            {records.length === 0 ? (
              <p className="text-xs text-muted-foreground" style={smallText}>{t('autoplay_status.empty')}</p>
            ) : records.map((record) => (
              <div key={record.id} className="h-10 overflow-hidden" style={{ height: 40 * scale }}>
                <AutoplayRecordRow record={record} overlay />
              </div>
            ))}
          </section>
        )}
      </div>
    </div>
  )
}
