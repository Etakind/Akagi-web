import { Mahgen } from 'mahgen'

// Keep the existing mahgen image compositor, but make failures visible without
// the upstream custom element logging the supplied hand/sequence.
export class TileRenderer extends HTMLElement {
  private image: HTMLImageElement
  private revision = 0
  static get observedAttributes() { return ['data-seq', 'data-river-mode'] }

  constructor() {
    super()
    this.image = document.createElement('img')
    this.attachShadow({ mode: 'open' }).appendChild(this.image)
  }

  attributeChangedCallback() { void this.renderTiles() }

  private async renderTiles() {
    const revision = ++this.revision
    const seq = this.getAttribute('data-seq')
    if (!seq) {
      this.image.removeAttribute('src')
      this.image.alt = ''
      this.removeAttribute('data-render-error')
      return
    }
    try {
      const image = await Mahgen.render(seq, this.hasAttribute('data-river-mode'))
      if (revision !== this.revision) return
      this.image.src = image
      this.image.alt = ''
      this.removeAttribute('data-render-error')
      this.removeAttribute('title')
    } catch {
      if (revision !== this.revision) return
      this.image.removeAttribute('src')
      this.image.alt = '⚠'
      this.setAttribute('data-render-error', '')
      this.title = 'Tile image unavailable'
      console.warn('AKAGI_TILE_RENDER_FAILED')
    }
  }
}

customElements.define('akagi-tiles', TileRenderer)
