import { describe, expect, it } from 'vitest'
import { parseAttachPort } from './chromiumPort'

describe('parseAttachPort', () => {
  it.each([
    ['0', 0],
    ['00000', 0],
    ['009222', 9222],
    ['65535', 65535],
    ['00065535', 65535],
  ])('accepts the decimal port %j as %j', (text, expected) => {
    expect(parseAttachPort(text)).toBe(expected)
  })

  it.each(['', '-1', '65536', '1.5', '1e3', 'abc', ' 9222 '])('rejects non-port text %j', (text) => {
    expect(parseAttachPort(text)).toBeNull()
  })
})
