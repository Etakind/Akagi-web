import { readFileSync } from 'node:fs'
import { runInNewContext } from 'node:vm'
import { describe, expect, it } from 'vitest'

const guard = readFileSync('../src/autoplay/official_page.js', 'utf8')
describe('attached browser input scope', () => {
  it.each([
    ['https://game.maj-soul.com/1/', true],
    ['https://game.maj-soul.com/1/?x=1', true],
    ['http://game.maj-soul.com/1/', false],
    ['https://game.maj-soul.com.evil.test/1/', false],
    ['https://game.maj-soul.com:8443/1/', false],
    ['https://game.maj-soul.com/other/', false],
    ['https://mail.example.test/', false],
    ['about:blank', false],
  ])('checks the current page before input: %s', (url, allowed) => {
    expect(runInNewContext(guard, { location: new URL(url) })).toBe(allowed)
  })
})
