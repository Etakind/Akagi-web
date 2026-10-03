import type { PlatformKind } from '@/types'
export const PLATFORMS = [
  { kind: 'Majsoul' as const, labelKey: 'platform.majsoul', defaultStartUrl: 'https://game.maj-soul.com/1/' },
  { kind: 'Tenhou' as const, labelKey: 'platform.tenhou', defaultStartUrl: 'https://tenhou.net/4/' },
]
export function platformInfo(kind: PlatformKind = 'Majsoul') { return PLATFORMS.find(p => p.kind === kind)! }
