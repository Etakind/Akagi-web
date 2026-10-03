import { useTranslation } from 'react-i18next'
import { useCaptureStore } from '@/stores/captureStore'
import { useBotStore } from '@/stores/botStore'
import {
  BOT_LABEL,
  botTone,
  CAPTURE_LABEL,
  captureTone,
  TONE_DOT,
  type IndicatorTone,
} from '@/lib/statusIndicators'

function IndicatorDot({
  tone,
  label,
  title,
}: {
  tone: IndicatorTone
  label: string
  title?: string
}) {
  return (
    <span className="flex items-center gap-1.5" title={title}>
      <span className={`h-1.5 w-1.5 rounded-full ${TONE_DOT[tone]}`} />
      {label}
    </span>
  )
}

export function Statusbar() {
  const { t } = useTranslation()
  const capture = useCaptureStore((s) => s.status)
  const bot = useBotStore((s) => s.status)

  // Capture LED — "is Akagi reading the game?" (proxy / capture transport).
  // Tooltip surfaces the capture descriptor, or the error message on failure,
  // so the user can see *why* it's down.
  const capTone = captureTone(capture.state)
  const captureTitle =
    capture.state === 'error'
      ? capture.error
      : capture.state === 'running' || capture.state === 'starting'
        ? capture.descriptor
        : undefined

  // Bot LED — "is the recommendation engine up?". Independent of capture: the
  // bot can error (broken env, failed spawn) while capture keeps running.
  const botT = botTone(bot.state)
  const botTitle =
    bot.state === 'error'
      ? bot.error
      : bot.state === 'loading'
        ? `${bot.bot} · ${t(`status.stage_${bot.stage}`)}`
        : bot.state === 'ready' || bot.state === 'stopped'
          ? bot.bot
          : undefined

  return (
    <footer className="flex items-center justify-between border-t border-border px-4 py-1.5 text-xs text-muted-foreground bg-muted/30">
      <IndicatorDot
        tone={capTone}
        label={t(CAPTURE_LABEL[capTone])}
        title={captureTitle}
      />
      <span className="flex items-center gap-3">
        <IndicatorDot
          tone={botT}
          label={t(BOT_LABEL[bot.state])}
          title={botTitle}
        />
      </span>
    </footer>
  )
}
