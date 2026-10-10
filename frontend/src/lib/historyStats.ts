// Aggregation helpers over GameRecord[]. Mirrors the totals/derivations
// from Mortal's `libriichi` Stat module but consumes per-game `GameStats`
// already produced by the backend aggregator.
//
// All rates returned in the [0, 1] range. `null` when the denominator
// is zero (frontend renders these as "—").

import type { GameRecord, GameStats } from '@/types'

export type AggregateStats = {
  games: number
  rounds: number
  ranks: number[] // length = max_num_players seen (4 if any 4p, else 3)

  // From-rate base counts
  agari: number
  houjuu: number
  riichi: number
  fuuro: number
  ryukyoku: number
  oya: number
  agari_as_oya: number
  houjuu_to_oya: number

  // Δscore totals (for averages)
  agari_point_oya: number
  agari_point_ko: number
  riichi_agari_point: number
  fuuro_agari_point: number
  dama_agari_point: number
  riichi_agari: number
  fuuro_agari: number
  dama_agari: number
  riichi_houjuu: number
  fuuro_houjuu: number

  houjuu_point_to_oya: number
  houjuu_point_to_ko: number

  agari_jun: number
  riichi_agari_jun: number
  fuuro_agari_jun: number
  dama_agari_jun: number
  houjuu_jun: number
  riichi_jun: number
  fuuro_num: number
  riichi_point: number

  yakuman: number
  nagashi_mangan: number

  ranksum: number // for avg rank
  ranksum_count: number
  tobi_count: number
}

const ZERO_STATS: GameStats = {
  round: 0,
  oya: 0,

  fuuro: 0,
  fuuro_num: 0,
  fuuro_point: 0,
  fuuro_agari: 0,
  fuuro_agari_jun: 0,
  fuuro_agari_point: 0,
  fuuro_houjuu: 0,

  agari: 0,
  agari_as_oya: 0,
  agari_jun: 0,
  agari_point_oya: 0,
  agari_point_ko: 0,

  houjuu: 0,
  houjuu_jun: 0,
  houjuu_to_oya: 0,
  houjuu_point_to_oya: 0,
  houjuu_point_to_ko: 0,

  riichi: 0,
  riichi_as_oya: 0,
  riichi_jun: 0,
  riichi_agari: 0,
  riichi_agari_point: 0,
  riichi_agari_jun: 0,
  riichi_houjuu: 0,
  riichi_ryukyoku: 0,
  riichi_point: 0,
  chasing_riichi: 0,
  riichi_got_chased: 0,

  dama_agari: 0,
  dama_agari_jun: 0,
  dama_agari_point: 0,

  ryukyoku: 0,
  ryukyoku_point: 0,

  yakuman: 0,
  nagashi_mangan: 0,
}

const STARTING_4P = 25_000
const STARTING_3P = 35_000

export function aggregateStats(records: GameRecord[]): AggregateStats {
  // Pick rank-vector length: 4 if any 4p game appears, else 3 (or default 4 when empty).
  const has4p = records.some((r) => r.num_players === 4)
  const rankLen = has4p || records.length === 0 ? 4 : 3
  const ranks = new Array<number>(rankLen).fill(0)

  const acc = { ...ZERO_STATS }
  let games = 0
  let oya = 0
  let agari_as_oya = 0
  let houjuu_to_oya = 0
  let ranksum = 0
  let ranksum_count = 0
  let tobi_count = 0

  for (const r of records) {
    if (r.partial) continue
    games += 1
    addStats(acc, r.stats)
    oya += r.stats.oya
    agari_as_oya += r.stats.agari_as_oya
    houjuu_to_oya += r.stats.houjuu_to_oya

    if (r.our_rank != null && r.our_rank >= 1 && r.our_rank <= rankLen) {
      ranks[r.our_rank - 1] += 1
      ranksum += r.our_rank
      ranksum_count += 1
    }
    if (r.our_seat != null) {
      const score = r.final_scores[r.our_seat]
      if (score != null && score < 0) tobi_count += 1
      // Tobi check: any seat < 0 also marks (some platforms call it 飛び for the loser).
    }
  }

  return {
    games,
    rounds: acc.round,
    ranks,
    agari: acc.agari,
    houjuu: acc.houjuu,
    riichi: acc.riichi,
    fuuro: acc.fuuro,
    ryukyoku: acc.ryukyoku,
    oya,
    agari_as_oya,
    houjuu_to_oya,
    agari_point_oya: acc.agari_point_oya,
    agari_point_ko: acc.agari_point_ko,
    riichi_agari_point: acc.riichi_agari_point,
    fuuro_agari_point: acc.fuuro_agari_point,
    dama_agari_point: acc.dama_agari_point,
    riichi_agari: acc.riichi_agari,
    fuuro_agari: acc.fuuro_agari,
    dama_agari: acc.dama_agari,
    riichi_houjuu: acc.riichi_houjuu,
    fuuro_houjuu: acc.fuuro_houjuu,
    houjuu_point_to_oya: acc.houjuu_point_to_oya,
    houjuu_point_to_ko: acc.houjuu_point_to_ko,
    agari_jun: acc.agari_jun,
    riichi_agari_jun: acc.riichi_agari_jun,
    fuuro_agari_jun: acc.fuuro_agari_jun,
    dama_agari_jun: acc.dama_agari_jun,
    houjuu_jun: acc.houjuu_jun,
    riichi_jun: acc.riichi_jun,
    fuuro_num: acc.fuuro_num,
    riichi_point: acc.riichi_point,
    yakuman: acc.yakuman,
    nagashi_mangan: acc.nagashi_mangan,
    ranksum,
    ranksum_count,
    tobi_count,
  }
}

function addStats(acc: GameStats, s: GameStats) {
  ;(Object.keys(acc) as Array<keyof GameStats>).forEach((k) => {
    acc[k] = acc[k] + s[k]
  })
}

export function rate(num: number, den: number): number | null {
  if (den === 0) return null
  return num / den
}

/** Average rank, e.g. 2.42. `null` when no rank-bearing games. */
export function avgRank(stats: AggregateStats): number | null {
  if (stats.ranksum_count === 0) return null
  return stats.ranksum / stats.ranksum_count
}

/** Total Δscore across all games (includes 4p and 3p mixed). */
export function totalDelta(records: GameRecord[]): number {
  let total = 0
  for (const r of records) {
    if (r.partial) continue
    if (r.our_seat == null) continue
    const start = r.num_players === 3 ? STARTING_3P : STARTING_4P
    total += (r.final_scores[r.our_seat] ?? start) - start
  }
  return total
}
