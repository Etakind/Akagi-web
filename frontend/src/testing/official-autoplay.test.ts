import { readFileSync } from 'node:fs'
import { JSDOM } from 'jsdom'
import { describe, expect, it } from 'vitest'

const guard = readFileSync('../src/autoplay/official_page.js', 'utf8')
describe('attached browser input scope', () => {
  it.each(['majsoul', 'tenhou'])('checks real Location for %s', (game) => {
    const host = game === 'majsoul' ? 'game.maj-soul.com' : 'tenhou.net'
    const path = game === 'majsoul' ? '/1/' : '/4/'
    for (const [url, allowed] of [
      [`https://${host}${path}`, true],
      [`https://${host}${path}?x=1`, true],
      [`http://${host}${path}`, false],
      [`https://${host}.evil.test${path}`, false],
      [`https://${host}:8443${path}`, false],
      [`https://name:secret@${host}${path}`, false],
      [`https://${host}/other/`, false],
      ['https://mail.example.test/', false],
      ['about:blank', false],
    ] as const) {
      const dom = new JSDOM('', { url, runScripts: 'outside-only' })
      try {
        expect('username' in dom.window.location).toBe(false)
        expect(dom.window.eval(guard.replace('__AKAGI_GAME__', JSON.stringify(game))), url).toBe(allowed)
      } finally { dom.window.close() }
    }
  })
})
