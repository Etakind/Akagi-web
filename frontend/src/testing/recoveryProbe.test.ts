import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import vm from 'node:vm'

const observe = readFileSync('../src/capture/chromium/recovery_probe.js', 'utf8')
const close = readFileSync('../src/capture/chromium/recovery_close.js', 'utf8')
class Socket {
  static OPEN = 1
  readyState = 1
  listeners = new Set<(event: { data: ArrayBuffer }) => void>()
  closes = 0
  addEventListener(_: string, callback: (event: { data: ArrayBuffer }) => void) { this.listeners.add(callback) }
  removeEventListener(_: string, callback: (event: { data: ArrayBuffer }) => void) { this.listeners.delete(callback) }
  close() { this.closes++; this.readyState = 3 }
  async emit(name: string) {
    const bytes = new TextEncoder().encode(name)
    const frame = new Uint8Array([1, 10, bytes.length, ...bytes]).buffer
    for (const listener of this.listeners) listener({ data: frame })
    await Promise.resolve(); await Promise.resolve()
  }
}
function setup(sockets: Socket[]) {
  const scope = vm.createContext({ window: {}, WebSocket: Socket, TextDecoder, ArrayBuffer, Blob, Uint8Array })
  vm.runInContext(`(${observe}).call(sockets)`, vm.createContext({ ...scope, sockets }))
  return scope
}
describe('game connection identification', () => {
  it('closes only the observed game socket and never a lobby/probe socket', async () => {
    const game = new Socket(), lobby = new Socket(), probe = new Socket()
    const scope = setup([lobby, game, probe])
    await lobby.emit('.lq.NotifyAccountUpdate'); await game.emit('.lq.ActionPrototype')
    expect(vm.runInContext(close, scope)).toBe('closed')
    expect([lobby.closes, game.closes, probe.closes]).toEqual([0, 1, 0])
    expect(vm.runInContext(close, scope)).toBe('unidentified')
  })
  it('refuses ambiguous games and never closes unclassified connections', async () => {
    const a = new Socket(), b = new Socket()
    const scope = setup([a, b])
    expect(vm.runInContext(close, scope)).toBe('unidentified')
    await a.emit('.lq.ActionPrototype'); await b.emit('.lq.ActionPrototype')
    expect(vm.runInContext(close, scope)).toBe('ambiguous')
    expect(a.closes + b.closes).toBe(0)
  })
  it('does not install duplicate observers', () => {
    const game = new Socket(); const scope = setup([game])
    vm.runInContext(`(${observe}).call(sockets)`, vm.createContext({ ...scope, sockets: [game] }))
    expect(game.listeners.size).toBe(1)
  })
})
