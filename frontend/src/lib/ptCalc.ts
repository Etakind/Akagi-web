// Pure PT (cumulative-points) calculation. Three rule families plus a
// custom escape hatch:
//
// - Majsoul: `(score - start) / 1000 + uma + dan_bonus(rank, lobby, dan, mode)`
//   * uma 4p = [+15, +5, -5, -15], 3p = [+15, 0, -15]
//   * dan_bonus rank 1/2 from lobby table; rank 3 = 0; last rank from
//     dan-penalty table (already negative)
//   * `start` = 25_000 (4p) / 35_000 (3p)
//
// - Tenhou: PT is read directly from a [dan][mode][rank-1] cell.
//
// - Custom: user supplies [uma per rank], [dan-bonus per rank]. Formula
//   matches Majsoul minus the lobby/dan tables: PT =
//   (score - start) / 1000 + uma[rank-1] + danBonus[rank-1].
//
// Observer-mode games (no `our_seat`) return 0 — caller should typically
// skip plotting those.

import type { GameRecord } from '@/types'
import {
  MAJSOUL_DAN_PENALTY_3P,
  MAJSOUL_DAN_PENALTY_4P,
  MAJSOUL_LOBBY_3P,
  MAJSOUL_LOBBY_4P,
  type MajsoulDan,
  type MajsoulLobby,
  TENHOU_3P,
  TENHOU_4P,
  type TenhouDan,
} from '@/lib/ptTables'

export type PtRule =
  | { kind: 'majsoul'; lobby: MajsoulLobby; dan: MajsoulDan }
  | { kind: 'tenhou'; dan: TenhouDan }
  | {
      kind: 'custom'
      /** [+1st, +2nd, +3rd, +4th] for 4p, [+1st, +2nd, +3rd] for 3p. */
      uma4p: [number, number, number, number]
      uma3p: [number, number, number]
      /** Same shape as `uma*`. */
      danBonus4p: [number, number, number, number]
      danBonus3p: [number, number, number]
    }

export const DEFAULT_CUSTOM_RULE: Extract<PtRule, { kind: 'custom' }> = {
  kind: 'custom',
  uma4p: [15, 5, -5, -15],
  uma3p: [15, 0, -15],
  danBonus4p: [0, 0, 0, 0],
  danBonus3p: [0, 0, 0],
}

const STARTING_SCORE_4P = 25_000
const STARTING_SCORE_3P = 35_000
const UMA_4P: [number, number, number, number] = [15, 5, -5, -15]
const UMA_3P: [number, number, number] = [15, 0, -15]

/**
 * PT delta for one game under the chosen rule. Returns 0 when the
 * record can't be scored (observer mode, missing rank, mode mismatch
 * with the rule). The caller decides whether to plot or skip.
 */
export function computePt(record: GameRecord, rule: PtRule): number {
  if (record.our_rank == null || record.our_delta == null) return 0
  const rank = record.our_rank
  const isHanchan =
    record.kyoku_mode === 'east_south' || record.kyoku_mode === 'other'
  const modeIdx = isHanchan ? 1 : 0
  const np = record.num_players

  switch (rule.kind) {
    case 'majsoul':
      return majsoulPt(record, rule, rank, modeIdx, np)
    case 'tenhou':
      return tenhouPt(record, rule, rank, modeIdx, np)
    case 'custom':
      return customPt(record, rule, rank, np)
  }
}

function majsoulPt(
  record: GameRecord,
  rule: Extract<PtRule, { kind: 'majsoul' }>,
  rank: number,
  modeIdx: 0 | 1,
  np: number,
): number {
  const start = np === 3 ? STARTING_SCORE_3P : STARTING_SCORE_4P
  const seat = record.our_seat
  const score = seat == null ? start : record.final_scores[seat]
  const baseTerm = (score - start) / 1000

  if (np === 3) {
    const uma = UMA_3P[rank - 1] ?? 0
    let danBonus = 0
    if (rank === 1) {
      danBonus = MAJSOUL_LOBBY_3P[rule.lobby][modeIdx]
    } else if (rank === 3) {
      // Last place: dan penalty (already negative).
      danBonus = MAJSOUL_DAN_PENALTY_3P[rule.dan][modeIdx]
    }
    return Math.ceil(baseTerm + uma + danBonus)
  }

  const uma = UMA_4P[rank - 1] ?? 0
  let danBonus = 0
  if (rank === 1 || rank === 2) {
    const row = MAJSOUL_LOBBY_4P[rule.lobby][rank - 1]
    danBonus = row[modeIdx]
  } else if (rank === 4) {
    danBonus = MAJSOUL_DAN_PENALTY_4P[rule.dan][modeIdx]
  }
  return Math.ceil(baseTerm + uma + danBonus)
}

function tenhouPt(
  _record: GameRecord,
  rule: Extract<PtRule, { kind: 'tenhou' }>,
  rank: number,
  modeIdx: 0 | 1,
  np: number,
): number {
  const table = np === 3 ? TENHOU_3P[rule.dan] : TENHOU_4P[rule.dan]
  const cells = table[modeIdx]
  return cells[rank - 1] ?? 0
}

function customPt(
  record: GameRecord,
  rule: Extract<PtRule, { kind: 'custom' }>,
  rank: number,
  np: number,
): number {
  const start = np === 3 ? STARTING_SCORE_3P : STARTING_SCORE_4P
  const seat = record.our_seat
  const score = seat == null ? start : record.final_scores[seat]
  const baseTerm = (score - start) / 1000
  const uma = np === 3 ? rule.uma3p[rank - 1] : rule.uma4p[rank - 1]
  const danBonus =
    np === 3 ? rule.danBonus3p[rank - 1] : rule.danBonus4p[rank - 1]
  return baseTerm + (uma ?? 0) + (danBonus ?? 0)
}

/**
 * Cumulative PT series in chronological order (oldest → newest). Each
 * point is `{x: index, y: cumulative_pt, record}`. Observer / unrankable
 * games are skipped (don't move the curve).
 */
export type CumulativePoint = {
  index: number
  cumulative: number
  delta: number
  record: GameRecord
}

export function cumulativePtSeries(
  records: GameRecord[],
  rule: PtRule,
): CumulativePoint[] {
  // Oldest first.
  const sorted = [...records].sort(
    (a, b) =>
      new Date(a.started_at).valueOf() - new Date(b.started_at).valueOf(),
  )
  const out: CumulativePoint[] = []
  let acc = 0
  let i = 0
  for (const r of sorted) {
    if (r.our_rank == null) continue
    const delta = computePt(r, rule)
    acc += delta
    out.push({ index: i++, cumulative: acc, delta, record: r })
  }
  return out
}

/**
 * Rank distribution (rank → count) over a record set. Output array has
 * length `np` (3 or 4); index 0 = 1st place. Records without rank are
 * skipped.
 */
export function rankDistribution(
  records: GameRecord[],
  np: 3 | 4,
): number[] {
  const counts = new Array<number>(np).fill(0)
  for (const r of records) {
    if (r.num_players !== np) continue
    if (r.our_rank == null) continue
    if (r.our_rank >= 1 && r.our_rank <= np) {
      counts[r.our_rank - 1] += 1
    }
  }
  return counts
}
