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
