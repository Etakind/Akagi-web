import { describe, expect, it } from 'vitest'

import { compareVersions } from '@/lib/appVersion'
import { ANNOUNCEMENTS } from './entries'

import en from '@/i18n/resources/en.json'
import ja from '@/i18n/resources/ja.json'
import zhTW from '@/i18n/resources/zh-TW.json'
import zhCN from '@/i18n/resources/zh-CN.json'

const LOCALES = { en, ja, 'zh-TW': zhTW, 'zh-CN': zhCN } as const

type AnnouncementsBlock = {
  dialog: Record<string, string>
  entries: Record<string, Record<string, string>>
}

function announcementsOf(locale: keyof typeof LOCALES): AnnouncementsBlock {
  return (LOCALES[locale] as { announcements: AnnouncementsBlock }).announcements
}

describe('ANNOUNCEMENTS data', () => {
  it('has unique ids', () => {
    const ids = ANNOUNCEMENTS.map((e) => e.id)
    expect(new Set(ids).size).toBe(ids.length)
  })

  it('is dated strictly newest-first (dates double as the seen-baseline order)', () => {
    for (let i = 1; i < ANNOUNCEMENTS.length; i++) {
      expect(
        ANNOUNCEMENTS[i - 1].date > ANNOUNCEMENTS[i].date,
        `${ANNOUNCEMENTS[i - 1].id} (${ANNOUNCEMENTS[i - 1].date}) must be dated after ${ANNOUNCEMENTS[i].id} (${ANNOUNCEMENTS[i].date})`,
      ).toBe(true)
    }
  })

  it('uses valid ISO dates', () => {
    for (const e of ANNOUNCEMENTS) {
      expect(e.date, `date of ${e.id}`).toMatch(/^\d{4}-\d{2}-\d{2}$/)
      expect(Number.isNaN(new Date(`${e.date}T00:00:00`).getTime())).toBe(false)
    }
  })

  it('keeps versioned entries ordered like their dates', () => {
    const versioned = ANNOUNCEMENTS.filter((e) => e.version !== undefined)
    for (let i = 1; i < versioned.length; i++) {
      expect(
        compareVersions(versioned[i - 1].version!, versioned[i].version!),
        `${versioned[i - 1].version} should be newer than ${versioned[i].version}`,
      ).toBeGreaterThan(0)
    }
  })

  it('has at least one feature per entry', () => {
    for (const e of ANNOUNCEMENTS) {
      expect(e.features.length, `features of ${e.id}`).toBeGreaterThan(0)
    }
  })

  it('has every entry string in every locale', () => {
    for (const locale of Object.keys(LOCALES) as (keyof typeof LOCALES)[]) {
      const block = announcementsOf(locale)
      for (const e of ANNOUNCEMENTS) {
        const strings = block.entries[e.id]
        expect(strings, `${locale}: announcements.entries.${e.id}`).toBeTruthy()
        const needed = ['title']
        for (const f of e.features) needed.push(`${f.key}_title`, `${f.key}_desc`)
        if (e.image) needed.push('image_alt')
        if (e.link) needed.push('link_label')
        for (const leaf of needed) {
          const value = strings[leaf]
          expect(
            typeof value === 'string' && value.length > 0,
            `${locale}: announcements.entries.${e.id}.${leaf}`,
          ).toBe(true)
        }
      }
    }
  })

  it('has the dialog UI strings in every locale', () => {
    const needed = ['title', 'intro', 'got_it', 'all_releases', 'settings_button']
    for (const locale of Object.keys(LOCALES) as (keyof typeof LOCALES)[]) {
      const block = announcementsOf(locale)
      for (const key of needed) {
        expect(
          typeof block.dialog[key] === 'string' && block.dialog[key].length > 0,
          `${locale}: announcements.dialog.${key}`,
        ).toBe(true)
      }
    }
  })
})
