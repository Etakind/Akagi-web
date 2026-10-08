import { useTranslation } from 'react-i18next'
import type { AutoplayRecord } from '@/types'
import { autoplayCountdown } from '@/lib/autoplayStatus'
import { cn } from '@/lib/utils'

export function AutoplayRecordRow({ record, overlay = false }: { record: AutoplayRecord; overlay?: boolean }) {
  const { t } = useTranslation()
  const tile = (pai: string) => {
    const match = /^(\d)([mps])(r?)$/.exec(pai)
    if (!match) return t(`autoplay_status.tile.${pai}`, { defaultValue: pai })
    return t(`autoplay_status.tile.${match[2]}`, { number: match[1] })
      + (match[3] ? t('autoplay_status.tile.red') : '')
  }
  const tiles = record.action.pai ? [record.action.pai] : record.action.consumed ?? []
  const countdown = record.phase === 'scheduled' && record.remaining_ms != null && record.remaining_ms > 0
  const reason = record.reason?.startsWith('server_error:')
    ? t('autoplay_status.reason.server_error', { code: record.reason.split(':')[1] })
    : record.reason ? t(`autoplay_status.reason.${record.reason}`, { defaultValue: record.reason }) : ''

  return (
    <div
      className={cn('min-w-0 text-xs leading-5', record.phase === 'unconfirmed' && 'text-red-500')}
      style={overlay ? {
        fontSize: 'calc(12px * var(--overlay-scale, 1))',
        lineHeight: 'calc(20px * var(--overlay-scale, 1))',
      } : undefined}
      title={reason}
    >
      <div className="truncate font-medium">
        {t(`autoplay_status.action.${record.action.type}`, { defaultValue: record.action.type })}
        {tiles.length > 0 && ` ${tiles.map(tile).join(' ')}`}
        {record.step !== 'input' && ` · ${t(`autoplay_status.step.${record.step}`)}`}
        {record.retry > 0 && ` · ${t('autoplay_status.retry', { count: record.retry })}`}
      </div>
      <div className={cn('truncate tabular-nums', record.phase === 'unconfirmed' ? 'text-red-500' : 'text-muted-foreground')}>
        {countdown ? autoplayCountdown(record.remaining_ms!) : t(`autoplay_status.phase.${record.phase === 'scheduled' ? 'preparing' : record.phase}`)}
        {!countdown && reason && ` (${reason})`}
      </div>
    </div>
  )
}
