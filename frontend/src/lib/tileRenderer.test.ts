import { afterEach, describe, expect, it, vi } from 'vitest'
import { Mahgen } from 'mahgen'
import './tileRenderer'

vi.mock('mahgen', () => ({ Mahgen: { render: vi.fn() } }))
afterEach(() => { document.body.replaceChildren(); vi.restoreAllMocks() })

const settle = () => new Promise<void>((resolve) => queueMicrotask(resolve))
describe('tile renderer failure boundary', () => {
  it('shows a placeholder without logging the sequence or thrown payload, then recovers', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    vi.mocked(Mahgen.render).mockRejectedValueOnce(new Error('SYNTHETIC_PRIVATE_PAYLOAD'))
    const el = document.createElement('akagi-tiles')
    document.body.append(el)
    el.setAttribute('data-seq', 'SYNTHETIC_PRIVATE_SEQUENCE')
    await settle()
    expect(el.hasAttribute('data-render-error')).toBe(true)
    expect(el.shadowRoot?.querySelector('img')?.alt).toBe('⚠')
    expect(warn.mock.calls).toEqual([['AKAGI_TILE_RENDER_FAILED']])
    vi.mocked(Mahgen.render).mockResolvedValueOnce('data:image/png;base64,AA==')
    el.setAttribute('data-seq', '123m')
    await settle()
    expect(el.hasAttribute('data-render-error')).toBe(false)
    expect(el.shadowRoot?.querySelector('img')?.getAttribute('src')).toBe('data:image/png;base64,AA==')
  })

  it('does not overwrite a newer hand with an older completed render', async () => {
    let finishOld!: (value: string) => void
    vi.mocked(Mahgen.render).mockReturnValueOnce(new Promise(resolve => { finishOld = resolve }))
    const el = document.createElement('akagi-tiles')
    el.setAttribute('data-seq', '123m')
    vi.mocked(Mahgen.render).mockResolvedValueOnce('data:image/png;base64,NEW')
    el.setAttribute('data-seq', '456p')
    await settle()
    finishOld('data:image/png;base64,OLD')
    await settle()
    expect(el.shadowRoot?.querySelector('img')?.getAttribute('src')).toBe('data:image/png;base64,NEW')
  })
})
