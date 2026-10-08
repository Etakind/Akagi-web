// mahgen 1.0.0 embeds a Browserify worker with an old regenerator runtime.
// In a strict worker its global assignment throws, falling back to Function().
// Replace only that known initialization; never enable unsafe-eval in the UI.
export const MAHGEN_DATA_PREFIX = '\0akagi-mahgen-data:'

export function mahgenCsp() {
  const dataModules = new Map()
  return {
    name: 'akagi-mahgen-csp',
    enforce: 'pre',
    buildStart() {
      dataModules.clear()
    },
    resolveId(id) {
      return id.startsWith(MAHGEN_DATA_PREFIX) ? id : null
    },
    load(id) {
      return dataModules.get(id) ?? null
    },
    transform(source, id) {
      if (!id.split(/[?#]/, 1)[0].replaceAll('\\', '/').endsWith('/mahgen/dist/index.mjs')) return null
      const embedded = /\bN\s*=\s*"([A-Za-z0-9+/=]+)"/.exec(source)
      if (!embedded) throw new Error('mahgen worker structure changed: review CSP adapter')
      const worker = Buffer.from(embedded[1], 'base64').toString('utf8')
      const initialization = 'try{regeneratorRuntime=x}catch{Function("r","regeneratorRuntime = r")(x)}'
      if (worker.split(initialization).length !== 2) throw new Error('mahgen runtime changed: review CSP adapter')
      const safe = worker.replace(initialization, 'globalThis.regeneratorRuntime=x')
      if (/\b(?:eval|Function)\s*\(/.test(safe)) throw new Error('mahgen worker contains unsupported dynamic execution')
      // mahgen embeds megabytes of PNG/worker base64 in one module, which a
      // module-level chunker cannot split. Export only pure strings from bounded
      // data modules; keep Blob/Worker creation and the image protocol unchanged.
      const chunks = ['']
      const imports = [[]]
      let sequence = 0
      const store = (value) => {
        const name = `__akagiMahgenData${sequence++}`
        const declaration = `export const ${name} = ${JSON.stringify(value)};\n`
        let index = chunks.length - 1
        if (chunks[index].length + declaration.length > 300_000) {
          index = chunks.length
          chunks.push('')
          imports.push([])
        }
        chunks[index] += declaration
        imports[index].push(name)
        return name
      }
      const base64 = Buffer.from(safe).toString('base64')
      const pieces = []
      for (let start = 0; start < base64.length; start += 250_000) {
        pieces.push(store(base64.slice(start, start + 250_000)))
      }
      let code = source.replace(`"${embedded[1]}"`, pieces.join(' + '))
      let images = 0
      code = code.replace(/"(iVBORw0KGgo[A-Za-z0-9+/=]+)"/g, (_, value) => {
        images++
        if (value.length > 290_000) throw new Error('mahgen image size changed: review data adapter')
        return store(value)
      })
      if (images === 0) throw new Error('mahgen images changed: review data adapter')
      const headers = chunks.map((chunk, index) => {
        const moduleId = `${MAHGEN_DATA_PREFIX}${index}`
        dataModules.set(moduleId, chunk)
        return `import { ${imports[index].join(', ')} } from ${JSON.stringify(moduleId)};`
      })
      return { code: `${headers.join('\n')}\n${code}`, map: null }
    },
    generateBundle(_, bundle) {
      // Keep Vite's default 500kB boundary; fail rather than suppress a warning.
      for (const output of Object.values(bundle)) {
        if (output.type === 'chunk' && Buffer.byteLength(output.code) > 500_000) {
          this.error(`JavaScript chunk ${output.fileName} exceeds 500kB; review chunking`)
        }
      }
    },
  }
}
