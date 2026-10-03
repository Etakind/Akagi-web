import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Toaster } from '@/components/ui/sonner'
import { invoke } from '@/lib/tauri'
import { useTauriBridge } from '@/hooks/useTauriBridge'
import { useConfigStore } from '@/stores/configStore'
import { platformInfo, PLATFORMS } from '@/lib/platforms'
import type { AppConfig, DetectedBrowser, PlatformKind } from '@/types'
export function Setup() {
  useTauriBridge()
  const { t } = useTranslation()
  const navigate = useNavigate()
  const setStored = useConfigStore(s => s.setConfig)
  const [draft, setDraft] = useState<AppConfig | null>(null)
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)
  useEffect(() => { invoke<AppConfig>('get_config').then(setDraft).catch(() => setError('Configuration unavailable')) }, [])
  const save = async () => {
    if (!draft) return
    setBusy(true)
    try {
      const next = { ...draft, general: { ...draft.general, first_run_completed: true } }
      await invoke('update_config', { newConfig: next })
      setStored(next)
      navigate('/')
    } catch { setError('Unable to save configuration') } finally { setBusy(false) }
  }
  return <div className="max-w-2xl mx-auto p-6 grid gap-4">
    <h1 className="text-xl font-bold">Akagi</h1>
    <p>{t('bots.local_only')}</p>
    {error && <p role="alert">{error}</p>}
    {draft && <>
      <Field label={t('settings.platform_card_title')}><Select value={draft.platform.kind} onValueChange={v => {
        const kind = v as PlatformKind
        setDraft({ ...draft, platform: { kind }, autoplay: { ...draft.autoplay, enabled: false }, capture: { ...draft.capture, chromium: { ...draft.capture.chromium, start_url: platformInfo(kind).defaultStartUrl } } })
      }}><SelectTrigger><SelectValue /></SelectTrigger><SelectContent>{PLATFORMS.map(p => <SelectItem key={p.kind} value={p.kind}>{t(p.labelKey)}</SelectItem>)}</SelectContent></Select></Field>
      <ChromiumConfigStep draft={draft} setDraft={setDraft} />
      <Button disabled={busy} onClick={save}>{t('common.save')}</Button>
    </>}
    <Toaster />
  </div>
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
          platform: t(platformInfo(draft.platform.kind).labelKey),
          url: platformInfo(draft.platform.kind).defaultStartUrl,
        })}
      >
        <Input
          value={chromium.start_url}
          onChange={(e) => setChromium({ start_url: e.target.value })}
          placeholder={platformInfo(draft.platform.kind).defaultStartUrl}
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

function Field({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <div className="grid gap-1.5">
      <Label>{label}</Label>
      {children}
      {hint && <span className="text-xs text-muted-foreground">{hint}</span>}
    </div>
  )
}
