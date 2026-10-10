import { describe, expect, it } from 'vitest'
import type { GameRecord } from '@/types'
import { aggregateStats } from './historyStats'
import { cumulativePtSeries, rankDistribution, DEFAULT_CUSTOM_RULE } from './ptCalc'

describe('partial capture statistics', () => {
  it('never counts incomplete games as whole games or placements', () => {
    const partial = { partial: true, num_players: 4, our_rank: 1, started_at: new Date().toISOString() } as GameRecord
    expect(aggregateStats([partial]).games).toBe(0)
    expect(rankDistribution([partial], 4)).toEqual([0, 0, 0, 0])
    expect(cumulativePtSeries([partial], DEFAULT_CUSTOM_RULE)).toEqual([])
  })
})
