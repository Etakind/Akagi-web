import { describe, expect, it } from 'vitest'

import en from './resources/en.json'
import ja from './resources/ja.json'
import zhCN from './resources/zh-CN.json'
import zhTW from './resources/zh-TW.json'

const locales = { en, ja, 'zh-CN': zhCN, 'zh-TW': zhTW }

describe('reload recovery translations', () => {
  it('defines both reload failure reasons in every supported locale', () => {
    for (const [language, resource] of Object.entries(locales)) {
      expect(resource.recovery.description, language).toBeTruthy()
      expect(resource.recovery.reason.reload_failed, language).toBeTruthy()
      expect(resource.recovery.reason.reload_restore_timeout, language).toBeTruthy()
    }
  })

  it('labels restored suggestions as manual-only in every supported locale', () => {
    for (const [language, resource] of Object.entries(locales)) {
      expect(resource.recovery.advisory_only, language).toBeTruthy()
      expect(resource.recovery.advisory_only.toLowerCase(), language).toMatch(
        /manual|手動|手动|操作/,
      )
    }
  })

  it('defines the invalid attach-port save error in every supported locale', () => {
    for (const [language, resource] of Object.entries(locales)) {
      expect(resource.settings.attach_port_invalid, language).toBeTruthy()
      expect(resource.settings.attach_port_invalid, language).not.toBe(
        'settings.attach_port_invalid',
      )
    }
  })
})
