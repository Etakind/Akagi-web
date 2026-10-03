// Per-game detail dialog. Shows the recorded player's perspective: who
// played, final standings (rank / score / Δ), and the per-game stats.
// Mirrors the GameRecord shape directly — no re-aggregation needed.

import { useTranslation } from 'react-i18next'

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { matchGameId, roomLabelKey } from '@/lib/matchInfo'
import type { GameRecord } from '@/types'

const STARTING_4P = 25_000
const STARTING_3P = 35_000

export function GameDetailDialog({
  record,
  onOpenChange,
}: {
  record: GameRecord | null
  onOpenChange: (open: boolean) => void
}) {
  const { t } = useTranslation()
  const open = record !== null
  const start = record?.num_players === 3 ? STARTING_3P : STARTING_4P
  const room = roomLabelKey(record?.match_info)
  const gameId = matchGameId(record?.match_info)

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      {/* sm:max-w-2xl, not max-w-2xl: DialogContent ships its own
          `sm:max-w-sm`, and only a same-variant class replaces it through
          cn's tailwind-merge — a base max-w-* silently loses at ≥sm. */}
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>{t('history.detail.title')}</DialogTitle>
          {record && (
            <DialogDescription className="font-mono text-xs break-all">
              {record.id}
            </DialogDescription>
          )}
        </DialogHeader>
        {record && (
          <div className="space-y-4">
            <Section title={t('history.detail.started_at')}>
              <span className="font-mono text-sm">
                {new Date(record.started_at).toLocaleString()}
              </span>
            </Section>
            <Section title={t('history.detail.ended_at')}>
              <span className="font-mono text-sm">
                {new Date(record.ended_at).toLocaleString()}
              </span>
            </Section>
            <Section title={t('history.detail.platform')}>
              <span className="text-sm">
                {t(`platform.${record.platform}`)}
              </span>
            </Section>
            <Section title={t('history.detail.mode')}>
              <span className="text-sm">
                {record.num_players}p ·{' '}
                {record.kyoku_mode === 'east_only'
                  ? t('history.filter.east_only')
                  : record.kyoku_mode === 'east_south'
                    ? t('history.filter.east_south')
                    : t('history.filter.other')}
              </span>
            </Section>
            {room && (
              <Section title={t('history.detail.room')}>
                <span className="text-sm">{t(room.key, room.params)}</span>
              </Section>
            )}
            {gameId && (
              <Section title={t('history.detail.game_id')}>
                <div className="flex items-center gap-2">
                  <span className="font-mono text-xs break-all">{gameId}</span>

                </div>
              </Section>
            )}


            <Section title={t('history.detail.final')}>
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead className="w-12">#</TableHead>
                    <TableHead>{t('history.table.names')}</TableHead>
                    <TableHead className="text-right">
                      {t('history.table.end_score')}
                    </TableHead>
                    <TableHead className="text-right">Δ</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {/* Order rows by rank (1..N) so the dialog reads top-to-bottom. */}
                  {record.final_ranks
                    .map((rank, seat) => ({ rank, seat }))
                    .sort((a, b) => a.rank - b.rank)
                    .map(({ rank, seat }) => {
                      const score = record.final_scores[seat]
                      const delta = score - start
                      const isUs = record.our_seat === seat
                      return (
                        <TableRow
                          key={seat}
                          className={isUs ? 'bg-accent/40' : ''}
                        >
                          <TableCell>{rank}</TableCell>
                          <TableCell>
                            {record.names[seat] || t('history.detail.seat_fallback', { seat })}
                            {isUs && (
                              <span className="ml-1 text-xs text-muted-foreground">
                                {t('history.detail.you_marker')}
                              </span>
                            )}
                          </TableCell>
                          <TableCell className="text-right font-mono">
                            {score.toLocaleString()}
                          </TableCell>
                          <TableCell
                            className={
                              'text-right font-mono ' +
                              (delta > 0
                                ? 'text-emerald-500'
                                : delta < 0
                                  ? 'text-red-500'
                                  : '')
                            }
                          >
                            {delta >= 0 ? '+' : ''}
                            {delta.toLocaleString()}
                          </TableCell>
                        </TableRow>
                      )
                    })}
                </TableBody>
              </Table>
            </Section>

            <Section title={t('history.detail.stats')}>
              <div className="grid grid-cols-2 md:grid-cols-3 gap-x-6 gap-y-1 text-xs">
                <Stat label={t('mahjong.round')} value={record.stats.round} />
                <Stat label={t('mahjong.oya')} value={record.stats.oya} />
                <Stat label={t('mahjong.agari')} value={record.stats.agari} />
                <Stat label={t('mahjong.houjuu')} value={record.stats.houjuu} />
                <Stat label={t('mahjong.riichi')} value={record.stats.riichi} />
                <Stat label={t('mahjong.fuuro')} value={record.stats.fuuro} />
                <Stat label={t('mahjong.ryukyoku')} value={record.stats.ryukyoku} />
                <Stat label={t('mahjong.yakuman')} value={record.stats.yakuman} />
                <Stat
                  label={t('mahjong.nagashi_mangan')}
                  value={record.stats.nagashi_mangan}
                />
              </div>
            </Section>
          </div>
        )}
      </DialogContent>
    </Dialog>
  )
}

function Section({
  title,
  children,
}: {
  title: string
  children: React.ReactNode
}) {
  return (
    <div className="space-y-1">
      <div className="text-xs uppercase tracking-wider text-muted-foreground">
        {title}
      </div>
      {children}
    </div>
  )
}

function Stat({ label, value }: { label: string; value: number }) {
  return (
    <div className="flex items-baseline justify-between border-b border-border/40 py-0.5">
      <span className="text-muted-foreground">{label}</span>
      <span className="font-mono">{value}</span>
    </div>
  )
}
