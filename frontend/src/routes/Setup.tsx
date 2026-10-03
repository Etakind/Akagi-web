import { useEffect, useRef, useState } from 'react'
import { useNavigate, useSearchParams } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Toaster } from '@/components/ui/sonner'
import { InstallBlockingOverlay } from '@/components/InstallBlockingOverlay'
import { NativeApiFields } from '@/components/NativeApiFields'
import { invoke } from '@/lib/tauri'
import { checkApiBeforeSave } from '@/lib/nativeApi'
import { mergeExternal } from '@/lib/merge'
import { useTauriBridge } from '@/hooks/useTauriBridge'
import { useConfigStore } from '@/stores/configStore'
import { ManifestField } from '@/components/ManifestField'
import { GithubMark, DiscordMark } from '@/components/BrandMarks'
import { MjotLogo } from '@/components/MjotBrand'
import { NATIVE_3P, NATIVE_4P, isNativeBot } from '@/lib/nativeBots'
import { AKAGI_GITHUB_URL, AKAGI_DISCORD_URL, openExternal } from '@/lib/external'
import { platformInfo } from '@/lib/platforms'
import { LANG_LABELS, SUPPORTED_LANGS, type SupportedLang } from '@/i18n'
import type { AppConfig, BotInfo, BotSettings, DetectedBrowser } from '@/types'

type Step = 'welcome' | 'config' | 'configure' | 'finish'

// The 'bots' (install Mortal from GitHub) step is intentionally omitted: the
// built-in native bot is the zero-install default, so the wizard no longer
// installs an author bot. The 'configure' (bot settings) step is kept as the
// future home for built-in-bot settings. `BotsStep` is retained but unreachable.
const STEPS: Step[] = ['welcome', 'config', 'configure', 'finish']

// Author-provided MJAI bots installed by the first-run wizard. Same
// install path as the manual Bots → Install From GitHub flow, just
// pre-filled with author defaults.
const BOT_4P_NAME = 'mortal'
const BOT_3P_NAME = 'mortal3p'
// The built-in bots (shared names from `@/lib/nativeBots`) are always
// available (weights are embedded in the binary), so a native active bot is a
// working assistant — the wizard must not report "no bot / no analysis".

export function Setup() {
  const { t } = useTranslation()
  // The wizard renders standalone (no <App> parent), so we wire the
  // tauri event bridge + toast surface here ourselves. Without this the
  // CfT download progress notifications wouldn't show up during setup.
  useTauriBridge()
  const stored = useConfigStore((s) => s.config)
  const setStored = useConfigStore((s) => s.setConfig)
  // Seed the editable draft. On a genuine first run this also pre-selects the
  // Chromium capture backend (see `withFirstRunCaptureDefault`); a re-run keeps
  // the user's saved mode. Idempotent, so seeding here and in the effect below
  // (whichever path fires first depending on whether `stored` was ready at
  // mount) yields the same draft.
  const [draft, setDraft] = useState<AppConfig | null>(() =>
    stored ? stored : null,
  )
  const [step, setStep] = useState<Step>('welcome')
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState<string | null>(null)
  const [params] = useSearchParams()
  const navigate = useNavigate()
  // Bot settings drafts keyed by bot name. Populated lazily when the
  // configure step mounts (ConfigureBotsStep loads via get_bot_settings).
  // Lifted here so `next()` can flush them to disk before advancing.
  // MUST live above the early-return below — React forbids skipping a
  // hook on first render and then calling it on subsequent renders.
  const [botSettingsDraft, setBotSettingsDraft] = useState<Record<string, BotSettings>>({})
  // Stored-config snapshot the wizard draft was last synced against — the
  // merge base for folding external config changes into the open draft.
  const syncedStoredRef = useRef<AppConfig | null>(stored)

  // True when the user is re-running setup from Settings (not first run).
  const isRerun = params.get('rerun') === '1' || stored?.general.first_run_completed === true

  useEffect(() => {
    if (!stored) return
    const prev = syncedStoredRef.current
    syncedStoredRef.current = stored
    if (!prev) {
      // Seed the editable draft once the config loads (first run → Chromium
      // pre-selected; see the useState initializer above).
      setDraft(stored)
      return
    }
    // The stored config changed mid-wizard (e.g. the purchase store persisted
    // a delivered API key while the buyer was on another step). Three-way
    // merge instead of re-seeding: only fields the user hasn't modified
    // relative to the previous stored snapshot adopt the new stored value —
    // recursively, so nested sections like `bot.api` merge per field — and
    // every not-yet-saved wizard choice (platform, capture mode, …) survives.
    setDraft((cur) => (cur ? mergeExternal(cur, prev, stored) : stored))
  }, [stored])

  useEffect(() => {
    if (!stored) {
      invoke<AppConfig>('get_config').then(setStored).catch(() => {})
    }
  }, [stored, setStored])

  if (!draft) {
    return <div className="p-6 text-muted-foreground">{t('setup.loading')}</div>
  }

  const idx = STEPS.indexOf(step)
  const canBack = idx > 0
  const canNext = idx < STEPS.length - 1

  const saveBotSettings = async () => {
    for (const [name, settings] of Object.entries(botSettingsDraft)) {
      try {
        await invoke('update_bot_settings', { name, values: settings.values })
      } catch (e) {
        // Surface the first failure to the wizard's error strip; let
        // the user retry. Don't proceed to Finish with stale settings.
        throw new Error(`Save failed for ${name}: ${e}`, { cause: e })
      }
    }
  }

  const next = async () => {
    if (step === 'configure') {
      setBusy(true)
      setErr(null)
      try {
        // Same guard as the Bots page Save: if cloud inference is enabled,
        // the key must work before we let the wizard advance — otherwise Finish
        // would persist an enabled-but-broken API that silently falls back to
        // the local model. Checked here (not on Finish) so the error surfaces
        // right by the API fields; the user must fix the key or turn it off.
        const check = await checkApiBeforeSave(draft.bot.api)
        if (!check.ok) {
          setErr(
            check.kind === 'missing'
              ? `${t('bots.api.save_key_check_failed')} ${t('bots.api.need_url_key')}`
              : `${t('bots.api.save_key_check_failed')} ${check.message}`,
          )
          setBusy(false)
          return
        }
        await saveBotSettings()
      } catch (e) {
        setErr(String(e))
        setBusy(false)
        return
      }
      setBusy(false)
    }
    setStep(STEPS[idx + 1])
  }
  const back = () => setStep(STEPS[idx - 1])

  const finish = async () => {
    setBusy(true)
    setErr(null)
    try {
      // Re-query the bot list so the chosen active_4p / active_3p
      // reflect what's *actually* installed right now, regardless of
      // whether the user installed in this wizard run or already had
      // bots from a previous install.
      let installed: BotInfo[] = []
      try {
        installed = await invoke<BotInfo[]>('list_bots')
      } catch {
        /* ignore: bot dir may not be set up — we'll just leave the
           active_* fields at whatever the user had before. */
      }
      const has4p = installed.some((b) => b.name === BOT_4P_NAME)
      const has3p = installed.some((b) => b.name === BOT_3P_NAME)

      const final: AppConfig = {
        ...draft,
        general: { ...draft.general, first_run_completed: true },
        bot: {
          ...draft.bot,
          // Enable the assistant on setup completion: there's always a working
          // zero-install built-in bot (the BotConfig defaults select it), so
          // the user gets recommendations out of the box.
          //
          // Active-bot pick, in priority order: MJOT enabled → the built-in
          // bots (cloud inference only applies to them — selecting an author
          // bot here would finish the wizard with an enabled API nothing
          // uses); else author bots installed by a previous run → keep them;
          // else the defaults (built-in native bot) stand.
          enabled: true,
          active_4p: draft.bot.api.enabled ? NATIVE_4P : has4p ? BOT_4P_NAME : draft.bot.active_4p,
          active_3p: draft.bot.api.enabled ? NATIVE_3P : has3p ? BOT_3P_NAME : draft.bot.active_3p,
        },
      }
      await invoke('update_config', { newConfig: final })
      setStored(final)
      navigate('/', { replace: true })
    } catch (e) {
      setErr(String(e))
    } finally {
      setBusy(false)
    }
  }

  const cancel = () => navigate('/', { replace: true })

  return (
    <div className="min-h-screen w-full flex items-center justify-center p-6">
      <Toaster />
      <InstallBlockingOverlay />
      <Card className="w-full max-w-2xl">
        <CardHeader>
          <div className="flex items-center justify-between">
            <CardTitle>{t('setup.title')}</CardTitle>
            <span className="text-xs text-muted-foreground">
              {t('setup.step_progress', { current: idx + 1, total: STEPS.length })}
            </span>
          </div>
          <Stepper current={idx} />
        </CardHeader>
        <CardContent className="grid gap-6">
          {step === 'welcome' && <WelcomeStep />}
          {step === 'config' && <ChromiumConfigStep draft={draft} setDraft={setDraft} />}
          {step === 'configure' && (
            <ConfigureBotsStep
              draft={draft}
              setDraft={setDraft}
              drafts={botSettingsDraft}
              setDrafts={setBotSettingsDraft}
            />
          )}
          {step === 'finish' && <FinishStep draft={draft} />}

          {err && (
            <div className="rounded-md border border-red-500/40 bg-red-500/10 px-3 py-2 text-sm text-red-400">
              {err}
            </div>
          )}

          <div className="flex justify-between">
            <div className="flex gap-2">
              {canBack ? (
                <Button variant="outline" onClick={back} disabled={busy}>{t('common.back')}</Button>
              ) : (
                <span />
              )}
              {isRerun && (
                <Button variant="ghost" onClick={cancel} disabled={busy}>{t('common.cancel')}</Button>
              )}
            </div>
            {canNext ? (
              <Button onClick={next} disabled={busy}>{t('common.next')}</Button>
            ) : (
              <Button onClick={finish} disabled={busy}>
                {busy ? t('common.saving') : t('common.finish')}
              </Button>
            )}
          </div>
        </CardContent>
      </Card>
    </div>
  )
}

function Stepper({ current }: { current: number }) {
  return (
    <div className="flex gap-1.5 mt-3">
      {STEPS.map((_, i) => (
        <div
          key={i}
          className={`h-1 flex-1 rounded ${i <= current ? 'bg-primary' : 'bg-muted'}`}
        />
      ))}
    </div>
  )
}

function WelcomeStep() {
  const { t, i18n } = useTranslation()
  return (
    <div className="grid gap-3">
      <div className="grid gap-1.5">
        {/* Trilingual label so a user who can't read English/JP/CN still
            recognises this as a language picker before changing it. */}
        <Label className="text-xs">{t('setup.lang_label')}</Label>
        <Select
          value={i18n.language}
          onValueChange={(v) => void i18n.changeLanguage(v)}
        >
          <SelectTrigger className="w-full">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {SUPPORTED_LANGS.map((lang) => (
              <SelectItem key={lang} value={lang}>
                {LANG_LABELS[lang as SupportedLang]}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      <h2 className="text-lg font-semibold mt-2">{t('setup.welcome.title')}</h2>
      <p className="text-sm text-muted-foreground">
        {t('setup.welcome.body1')}
      </p>
      <p className="text-sm text-muted-foreground">
        {t('setup.welcome.body2')}
      </p>

      {/* Official project links — surfaced early so a fresh user can
          find the canonical repo / community before going any further.
          Intentionally low-key (no banner / colour) since these are
          informational, not warnings. */}
      <div className="flex flex-wrap items-center gap-2 mt-1">
        <span className="text-xs text-muted-foreground mr-1">
          {t('setup.welcome.links_label')}
        </span>
        <Button
          variant="outline"
          size="sm"
          onClick={() => openExternal(AKAGI_GITHUB_URL)}
        >
          <GithubMark className="h-4 w-4 mr-1.5" />
          GitHub
        </Button>
        <Button
          variant="outline"
          size="sm"
          onClick={() => openExternal(AKAGI_DISCORD_URL)}
        >
          <DiscordMark className="h-4 w-4 mr-1.5" />
          Discord
        </Button>
      </div>
    </div>
  )
}

function ChromiumConfigStep({
  draft,
  setDraft,
}: {
  draft: AppConfig
  setDraft: (c: AppConfig) => void
}) {
  const { t } = useTranslation()
  const chromium = draft.capture.chromium
  const setChromium = (patch: Partial<typeof chromium>) =>
    setDraft({
      ...draft,
      capture: { ...draft.capture, chromium: { ...chromium, ...patch } },
    })

  const [detected, setDetected] = useState<DetectedBrowser[] | null>(null)
  const [installed, setInstalled] = useState<string[] | null>(null)
  const [busy, setBusy] = useState<'idle' | 'detecting' | 'downloading'>('idle')

  const refresh = async () => {
    setBusy('detecting')
    try {
      const [d, i] = await Promise.all([
        invoke<DetectedBrowser[]>('detect_system_chrome'),
        invoke<string[]>('list_cft_installed'),
      ])
      setDetected(d)
      setInstalled(i)
    } catch {
      setDetected([])
      setInstalled([])
    } finally {
      setBusy('idle')
    }
  }

  useEffect(() => {
    // Mount-time load; refresh() sets state internally.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    refresh()
  }, [])

  const downloadCft = async () => {
    setBusy('downloading')
    try {
      await invoke<string>('download_chrome_for_testing', {
        channel: chromium.cft_channel || 'stable',
      })
      // Explicit download in the wizard = explicit opt-in to CfT.
      // Without this, a user who has both system Chrome and a freshly
      // downloaded CfT would still launch the system Chrome (system
      // takes priority unless force_cft is on), which is exactly what
      // the "Download" button was meant to override.
      setChromium({ force_cft: true })
      await refresh()
    } catch {
      /* surfaced via notify */
    } finally {
      setBusy('idle')
    }
  }

  const ready = (detected && detected.length > 0) || (installed && installed.length > 0) || chromium.executable

  return (
    <div className="grid gap-3">
      <h2 className="text-lg font-semibold">{t('setup.chromium.title')}</h2>
      {draft.capture.unavailable_reason && <p role="alert">{draft.capture.unavailable_reason}</p>}
      <Field label={t('settings.capture_enabled')}>
        <Select value={draft.capture.enabled ? 'on' : 'off'} onValueChange={(v) => setDraft({ ...draft, capture: { ...draft.capture, enabled: v === 'on', unavailable_reason: null } })}>
          <SelectTrigger><SelectValue /></SelectTrigger>
          <SelectContent><SelectItem value="on">{t('common.on')}</SelectItem><SelectItem value="off">{t('common.off')}</SelectItem></SelectContent>
        </Select>
      </Field>
      <Field label={t('settings.attach_port')} hint={t('settings.attach_port_hint')}>
        <Input type="number" min={0} max={65535} step={1} value={chromium.attach_port ?? 0}
          onChange={(e) => setChromium({ attach_port: Math.max(0, Math.min(65535, Math.trunc(Number(e.target.value) || 0))) })} />
      </Field>
      <Field label={t('settings.user_data_dir')} hint={t(chromium.attach_port ? 'settings.user_data_dir_attach_hint' : 'settings.user_data_dir_hint')}>
        <Input value={chromium.user_data_dir} onChange={(e) => setChromium({ user_data_dir: e.target.value })} placeholder={t('common.default')} />
      </Field>
      <Field label={t('settings.browser_executable')} hint={t('setup.chromium.exec_hint')}>
        <Input
          value={chromium.executable}
          onChange={(e) => setChromium({ executable: e.target.value })}
          placeholder={t('common.auto_detect')}
        />
      </Field>
      <div className="rounded-md border border-border/50 p-3 grid gap-2">
        <div className="flex items-center justify-between">
          <Label>{t('setup.chromium.detected_label')}</Label>
          <Button variant="outline" size="sm" onClick={refresh} disabled={busy !== 'idle'}>
            {busy === 'detecting' ? t('common.scanning') : t('common.refresh')}
          </Button>
        </div>
        {detected === null ? (
          <span className="text-xs text-muted-foreground">{t('common.scanning')}</span>
        ) : detected.length === 0 ? (
          <span className="text-xs text-muted-foreground">{t('setup.chromium.no_browser')}</span>
        ) : (
          <ul className="text-xs font-mono break-all">
            {detected.map((d) => (
              <li key={d.path}>· {d.path}</li>
            ))}
          </ul>
        )}
      </div>
      <div className="rounded-md border border-border/50 p-3 grid gap-2">
        <div className="flex items-center justify-between">
          <Label>{t('settings.cft_title')}</Label>
          <span className="text-xs text-muted-foreground">
            {installed === null
              ? t('settings.cft_status_loading')
              : installed.length === 0
                ? t('settings.cft_status_none')
                : t('settings.cft_status_count', { count: installed.length })}
          </span>
        </div>
        <Field label={t('settings.cft_channel')} hint={t('settings.cft_channel_hint')}>
          <Input
            value={chromium.cft_channel}
            onChange={(e) => setChromium({ cft_channel: e.target.value })}
            placeholder="stable"
          />
        </Field>
        <Button onClick={downloadCft} disabled={busy !== 'idle'} size="sm">
          {busy === 'downloading' ? t('common.downloading') : t('common.download')}
        </Button>
      </div>
      <Field
        label={t('settings.start_url')}
        hint={t('settings.start_url_hint', {
          platform: t(platformInfo().labelKey),
          url: platformInfo().defaultStartUrl,
        })}
      >
        <Input
          value={chromium.start_url}
          onChange={(e) => setChromium({ start_url: e.target.value })}
          placeholder={platformInfo().defaultStartUrl}
        />
      </Field>
      {!ready && (
        <div className="rounded-md border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-sm text-amber-200">
          {t('setup.chromium.warning_no_browser')}
        </div>
      )}
    </div>
  )
}

function ConfigureBotsStep({
  draft,
  setDraft,
  drafts,
  setDrafts,
}: {
  draft: AppConfig
  setDraft: (c: AppConfig) => void
  drafts: Record<string, BotSettings>
  setDrafts: React.Dispatch<React.SetStateAction<Record<string, BotSettings>>>
}) {
  const { t } = useTranslation()
  const [installed, setInstalled] = useState<BotInfo[] | null>(null)
  const [loadErrors, setLoadErrors] = useState<Record<string, string>>({})

  // Pull the bot list, then for each bot with a manifest fetch its
  // current values into the wizard draft. Skip bots that already have a
  // draft so back-and-forth navigation doesn't clobber unsaved edits.
  useEffect(() => {
    let cancelled = false
    ;(async () => {
      let list: BotInfo[]
      try {
        list = await invoke<BotInfo[]>('list_bots')
      } catch {
        if (!cancelled) setInstalled([])
        return
      }
      if (cancelled) return
      setInstalled(list)
      const targets = list.filter(
        (b) => (b.name === BOT_4P_NAME || b.name === BOT_3P_NAME) && b.manifest,
      )
      for (const b of targets) {
        if (drafts[b.name]) continue
        try {
          const s = await invoke<BotSettings>('get_bot_settings', { name: b.name })
          if (cancelled) return
          setDrafts((prev) => ({ ...prev, [b.name]: s }))
        } catch (e) {
          if (cancelled) return
          setLoadErrors((prev) => ({ ...prev, [b.name]: String(e) }))
        }
      }
    })()
    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const wizardBots = (installed ?? []).filter(
    (b) => b.name === BOT_4P_NAME || b.name === BOT_3P_NAME,
  )

  return (
    <div className="grid gap-4">
      <h2 className="text-lg font-semibold">{t('setup.configure.title')}</h2>

      {/* Built-in bot: optional MJOT cloud inference. Shown first — the
          built-in bot is the always-present default, so this is the primary
          thing to configure here even when no author bots are installed.
          The MJOT lockup is the heading (its wordmark reads "MJOT"); the
          tagline underneath says what it is. */}
      <div className="rounded-md border p-3 grid gap-3">
        <div className="grid gap-1">
          <MjotLogo className="h-7 w-auto justify-self-start" label="MJOT" />
          <span className="text-xs text-muted-foreground">{t('bots.api.mjot_tagline')}</span>
        </div>
        <NativeApiFields
          value={draft.bot.api}
          onChange={(api) => setDraft({ ...draft, bot: { ...draft.bot, api } })}
        />
      </div>

      {/* Author-bot (Mortal) manifest settings — only when such bots are
          installed from a previous run. */}
      {installed === null ? (
        <div className="text-sm text-muted-foreground">{t('setup.configure.loading')}</div>
      ) : wizardBots.length > 0 ? (
        <>
          <p className="text-sm text-muted-foreground">
            {t('setup.configure.normal_desc')}
          </p>

          {/* The Mortal weights bundled in the GitHub release are a
              placeholder forced by GitHub's file-size limit — they prove
              the install works but aren't strong enough for real play.
              Point users at Discord for the real weights. */}
          <div className="rounded-md border border-indigo-500/40 bg-indigo-500/10 p-3 grid gap-2">
            <div className="flex items-center gap-2 text-indigo-200 font-semibold text-sm">
              <DiscordMark className="h-4 w-4" />
              {t('setup.configure.models_title')}
            </div>
            <p className="text-sm text-indigo-100/90">
              {t('setup.configure.models_body')}
            </p>
            <div>
              <Button
                variant="outline"
                size="sm"
                onClick={() => openExternal(AKAGI_DISCORD_URL)}
              >
                <DiscordMark className="h-4 w-4 mr-1.5" />
                {t('setup.configure.models_btn')}
              </Button>
            </div>
          </div>
          {wizardBots.map((b) => (
            <BotSettingsForm
              key={b.name}
              name={b.name}
              loadError={loadErrors[b.name]}
              settings={drafts[b.name]}
              onChange={(values) =>
                setDrafts((prev) => {
                  const cur = prev[b.name]
                  if (!cur) return prev
                  return { ...prev, [b.name]: { ...cur, values } }
                })
              }
            />
          ))}
        </>
      ) : null}
    </div>
  )
}

function BotSettingsForm({
  name,
  settings,
  loadError,
  onChange,
}: {
  name: string
  settings: BotSettings | undefined
  loadError?: string
  onChange: (values: Record<string, unknown>) => void
}) {
  const { t } = useTranslation()
  const title = settings?.manifest.bot.display ?? name
  const description = settings?.manifest.bot.description

  if (loadError) {
    return (
      <div className="rounded-md border border-red-500/40 bg-red-500/10 p-3">
        <div className="font-medium">{title}</div>
        <div className="text-xs text-red-400 font-mono break-all mt-1">{loadError}</div>
      </div>
    )
  }
  if (!settings) {
    return (
      <div className="rounded-md border p-3 text-sm text-muted-foreground">
        {t('setup.configure.loading_bot', { name })}
      </div>
    )
  }

  const entries = Object.entries(settings.manifest.settings)
  return (
    <div className="rounded-md border p-3 grid gap-3">
      <div>
        <div className="font-medium">{title}</div>
        {description && <div className="text-xs text-muted-foreground">{description}</div>}
      </div>
      {entries.length === 0 ? (
        <div className="text-xs text-muted-foreground">{t('setup.configure.no_settings')}</div>
      ) : (
        <div className="grid gap-3">
          {entries.map(([key, spec]) => (
            <ManifestField
              key={key}
              fieldKey={key}
              spec={spec}
              value={settings.values[key] ?? spec.default}
              onChange={(v) => onChange({ ...settings.values, [key]: v })}
            />
          ))}
        </div>
      )}
    </div>
  )
}

function FinishStep({ draft }: { draft: AppConfig }) {
  const { t } = useTranslation()
  const m = draft.capture.mode
  const [installed, setInstalled] = useState<BotInfo[] | null>(null)
  useEffect(() => {
    invoke<BotInfo[]>('list_bots').then(setInstalled).catch(() => setInstalled([]))
  }, [])
  const has4p = installed?.some((b) => b.name === BOT_4P_NAME) ?? false
  const has3p = installed?.some((b) => b.name === BOT_3P_NAME) ?? false
  // Mirror what `finish()` actually persists: MJOT enabled → the built-in
  // bots; else prefer the author Mortal bot when installed; else keep the
  // configured active bot (which defaults to the built-in native bot). A
  // native active bot is a working assistant, so the summary must reflect it
  // instead of claiming analysis is unavailable.
  const apiOn = draft.bot.api.enabled
  const eff4p = apiOn ? NATIVE_4P : has4p ? BOT_4P_NAME : draft.bot.active_4p
  const eff3p = apiOn ? NATIVE_3P : has3p ? BOT_3P_NAME : draft.bot.active_3p
  const botLabel = (name: string) => (isNativeBot(name) ? t('bots.native_builtin') : name)
  const botParts = [
    eff4p ? `${botLabel(eff4p)} (4P)` : null,
    eff3p ? `${botLabel(eff3p)} (3P)` : null,
  ].filter((p): p is string => p !== null)
  const botSummary =
    botParts.length > 0 ? botParts.join(', ') : t('setup.finish.bots_none')
  return (
    <div className="grid gap-3">
      <h2 className="text-lg font-semibold">{t('setup.finish.title')}</h2>
      <div className="rounded-md border border-border/50 p-3 text-sm">
        <div><b>{t('setup.finish.platform_label')}</b> {t(platformInfo().labelKey)}</div>
        <div><b>{t('setup.finish.mode_label')}</b> {t('setup.finish.mode_chromium')}</div>
        {m === 'chromium' && (
          <>
            <div>
              <b>{t('setup.finish.exec_label')}</b>{' '}
              {draft.capture.chromium.executable
                ? draft.capture.chromium.executable
                : draft.capture.chromium.force_cft
                  ? t('setup.finish.exec_cft', { channel: draft.capture.chromium.cft_channel || 'stable' })
                  : t('setup.finish.exec_autodetect')}
            </div>
            <div><b>{t('setup.finish.start_url_label')}</b> {draft.capture.chromium.start_url}</div>
            <div><b>{t('setup.finish.cft_channel_label')}</b> {draft.capture.chromium.cft_channel}</div>
          </>
        )}
        <div><b>{t('setup.finish.bots_label')}</b> {installed === null ? t('setup.finish.bots_checking') : botSummary}</div>
      </div>
      <p className="text-sm text-muted-foreground">
        {t('setup.finish.click_finish_pre')}<b>{t('setup.finish.click_finish_btn')}</b>{t('setup.finish.click_finish_post')}
      </p>
    </div>
  )
}

function Field({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <div className="grid gap-1.5">
      <Label>{label}</Label>
      {children}
      {hint && <span className="text-xs text-muted-foreground">{hint}</span>}
    </div>
  )
}
