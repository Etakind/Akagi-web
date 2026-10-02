import { render, screen } from '@testing-library/react'
import { describe, it, expect, vi } from 'vitest'
import { DetailPanel } from './InspectorView'
import type { InspectorEntry, FrameRaw } from '@/types'
vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key }) }))
const frame = (raw: FrameRaw): InspectorEntry => ({ kind: 'ws_frame', ts_ms: 1, direction: 'up', flow_id: 'fixture', size: 4, raw, emitted: 0 })
describe('Inspector raw record compatibility', () => {
  it('explains why new frames omit their payload', () => {
    render(<DetailPanel entry={frame({ format: 'redacted', data: 'omitted by privacy policy' })} />)
    expect(screen.getByText('inspector.detail_redacted')).toBeTruthy()
    expect(screen.getByText('omitted by privacy policy')).toBeTruthy()
  })
  it('still reads historical text records', () => {
    render(<DetailPanel entry={frame({ format: 'text', data: '<Z/>' })} />)
    expect(screen.getByText('<Z/>')).toBeTruthy()
  })
  it('still hex renders historical binary records', () => {
    render(<DetailPanel entry={frame({ format: 'binary', data: 'AQID' })} />)
    expect(screen.getByText(/01 02 03/)).toBeTruthy()
  })
})
