// mahgen 1.0.0 embeds a Browserify worker with an old regenerator runtime.
// In a strict worker its global assignment throws, falling back to Function().
// Replace only that known initialization; never enable unsafe-eval in the UI.
export function mahgenCsp() {
  return {
    name: 'akagi-mahgen-csp',
    enforce: 'pre',
    transform(source, id) {
      if (!id.replaceAll('\\', '/').endsWith('/mahgen/dist/index.mjs')) return null
      const embedded = /\bN\s*=\s*"([A-Za-z0-9+/=]+)"/.exec(source)
      if (!embedded) throw new Error('mahgen worker structure changed: review CSP adapter')
      const worker = Buffer.from(embedded[1], 'base64').toString('utf8')
      const initialization = 'try{regeneratorRuntime=x}catch{Function("r","regeneratorRuntime = r")(x)}'
      if (worker.split(initialization).length !== 2) throw new Error('mahgen runtime changed: review CSP adapter')
      const safe = worker.replace(initialization, 'globalThis.regeneratorRuntime=x')
      if (/\b(?:eval|Function)\s*\(/.test(safe)) throw new Error('mahgen worker contains unsupported dynamic execution')
      return { code: source.replace(embedded[1], Buffer.from(safe).toString('base64')), map: null }
    },
  }
}
