// Mirrors backend schema. Source: frontend/README.md.

export type MjaiEvent =
  | { type: 'start_game'; names: string[]; kyoku_first?: number; aka_flag?: boolean; id?: number; num_players?: number }
  | { type: 'start_kyoku'; bakaze: string; dora_marker: string; kyoku: number; honba: number; kyotaku: number; oya: number; scores: number[]; tehais: string[][]; num_players?: number }
  | { type: 'tsumo'; actor: number; pai: string }
  | { type: 'dahai'; actor: number; pai: string; tsumogiri: boolean }
  | { type: 'chi'; actor: number; target: number; pai: string; consumed: [string, string] }
  | { type: 'pon'; actor: number; target: number; pai: string; consumed: [string, string] }
  | { type: 'daiminkan'; actor: number; target: number; pai: string; consumed: [string, string, string] }
  | { type: 'kakan'; actor: number; pai: string; consumed: [string, string, string] }
  | { type: 'ankan'; actor: number; consumed: [string, string, string, string] }
  | { type: 'dora'; dora_marker: string }
  | { type: 'reach'; actor: number; pai?: string }
  | { type: 'reach_accepted'; actor: number }
  | { type: 'hora'; actor: number; target: number; deltas?: number[]; ura_markers?: string[] }
  | { type: 'ryukyoku'; deltas?: number[] }
  | { type: 'kita'; actor: number; pai?: string }
  | { type: 'end_kyoku' }
  | { type: 'end_game' }
  | { type: 'none' }

export type BotResponse = MjaiEvent & { meta?: Record<string, unknown> }

/** Bot-driven custom display payload, attached on `meta.show`.
 *  Schema is intentionally generic so a single tile can render top-N
 *  actions, opponent reads, yaku breakdowns, etc. — bots decide the
 *  semantics and formatting. */
export type ShowItem = {
  /** Primary text on the row. */
  label?: string
  /** mjai tile strings; converted to mahgen via `mjaiToMahgen`. */
  pais?: string[]
  /** Raw mahgen DSL string. Wins over `pais` if both are set. */
  tiles?: string
  /** Right-side text (any format — e.g. "85.42%", "+12000"). */
  value?: string
  /** Hex accent color (e.g. "#00ff80") — applied as left bar + faint row tint. */
  color?: string
  /** Small subtitle under `label`. */
  note?: string
}

export type ShowMeta = {
  /** Optional title; falls back to the tile's default title. */
  title?: string
  items: ShowItem[]
}

export type BotStatus =
  | { state: 'idle' }
  | { state: 'loading'; bot: string; stage: 'spawning' }
  | { state: 'ready'; bot: string; actor_id: number }
  | { state: 'error'; bot: string; error: string }
  | { state: 'stopped'; bot: string }

export type CaptureKind = 'chromium'

export type CaptureStatus =
  | { state: 'stopped' }
  | { state: 'starting'; kind: CaptureKind; descriptor: string }
  | { state: 'running'; kind: CaptureKind; descriptor: string }
  | { state: 'error'; kind: CaptureKind; descriptor?: string; error: string }

export type Notification = {
  level: 'info' | 'success' | 'warn' | 'error'
  title: string
  body?: string
  sticky: boolean
  id?: string
}

export type CaptureMode = 'chromium'

export type ChromiumConfig = {
  attach_port?: number
  executable: string
  user_data_dir: string
  start_url: string
  cft_channel: string
  force_cft: boolean
  extra_args: string[]
}

export type CaptureConfig = {
  mode: CaptureMode
  enabled: boolean
  unavailable_reason?: string | null
  http: { record_all: boolean; bodies: boolean; max_body_bytes: number; static_assets: boolean }
  chromium: ChromiumConfig
}

export type DetectedBrowser = {
  kind: 'chrome' | 'edge' | 'brave' | 'chromium' | 'chrome_for_testing'
  path: string
}

/// Bridge selector (the runtime kind, not the history-record tag — those
/// share names but the schema enum carries extra archive-only variants).
/// Mirrors `src/config/platform.rs::Platform` (`#[derive(Serialize)]` →
/// PascalCase JSON: `"Majsoul"`, `"Tenhou"`).
export type PlatformKind = 'Majsoul' | 'Tenhou'

export type MajsoulAutoplayConfig = {
  pre_click_delay_min_ms: number
  pre_click_delay_max_ms: number
  inter_click_delay_ms: number
  hover_delay_ms: number
  click_hold_ms: number
  /** Wait this long for the client's own input command after a click
   *  before pressing again; 0 disables verification. */
  verify_input_ms: number
  /** Retries when no input command follows a click sequence; 0 = log only. */
  click_retries: number
  /** Reload the game page after this many dead decisions in a row; 0 = off. */
  reload_after_failures: number
  dealer_first_discard_extra_delay_ms: number
}

/** Pre-click delay model parameters. Mirrors
 *  `src/config/autoplay.rs::DelayModelConfig`; the fine-grained knobs are
 *  config-file-only — the Settings UI exposes the Lua script fields. */
export type DelayMode = 'legacy' | 'lua'

export type DelayModelConfig = {
  /** Which policy is active; exactly one. `legacy` = the old fixed
   *  uniform model, `lua` = the scriptable human-like model backed by
   *  `delay.lua` next to the config file (auto-generated). */
  mode: DelayMode
  /** UI-readiness floor: minimum total thinking time per decision, ms.
   *  Clicks issued before Mahjong Soul renders the UI are lost. */
  min_delay_ms: number
  /** Higher floor for decisions that click an action button (chi/pon/
   *  kan/ron/skip/riichi) — buttons render after the discard animation
   *  plus their own pop-in, later than hand tiles. */
  min_button_delay_ms: number
  distribution: 'uniform' | 'log_normal'
  /** Per-decision-kind log-normal `[mu, sigma]` in ln(seconds); keys like
   *  `dahai_tedashi`, `claim`. Calibrated from ranked-game records. */
  lognormal: Record<string, [number, number]>
  bank_on_long_thought: boolean
  riichi_extra_ms: number
  kan_extra_ms: number
  safety_margin_ms: number
  bank_use_fraction: number
  bank_max_single_ms: number
  no_budget_cap_ms: number
}

export type AutoplayConfig = {
  enabled: boolean
  majsoul: MajsoulAutoplayConfig
  delay: DelayModelConfig
}

/** The always-on-top suggestion overlay. Mirrors `crate::config::OverlayConfig`. */
export type OverlayConfig = {
  enabled: boolean
  top_n: number
  opacity: number
  always_on_top: boolean
}

/** Bounds enforced by `crate::config::overlay` — mirrored so the UI can't
 *  offer a value the backend would silently clamp. */
export const OVERLAY_TOP_N_MIN = 1
export const OVERLAY_TOP_N_MAX = 5
export const OVERLAY_OPACITY_MIN = 0.3
export const OVERLAY_OPACITY_MAX = 1.0

export type AppConfig = {
  general: { first_run_completed: boolean }
  logging: { dir: string; level: string; all_level: string }
  platform: { kind: PlatformKind }
  bot: {
    enabled: boolean
    active_4p: string
    active_3p: string
    migration_notice?: string | null
  }
  capture: CaptureConfig
  autoplay: AutoplayConfig
  overlay: OverlayConfig
}

export type BotInfo = { name: string }

export type Snapshot = {
  config: AppConfig
  bot_status: BotStatus
  capture_status: CaptureStatus
  log_dir: string
}

export type WaitInfo = { tile: string; left: number; agari_rate: number | null }

export type ImproveEntry = { draw: string; widened_waits: WaitInfo[]; widened_total: number }

export type Hand13Result = {
  shanten: number
  waits: WaitInfo[]
  waits_total: number
  next_shanten_waits_count: { [tileIdx: number]: number }
  avg_next_shanten_waits: number
  mixed_waits_score: number
  avg_agari_rate: number
  is_furiten: boolean
  furiten_rate: number
  improves: ImproveEntry[]
  improve_way_count: number
  avg_improve_waits_count: number
  dama_point: number
  riichi_point: number
  mixed_round_point: number
  yaku_ids: number[]
}

export type DiscardCandidate = { discard: string; result: Hand13Result }

export type Hand14Result = {
  shanten: number
  maintain: DiscardCandidate[]
  backwards: DiscardCandidate[]
}

export type OpponentRisk = {
  seat: number
  tenpai_rate: number
  risk: number[]
  is_riichi: boolean
}

export type AnalysisResult = {
  seat: number
  turn: number
  shanten: number
  state: 'wait13' | 'discard14'
  hand13: Hand13Result | null
  hand14: Hand14Result | null
  opponents: OpponentRisk[]
  mixed_risk: number[]
  best_attack_discard: string | null
  best_defence_discard: string | null
}

export type DiscardEntry = {
  tile: string
  tedashi: boolean
  is_riichi: boolean
  /** Claimed by another player (pon/chi/kan); kept for analysis, hidden in the
   * rendered river. */
  called?: boolean
}

export type MeldSnapshot = {
  kind: 'chi' | 'pon' | 'daiminkan' | 'ankan' | 'kakan'
  tiles: string[]
  from_who: number
  called_tile: string | null
}

export type PlayerSnapshot = {
  seat: number
  tehai: string[]
  melds: MeldSnapshot[]
  river: DiscardEntry[]
  score: number
  riichi_declared: boolean
  riichi_stage: boolean
  double_riichi: boolean
  riichi_declaration_index: number | null
  /** 3p only: north tiles set aside via kita / nukidora. Empty in 4p. */
  kita_tiles: string[]
}

export type GameStateSnapshot = {
  bakaze: 'E' | 'S' | 'W' | 'N'
  kyoku: number
  honba: number
  kyotaku: number
  oya: number
  current_player: number
  turn_count: number
  phase: 'wait_act' | 'wait_response'
  is_done: boolean
  /** 3 (sanma) or 4 (yonma). */
  num_players: number
  /** Length matches num_players. */
  players: PlayerSnapshot[]
  dora_markers: string[]
  our_seat: number | null
}

export type PlayerMahgenView = {
  seat: number
  hand: string
  melds: string[]
  river: string
}

export type MahgenView = {
  /** Length matches num_players. */
  players: PlayerMahgenView[]
  /** 3 (sanma) or 4 (yonma). */
  num_players: number
  dora_indicators: string
}

/** Mirrors `crate::schema::HoraScoreInfo`. Returned by `compute_bot_hora_score`. */
export type HoraScoreInfo = {
  points: number
  han: number
  fu: number
  yakuman: boolean
  /** mjai tile string of the winning tile. */
  win_tile: string
}

// ---------- Game History ----------
//
// Mirrors `crate::schema::history::*`. Strings carry RFC3339 timestamps;
// the frontend parses them with `new Date(...)` on demand.

export type Platform =
  | 'majsoul'
  | 'tenhou'
  | 'riichi_city'
  | 'mjai'
  | 'unknown'

export type KyokuMode = 'east_only' | 'east_south' | 'other'

/** Per-game stat counters from the recorded player's perspective. */
export type GameStats = {
  round: number
  oya: number

  fuuro: number
  fuuro_num: number
  fuuro_point: number
  fuuro_agari: number
  fuuro_agari_jun: number
  fuuro_agari_point: number
  fuuro_houjuu: number

  agari: number
  agari_as_oya: number
  agari_jun: number
  agari_point_oya: number
  agari_point_ko: number

  houjuu: number
  houjuu_jun: number
  houjuu_to_oya: number
  houjuu_point_to_oya: number
  houjuu_point_to_ko: number

  riichi: number
  riichi_as_oya: number
  riichi_jun: number
  riichi_agari: number
  riichi_agari_point: number
  riichi_agari_jun: number
  riichi_houjuu: number
  riichi_ryukyoku: number
  riichi_point: number
  chasing_riichi: number
  riichi_got_chased: number

  dama_agari: number
  dama_agari_jun: number
  dama_agari_point: number

  ryukyoku: number
  ryukyoku_point: number

  yakuman: number
  nagashi_mangan: number
}

/**
 * Platform-specific match identity captured at `start_game` — which room /
 * rank lobby the game was played in plus the platform's own game (paifu) id.
 * Mirrors `crate::schema::history::MatchInfo`: internally tagged on
 * `platform`, raw platform values, `None` fields omitted from the JSON.
 */
export type MatchInfo =
  | {
      platform: 'majsoul'
      /** Raw `game_uuid` — the replay identifier. */
      game_uuid?: string | null
      /** Ranked matchmode id (1..=28 = Bronze..Throne / Melee, 4p+3p). */
      mode_id?: number | null
      /** Friendly/AI room number. */
      room_id?: number | null
      /** Tournament id. */
      contest_uid?: number | null
    }
  | {
      platform: 'tenhou'
      /** Paifu id from `<TAIKYOKU log=…>`. */
      log_id?: string | null
      /** Raw `<GO type=…>` rule/room bitfield (tier in bits 0x20/0x80). */
      go_type?: number | null
      /** Lobby number; 0 = the public ranked lobby. */
      lobby?: number | null
    }
  | {
      platform: 'riichi_city'
      /** Table-instance token from the `cmd_enter_room` wrapper. */
      room_id?: string | null
      /** Matchmaking classification id (wire string). */
      classify_id?: string | null
      /** Rank stage tier of the matchmaking room. */
      stage_type?: number | null
      /** Game mode id (e.g. 1001). */
      game_play?: number | null
    }

export type GameRecord = {
  id: string
  /** RFC3339 timestamp. */
  started_at: string
  /** RFC3339 timestamp. */
  ended_at: string
  platform: Platform
  num_players: 3 | 4
  kyoku_mode: KyokuMode
  names: string[]
  our_seat: number | null
  final_scores: number[]
  final_ranks: number[]
  our_rank: number | null
  /** `final_score - starting_score` (4p:25000, 3p:35000). */
  our_delta: number | null
  stats: GameStats
  /** Absent/null on records from before this field existed. */
  match_info?: MatchInfo | null
  log_path: string
}

export type HistoryFilter = {
  platform?: Platform
  num_players?: 3 | 4
  kyoku_mode?: KyokuMode
  /** RFC3339 timestamp; inclusive. */
  started_after?: string
  /** RFC3339 timestamp; exclusive. */
  started_before?: string
}

export type HistoryEvent =
  | { kind: 'recorded'; record: GameRecord }
  | { kind: 'deleted'; id: string }

// ---------- Logs ----------
//
// Mirrors `crate::schema::ipc::{LogEntry, LogSessionInfo, ReadLogRequest,
// ReadLogResponse}`. The same shape is used both for entries read off
// disk (`read_log_session`) and for live-tailed entries delivered over a
// `tauri::ipc::Channel` (`subscribe_log_events`) — initial-load and live
// arrivals merge into the same UI list without translation.

export type LogLevel = 'TRACE' | 'DEBUG' | 'INFO' | 'WARN' | 'ERROR'

export type LogEntry = {
  ts_ms: number
  /** One of `LogLevel`, but kept open as `string` because backend may add levels. */
  level: string
  target: string
  file?: string
  line?: number
  message: string
  fields?: Record<string, unknown>
}

export type LogSessionInfo = {
  name: string
  path: string
  size_bytes: number
  mtime_ms: number
  is_active: boolean
}

export type ReadLogRequest = {
  session: string
  offset?: number
  limit?: number
  levels?: string[]
  /** Target prefixes; any-match (OR). */
  targets?: string[]
  /** Case-insensitive substring on `message`. */
  search?: string
}

export type ReadLogResponse = {
  entries: LogEntry[]
  has_more: boolean
  skipped_malformed: number
}

// ---------- Inspector ----------
//
// Mirrors `crate::schema::inspector::*` and `crate::schema::ipc::ReadInspector*`.
// Tagged on `kind` — switch on the discriminant to render kind-specific
// detail panels. Same shape arrives via `subscribe_inspector` (live tail)
// and `read_inspector` (past sessions), so renderers don't fork.

export type FrameDirection = 'up' | 'down'

export type FrameRaw =
  | { format: 'text'; data: string }
  | { format: 'binary'; data: string } // legacy base64
  | { format: 'redacted'; data: string }

export type ParsedFrame = {
  method: string
  args: unknown
}

export type BotReactionPayload = {
  bot: string
  actor_id: number
  trigger: MjaiEvent
  action: MjaiEvent
  meta?: Record<string, unknown>
  reaction_ms: number
}

/** Which capture backend observed an event. */
export type CaptureSource = 'chromium'

export type HttpPhase = 'request' | 'response'

export type HttpHeader = {
  name: string
  value: string
}

/** A body we kept, or the reason we did not. */
export type HttpBody = {
  text?: string
  bytes?: number
  /** Absent when the body was captured whole. */
  skipped?: string
}

/**
 * A recognizer's reading of an exchange. Vendor-specific vocabulary lives
 * in `data` — never in the exchange itself — so a new recognizer needs no
 * change here.
 */
export type HttpAnnotation = {
  kind: string
  summary: string
  data: unknown
}

export type InspectorEntry =
  | {
      kind: 'ws_frame'
      ts_ms: number
      direction: FrameDirection
      flow_id: string
      size: number
      raw: FrameRaw
      parsed?: ParsedFrame
      emitted: number
    }
  | {
      kind: 'mjai_event'
      ts_ms: number
      event: MjaiEvent
    }
  | {
      kind: 'bot_reaction'
      ts_ms: number
      // Backend serializes BotReaction with #[serde(flatten)], so its
      // fields land at the top level of the row alongside `kind` and
      // `ts_ms`.
      bot: string
      actor_id: number
      trigger: MjaiEvent
      action: MjaiEvent
      meta?: Record<string, unknown>
      reaction_ms: number
    }
  | {
      kind: 'http'
      ts_ms: number
      source: CaptureSource
      // Backend serializes HttpExchange with #[serde(flatten)], so its
      // fields land at the top level of the row, same as bot_reaction.
      exchange_id?: string
      phase: HttpPhase
      method: string
      url: string
      host: string
      version: string
      status?: number
      headers: HttpHeader[]
      body?: HttpBody
      annotations?: HttpAnnotation[]
    }

export type InspectorKind = InspectorEntry['kind']

export type ReadInspectorRequest = {
  session: string
  offset?: number
  limit?: number
  kinds?: InspectorKind[]
  actor?: number
  search?: string
}

export type ReadInspectorResponse = {
  entries: InspectorEntry[]
  has_more: boolean
  skipped_malformed: number
}
