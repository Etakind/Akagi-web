import { useTranslation } from 'react-i18next'
import { Card, CardHeader, CardTitle, CardContent } from '@/components/ui/card'
import { useConfigStore } from '@/stores/configStore'
export function Bots() {
  const { t } = useTranslation()
  const config = useConfigStore(s => s.config)
  return <div className="p-6"><Card><CardHeader><CardTitle>{t('nav.bots')}</CardTitle></CardHeader><CardContent className="grid gap-3">
    <p>{t('bots.local_only')}</p><p>Akagi · 4p / Akagi · 3p</p>
    {config?.bot.migration_notice && <p role="alert">{config.bot.migration_notice}</p>}
  </CardContent></Card></div>
}
