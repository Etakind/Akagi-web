import { describe, expect, it } from 'vitest'
import { existsSync, readFileSync } from 'node:fs'

const cdpSource = readFileSync('../src/capture/chromium/cdp.rs', 'utf8')
const recoverySource = readFileSync('../src/capture/chromium/recovery.rs', 'utf8')
const probeScript = '../src/capture/chromium/recovery_probe.js'
const closeScript = '../src/capture/chromium/recovery_close.js'

describe('reload-only recovery wiring', () => {
  it('does not ship automatic browser socket probes', () => {
    expect(cdpSource).not.toContain('prepare_probe')
    expect(cdpSource).not.toContain('recovery_probe.js')
    expect(cdpSource).not.toContain('recovery_close.js')
    expect(existsSync(probeScript)).toBe(false)
    expect(existsSync(closeScript)).toBe(false)
  })

  it('keeps recovery to one bounded page reload', () => {
    expect(recoverySource).toContain('driver.begin(RecoveryMethod::Reload)')
    expect(recoverySource).toContain('self.page.reload()')
    expect(recoverySource).toContain('Duration::from_secs(60)')
    expect(recoverySource).toContain('"reload_failed"')
    expect(recoverySource).toContain('"reload_restore_timeout"')
    expect(recoverySource).not.toContain('ReconnectOutcome')
    expect(recoverySource).not.toContain('fn reconnect')
  })
})
