// Static lookup tables for the Majsoul / Tenhou PT formulas, transcribed
// from the user-provided spec.
//
// Layout choices:
// - Majsoul rank/lobby tables are split by player count. The 3p schedule
//   has different bonus rows than the 4p one (see the user's tables).
// - Tenhou tables embed every cell directly — the displayed PT is the
//   table value, no further uma/dan-bonus split. We carry both 4p and 3p
//   versions as parallel arrays.
// - "+0" entries are stored as `0`. Cells the spec marks "-" (e.g.
//   天鳳位 has no progression target, "新人" / 級 ranks below 1級 don't
//   penalise) are stored as `0` too — they have no semantic effect and
//   the calculator simply returns 0 for those slots.

// ---------- Majsoul: rank IDs ----------

export const MAJSOUL_DAN_4P = [
  'shoshin_1',
  'shoshin_2',
  'shoshin_3',
  'jakushi_1',
  'jakushi_2',
  'jakushi_3',
  'jakketsu_1',
  'jakketsu_2',
  'jakketsu_3',
  'jakugou_1',
  'jakugou_2',
  'jakugou_3',
  'jakusei_1',
  'jakusei_2',
  'jakusei_3',
  'konten',
] as const
export type MajsoulDan = (typeof MAJSOUL_DAN_4P)[number]

export const MAJSOUL_LOBBY = ['bronze', 'silver', 'gold', 'jade', 'throne'] as const
export type MajsoulLobby = (typeof MAJSOUL_LOBBY)[number]

// ---------- Majsoul 4p: lobby-level 1st/2nd bonus ----------
//
// `[lobby][rank-1][mode]` where mode = 0 (east-only / tonpuu) or
// 1 (east-south / hanchan). Only ranks 1 and 2 are populated; rank 3
// is always 0 and rank 4 uses the dan table below.

type MajsoulLobbyTable = Record<MajsoulLobby, [number, number][]>

export const MAJSOUL_LOBBY_4P: MajsoulLobbyTable = {
  bronze: [
    [10, 20],
    [5, 10],
  ],
  silver: [
    [20, 40],
    [10, 20],
  ],
  gold: [
    [40, 80],
    [20, 40],
  ],
  jade: [
    [55, 110],
    [30, 55],
  ],
  throne: [
    [60, 120],
    [30, 60],
  ],
}

// ---------- Majsoul 4p: dan-level last-place penalty ----------
//
// `[dan][mode]` where mode = 0 (tonpuu) or 1 (hanchan). Stored as
// negative numbers (already the deduction).

type MajsoulDanTable = Record<MajsoulDan, [number, number]>

export const MAJSOUL_DAN_PENALTY_4P: MajsoulDanTable = {
  shoshin_1: [0, 0],
  shoshin_2: [0, 0],
  shoshin_3: [0, 0],
  jakushi_1: [-10, -20],
  jakushi_2: [-20, -40],
  jakushi_3: [-30, -60],
  jakketsu_1: [-40, -80],
  jakketsu_2: [-50, -100],
  jakketsu_3: [-60, -120],
  jakugou_1: [-80, -165],
  jakugou_2: [-90, -180],
  jakugou_3: [-100, -195],
  jakusei_1: [-110, -210],
  jakusei_2: [-120, -225],
  jakusei_3: [-130, -240],
  konten: [-130, -240], // 魂天 — no canonical penalty in the table; reuse last tier.
}

// ---------- Majsoul 3p: lobby-level 1st bonus ----------
//
// 3p bonus tables are smaller — only 1st place earns lobby bonus, 2nd
// is always 0, last (rank 3) uses the dan table.

export const MAJSOUL_LOBBY_3P: Record<MajsoulLobby, [number, number]> = {
  bronze: [15, 30],
  silver: [30, 60],
  gold: [55, 105],
  jade: [75, 160],
  throne: [120, 240],
}

export const MAJSOUL_DAN_PENALTY_3P: MajsoulDanTable = {
  shoshin_1: [0, 0],
  shoshin_2: [0, 0],
  shoshin_3: [0, 0],
  jakushi_1: [-10, -20],
  jakushi_2: [-20, -40],
  jakushi_3: [-30, -60],
  jakketsu_1: [-40, -80],
  jakketsu_2: [-50, -100],
  jakketsu_3: [-60, -120],
  jakugou_1: [-80, -165],
  jakugou_2: [-95, -190],
  jakugou_3: [-110, -215],
  jakusei_1: [-125, -240],
  jakusei_2: [-140, -265],
  jakusei_3: [-160, -290],
  konten: [-160, -290],
}

/** Localisable display label for a Majsoul dan id. */
export const MAJSOUL_DAN_LABEL: Record<MajsoulDan, string> = {
  shoshin_1: '初心1星',
  shoshin_2: '初心2星',
  shoshin_3: '初心3星',
  jakushi_1: '雀士1星',
  jakushi_2: '雀士2星',
  jakushi_3: '雀士3星',
  jakketsu_1: '雀傑1星',
  jakketsu_2: '雀傑2星',
  jakketsu_3: '雀傑3星',
  jakugou_1: '雀豪1星',
  jakugou_2: '雀豪2星',
  jakugou_3: '雀豪3星',
  jakusei_1: '雀聖1星',
  jakusei_2: '雀聖2星',
  jakusei_3: '雀聖3星',
  konten: '魂天',
}

export const MAJSOUL_LOBBY_LABEL: Record<MajsoulLobby, string> = {
  bronze: '銅之間',
  silver: '銀之間',
  gold: '金之間',
  jade: '玉之間',
  throne: '王座之間',
}

// ---------- Tenhou: rank IDs ----------

export const TENHOU_DAN_4P = [
  'newcomer',
  'kyu_9',
  'kyu_8',
  'kyu_7',
  'kyu_6',
  'kyu_5',
  'kyu_4',
  'kyu_3',
  'kyu_2',
  'kyu_1',
  'dan_1',
  'dan_2',
  'dan_3',
  'dan_4',
  'dan_5',
  'dan_6',
  'dan_7',
  'dan_8',
  'dan_9',
  'dan_10',
  'tenhoui',
] as const
export type TenhouDan = (typeof TENHOU_DAN_4P)[number]

export const TENHOU_DAN_LABEL: Record<TenhouDan, string> = {
  newcomer: '新人',
  kyu_9: '9級',
  kyu_8: '8級',
  kyu_7: '7級',
  kyu_6: '6級',
  kyu_5: '5級',
  kyu_4: '4級',
  kyu_3: '3級',
  kyu_2: '2級',
  kyu_1: '1級',
  dan_1: '初段',
  dan_2: '二段',
  dan_3: '三段',
  dan_4: '四段',
  dan_5: '五段',
  dan_6: '六段',
  dan_7: '七段',
  dan_8: '八段',
  dan_9: '九段',
  dan_10: '十段',
  tenhoui: '天鳳位',
}

// ---------- Tenhou 4p: full PT-by-rank table ----------
//
// `[dan][mode][rank-1]` → integer PT. mode 0 = tonpuu, mode 1 = hanchan.
// Rank vectors have length 4. Cells where the spec gives "+0" are 0.

type TenhouTable = Record<TenhouDan, [number[], number[]]>

export const TENHOU_4P: TenhouTable = {
  newcomer: [
    [20, 10, 0, 0],
    [30, 15, 0, 0],
  ],
  kyu_9: [
    [20, 10, 0, 0],
    [30, 15, 0, 0],
  ],
  kyu_8: [
    [20, 10, 0, 0],
    [30, 15, 0, 0],
  ],
  kyu_7: [
    [20, 10, 0, 0],
    [30, 15, 0, 0],
  ],
  kyu_6: [
    [20, 10, 0, 0],
    [30, 15, 0, 0],
  ],
  kyu_5: [
    [20, 10, 0, 0],
    [30, 15, 0, 0],
  ],
  kyu_4: [
    [20, 10, 0, 0],
    [30, 15, 0, 0],
  ],
  kyu_3: [
    [20, 10, 0, 0],
    [30, 15, 0, 0],
  ],
  kyu_2: [
    [20, 10, 0, -10],
    [30, 15, 0, -15],
  ],
  kyu_1: [
    [20, 10, 0, -20],
    [30, 15, 0, -30],
  ],
  dan_1: [
    [40, 10, 0, -30],
    [60, 15, 0, -45],
  ],
  dan_2: [
    [40, 10, 0, -40],
    [60, 15, 0, -60],
  ],
  dan_3: [
    [40, 10, 0, -50],
    [60, 15, 0, -75],
  ],
  dan_4: [
    [50, 20, 0, -60],
    [75, 30, 0, -90],
  ],
  dan_5: [
    [50, 20, 0, -70],
    [75, 30, 0, -105],
  ],
  dan_6: [
    [50, 20, 0, -80],
    [75, 30, 0, -120],
  ],
  dan_7: [
    [50, 20, 0, -90],
    [75, 30, 0, -135],
  ],
  dan_8: [
    [60, 30, 0, -100],
    [90, 45, 0, -150],
  ],
  dan_9: [
    [60, 30, 0, -110],
    [90, 45, 0, -165],
  ],
  dan_10: [
    [60, 30, 0, -120],
    [90, 45, 0, -180],
  ],
  tenhoui: [
    [60, 30, 0, -120],
    [90, 45, 0, -180],
  ],
}

// ---------- Tenhou 3p: full PT-by-rank table ----------
//
// `[dan][mode][rank-1]` → integer PT. Rank vectors have length 3.

export const TENHOU_DAN_3P = TENHOU_DAN_4P
export type TenhouDan3p = TenhouDan

export const TENHOU_3P: Record<TenhouDan, [number[], number[]]> = {
  newcomer: [
    [30, 0, 0],
    [45, 0, 0],
  ],
  kyu_9: [
    [30, 0, 0],
    [45, 0, 0],
  ],
  kyu_8: [
    [30, 0, 0],
    [45, 0, 0],
  ],
  kyu_7: [
    [30, 0, 0],
    [45, 0, 0],
  ],
  kyu_6: [
    [30, 0, 0],
    [45, 0, 0],
  ],
  kyu_5: [
    [30, 0, 0],
    [45, 0, 0],
  ],
  kyu_4: [
    [30, 0, 0],
    [45, 0, 0],
  ],
  kyu_3: [
    [30, 0, 0],
    [45, 0, 0],
  ],
  kyu_2: [
    [30, 0, -10],
    [45, 0, -15],
  ],
  kyu_1: [
    [30, 0, -20],
    [45, 0, -30],
  ],
  dan_1: [
    [50, 0, -30],
    [75, 0, -45],
  ],
  dan_2: [
    [50, 0, -40],
    [75, 0, -60],
  ],
  dan_3: [
    [50, 0, -50],
    [75, 0, -75],
  ],
  dan_4: [
    [70, 0, -60],
    [105, 0, -90],
  ],
  dan_5: [
    [70, 0, -70],
    [105, 0, -105],
  ],
  dan_6: [
    [70, 0, -80],
    [105, 0, -120],
  ],
  dan_7: [
    [90, 0, -90],
    [135, 0, -135],
  ],
  dan_8: [
    [90, 0, -100],
    [135, 0, -150],
  ],
  dan_9: [
    [90, 0, -110],
    [135, 0, -165],
  ],
  dan_10: [
    [90, 0, -120],
    [135, 0, -180],
  ],
  tenhoui: [
    [90, 0, -120],
    [135, 0, -180],
  ],
}
