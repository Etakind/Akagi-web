// PT-rule selector: tabs for Majsoul / Tenhou / Custom, with rule-
// specific sub-controls. Stateless wrt the rule itself — reads/writes
// via `useHistoryStore`.

import { useTranslation } from 'react-i18next'

import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import {
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
} from '@/components/ui/tabs'
import { DEFAULT_CUSTOM_RULE } from '@/lib/ptCalc'
import {
  MAJSOUL_DAN_4P,
  MAJSOUL_DAN_LABEL,
  MAJSOUL_LOBBY,
  MAJSOUL_LOBBY_LABEL,
  type MajsoulDan,
  type MajsoulLobby,
  TENHOU_DAN_4P,
  TENHOU_DAN_LABEL,
  type TenhouDan,
} from '@/lib/ptTables'
import { useHistoryStore } from '@/stores/historyStore'

export function PtRuleSelector() {
  const rule = useHistoryStore((s) => s.rule)
  const setRule = useHistoryStore((s) => s.setRule)
  const { t } = useTranslation()

  const onTabChange = (value: string) => {
    if (value === 'majsoul') {
      setRule({ kind: 'majsoul', lobby: 'jade', dan: 'jakketsu_3' })
    } else if (value === 'tenhou') {
      setRule({ kind: 'tenhou', dan: 'dan_4' })
    } else if (value === 'custom') {
      setRule(DEFAULT_CUSTOM_RULE)
    }
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-sm uppercase tracking-wider">
          {t('history.rule.title')}
        </CardTitle>
      </CardHeader>
      <CardContent>
        <Tabs value={rule.kind} onValueChange={onTabChange}>
          <TabsList>
            <TabsTrigger value="majsoul">{t('history.rule.majsoul')}</TabsTrigger>
            <TabsTrigger value="tenhou">{t('history.rule.tenhou')}</TabsTrigger>
            <TabsTrigger value="custom">{t('history.rule.custom')}</TabsTrigger>
          </TabsList>

          <TabsContent value="majsoul" className="pt-3">
            {rule.kind === 'majsoul' && (
              <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
                <Field label={t('history.rule.lobby')}>
                  <Select
                    value={rule.lobby}
                    onValueChange={(v) =>
                      setRule({ ...rule, lobby: v as MajsoulLobby })
                    }
                  >
                    <SelectTrigger className="w-full">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {MAJSOUL_LOBBY.map((id) => (
                        <SelectItem key={id} value={id}>
                          {MAJSOUL_LOBBY_LABEL[id]}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </Field>
                <Field label={t('history.rule.dan')}>
                  <Select
                    value={rule.dan}
                    onValueChange={(v) =>
                      setRule({ ...rule, dan: v as MajsoulDan })
                    }
                  >
                    <SelectTrigger className="w-full">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {MAJSOUL_DAN_4P.map((id) => (
                        <SelectItem key={id} value={id}>
                          {MAJSOUL_DAN_LABEL[id]}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </Field>
              </div>
            )}
          </TabsContent>

          <TabsContent value="tenhou" className="pt-3">
            {rule.kind === 'tenhou' && (
              <Field label={t('history.rule.dan')}>
                <Select
                  value={rule.dan}
                  onValueChange={(v) =>
                    setRule({ kind: 'tenhou', dan: v as TenhouDan })
                  }
                >
                  <SelectTrigger className="w-full">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {TENHOU_DAN_4P.map((id) => (
                      <SelectItem key={id} value={id}>
                        {TENHOU_DAN_LABEL[id]}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </Field>
            )}
          </TabsContent>

          <TabsContent value="custom" className="pt-3">
            {rule.kind === 'custom' && <CustomEditor />}
          </TabsContent>
        </Tabs>
      </CardContent>
    </Card>
  )
}

function CustomEditor() {
  const rule = useHistoryStore((s) => s.rule)
  const setRule = useHistoryStore((s) => s.setRule)
  const { t } = useTranslation()
  if (rule.kind !== 'custom') return null

  return (
    <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
      <NumberArrayField
        label={t('history.rule.uma_4p')}
        values={rule.uma4p}
        onChange={(vs) =>
          setRule({ ...rule, uma4p: vs as [number, number, number, number] })
        }
      />
      <NumberArrayField
        label={t('history.rule.uma_3p')}
        values={rule.uma3p}
        onChange={(vs) =>
          setRule({ ...rule, uma3p: vs as [number, number, number] })
        }
      />
      <NumberArrayField
        label={t('history.rule.dan_bonus_4p')}
        values={rule.danBonus4p}
        onChange={(vs) =>
          setRule({
            ...rule,
            danBonus4p: vs as [number, number, number, number],
          })
        }
      />
      <NumberArrayField
        label={t('history.rule.dan_bonus_3p')}
        values={rule.danBonus3p}
        onChange={(vs) =>
          setRule({ ...rule, danBonus3p: vs as [number, number, number] })
        }
      />
    </div>
  )
}

function NumberArrayField({
  label,
  values,
  onChange,
}: {
  label: string
  values: number[]
  onChange: (vs: number[]) => void
}) {
  return (
    <div className="space-y-1">
      <Label className="text-xs">{label}</Label>
      <div className="flex gap-1">
        {values.map((v, i) => (
          <Input
            key={i}
            type="number"
            value={v}
            onChange={(e) => {
              const n = parseFloat(e.target.value)
              if (!Number.isFinite(n)) return
              const next = [...values]
              next[i] = n
              onChange(next)
            }}
            className="w-full"
          />
        ))}
      </div>
    </div>
  )
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="space-y-1">
      <Label className="text-xs">{label}</Label>
      {children}
    </div>
  )
}
