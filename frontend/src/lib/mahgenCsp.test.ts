import { describe, expect, it } from 'vitest'
import { MAHGEN_DATA_PREFIX, mahgenCsp } from '../../scripts/mahgen-csp.mjs'

const INITIALIZATION = 'try{regeneratorRuntime=x}catch{Function("r","regeneratorRuntime = r")(x)}'
const PNG_A = 'iVBORw0KGgoAAA='
const PNG_B = 'iVBORw0KGgoBBB='

type DataPlugin = ReturnType<typeof mahgenCsp>

function hook<T extends keyof DataPlugin>(plugin: DataPlugin, name: T): (...args: unknown[]) => unknown {
  return plugin[name] as unknown as (...args: unknown[]) => unknown
}

function exportsFrom(source: string): string[] {
  return [...source.matchAll(/export const \w+ = ("(?:\\.|[^"])*");/g)]
    .map((match) => JSON.parse(match[1]) as string)
}

describe('mahgen CSP adapter', () => {
  it('keeps worker and PNG data in bounded pure-data virtual modules', () => {
    const worker = `self.keep = "prefix+/=";${INITIALIZATION};self.payload="${'A'.repeat(200_001)}";self.keep = "suffix"`
    const workerBase64 = btoa(worker)
    const source = [
      'const before = "unchanged";',
      `const N = "${workerBase64}";`,
      `const first = "${PNG_A}";`,
      `const second = "${PNG_B}";`,
      'const after = "unchanged";',
    ].join('\n')
    const plugin = mahgenCsp()
    const transformed = hook(plugin, 'transform')(source, '/virtual/node_modules/mahgen/dist/index.mjs?virtual#data') as { code: string }

    expect(transformed.code).toContain('const before = "unchanged";')
    expect(transformed.code).toContain('const after = "unchanged";')
    expect(transformed.code).not.toContain(`N = "${workerBase64}"`)

    const ids = [...transformed.code.matchAll(/from ".*akagi-mahgen-data:(\d+)"/g)]
      .map((match) => `${MAHGEN_DATA_PREFIX}${match[1]}`)
    expect(ids.length).toBeGreaterThan(0)
    const data = ids.flatMap((id) => exportsFrom(hook(plugin, 'load')(id) as string))
    const safeBase64 = btoa(worker.replace(INITIALIZATION, 'globalThis.regeneratorRuntime=x'))
    const workerPieceCount = Math.ceil(safeBase64.length / 250_000)
    expect(data.slice(0, workerPieceCount).join('')).toBe(safeBase64)
    expect(data.slice(workerPieceCount)).toEqual(expect.arrayContaining([PNG_A, PNG_B]))
    expect(data.every((value) => value.length < 300_000)).toBe(true)
    expect(atob(data.slice(0, workerPieceCount).join(''))).not.toContain('Function(')
    expect(atob(data.slice(0, workerPieceCount).join(''))).toContain('globalThis.regeneratorRuntime=x')
    expect(hook(plugin, 'resolveId')(ids[0])).toBe(ids[0])
    expect(hook(plugin, 'resolveId')('other-module')).toBeNull()
  })

  it('fails closed when mahgen data shape or images change', () => {
    const plugin = mahgenCsp()
    const transform = hook(plugin, 'transform')
    const worker = btoa(`self.ok=1;${INITIALIZATION}`)
    expect(() => transform(`const N = "${worker}";`, '/mahgen/dist/index.mjs')).toThrow(/images changed/)
    expect(() => transform('const other = "not-mahgen";', '/mahgen/dist/index.mjs')).toThrow(/worker structure changed/)
  })

  it('rejects generated chunks over the release size boundary', () => {
    const plugin = mahgenCsp()
    const generateBundle = hook(plugin, 'generateBundle')
    const context = {
      error(message: string) {
        throw new Error(message)
      },
    }
    expect(() => generateBundle.call(context, {}, {
      'large.js': { type: 'chunk', fileName: 'large.js', code: 'x'.repeat(500_001) },
    })).toThrow(/500kB/)
  })
})
