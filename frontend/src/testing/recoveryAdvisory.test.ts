import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'

const eventBusSource = readFileSync('../src/event_bus.rs', 'utf8')
const recoverySource = readFileSync('../src/capture/recovery.rs', 'utf8')
const bridgeSource = readFileSync('../src/bridge/majsoul/mod.rs', 'utf8')
const runnerSource = readFileSync('../src/bot/runner.rs', 'utf8')
const managerSource = readFileSync('../src/bot/manager.rs', 'utf8')

describe('restored manual advisory wiring', () => {
  it('tracks a display-only restore revision without making input valid', () => {
    expect(recoverySource).toContain('window_revision')
    expect(eventBusSource).toContain('advisory_revision')
    expect(eventBusSource).toContain('valid_for_display')
    expect(eventBusSource).toContain('if self.advisory_revision.is_some()')
    expect(bridgeSource).toContain('restored_offered')
    expect(bridgeSource).toContain('.last()?')
  })

  it('has a one-shot runner path that never serializes an executable action', () => {
    expect(runnerSource).toContain('suggest_restored')
    expect(managerSource).toContain('suggest_restored')
    expect(managerSource).toContain('advisory_only')
    expect(managerSource).toContain('decision_started: None')
  })
})
