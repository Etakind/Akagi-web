import { afterEach } from 'vitest'
import { cleanup } from '@testing-library/react'

// Vitest runs with `globals: false`, so Testing Library's auto-cleanup (which
// hooks a global `afterEach`) never registers. Do it explicitly.
afterEach(cleanup)

// jsdom ships no ResizeObserver, which Radix's ScrollArea constructs on mount.
if (!('ResizeObserver' in globalThis)) {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
}

/**
 * jsdom implements no `matchMedia`. Install a stub whose match state the test
 * controls, so components using `useIsNarrow` can be driven across the `lg`
 * breakpoint.
 *
 * Returns a `setMatches` that flips the result *and* notifies subscribers, the
 * way a real resize would.
 */
export function mockMatchMedia(initialMatches: boolean) {
  const listeners = new Set<() => void>()
  let matches = initialMatches

  window.matchMedia = ((query: string) => ({
    matches,
    media: query,
    onchange: null,
    addEventListener: (_: string, cb: () => void) => void listeners.add(cb),
    removeEventListener: (_: string, cb: () => void) => void listeners.delete(cb),
    addListener: (cb: () => void) => void listeners.add(cb),
    removeListener: (cb: () => void) => void listeners.delete(cb),
    dispatchEvent: () => false,
  })) as unknown as typeof window.matchMedia

  return function setMatches(next: boolean) {
    matches = next
    for (const cb of listeners) cb()
  }
}

// Use browser-like in-memory Storage when the Node Web Storage global shadows
// jsdom's implementation. This is confined to the test environment.
function testStorage(): Storage {
  const data = new Map<string, string>()
  return {
    get length() { return data.size },
    clear() { data.clear() },
    getItem(key) { return data.get(String(key)) ?? null },
    setItem(key, value) { data.set(String(key), String(value)) },
    removeItem(key) { data.delete(String(key)) },
    key(index) { return [...data.keys()][index] ?? null },
  }
}
for (const name of ['localStorage', 'sessionStorage'] as const) {
  const storage = window[name] ?? testStorage()
  Object.defineProperty(window, name, { value: storage, configurable: true })
  Object.defineProperty(globalThis, name, { value: storage, configurable: true })
}
