import { useTranslation } from 'react-i18next'
import { TileFrame } from '@/components/TileFrame'
import { useNotifyStore } from '@/stores/notifyStore'
import { fmtTime } from '@/lib/format'
import type { Breakpoint } from '@/tiles/defaults'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { AutoplayRecordRow } from '@/components/AutoplayRecordRow'
import { useAutoplayStatusStore } from '@/stores/autoplayStatusStore'
import { cn } from '@/lib/utils'

export function EventsTile({ bp }: { bp: Breakpoint }) {
  const { t } = useTranslation()
  const events = useNotifyStore((s) => s.events)
  const recent = events.slice().reverse().slice(0, 50)
  const records = useAutoplayStatusStore((s) => s.records)

  return (
    <TileFrame id="events" title={t('tile.events')} bp={bp} contentClassName="p-0 overflow-hidden">
      <Tabs defaultValue="game" className="h-full min-h-0 gap-0">
        <TabsList className="shrink-0 mx-2 my-1">
          <TabsTrigger value="game">{t('autoplay_status.game_events')}</TabsTrigger>
          <TabsTrigger value="autoplay">{t('autoplay_status.title')}</TabsTrigger>
        </TabsList>
        <TabsContent value="game" className="min-h-0 overflow-auto">
      <ul className="flex flex-col text-xs font-mono divide-y divide-border">
        {recent.length === 0 ? (
          <li className="px-3 py-2 text-muted-foreground">{t('tile.events_empty')}</li>
        ) : recent.map((e) => (
          <li key={e._seq} className="flex items-center gap-2 px-3 py-1.5">
            <span className="text-muted-foreground text-[10px]">{fmtTime(new Date(e._ts))}</span>
            <span className="font-medium">{e.type}</span>
            {'actor' in e && typeof e.actor === 'number' && (
              <span className="text-muted-foreground">@{e.actor}</span>
            )}
            {'pai' in e && typeof e.pai === 'string' && (
              <span className="text-emerald-400">{e.pai}</span>
            )}
          </li>
        ))}
      </ul>
        </TabsContent>
        <TabsContent value="autoplay" className="min-h-0 overflow-auto">
          <ul className="divide-y divide-border">
            {records.length === 0 ? (
              <li className="px-3 py-2 text-xs text-muted-foreground">{t('autoplay_status.empty')}</li>
            ) : records.slice().reverse().map((record) => (
              <li key={record.id} className={cn('px-3 py-1.5', record.phase === 'unconfirmed' && 'text-red-500')}>
                <div className={cn('text-[10px] font-mono', record.phase === 'unconfirmed' ? 'text-red-500' : 'text-muted-foreground')}>
                  {fmtTime(new Date(record.created_at))} · {t('autoplay_status.round')} {record.round}
                </div>
                <AutoplayRecordRow record={record} />
              </li>
            ))}
          </ul>
        </TabsContent>
      </Tabs>
    </TileFrame>
  )
}
