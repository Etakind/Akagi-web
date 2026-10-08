import type { RefObject } from 'react'
import { Mahgen } from '@/components/Mahgen'
import { hexToRgba } from '@/lib/botShow'
import { mjaiToMahgen } from '@/lib/tileIdx'
import { cn } from '@/lib/utils'
import type { ShowItem } from '@/types'

// The bot's `meta.show` rows, rendered once and used twice: by the Bot Show
// dashboard tile and by the always-on-top overlay window. Purely presentational
// — see `@/lib/botShow` for the helpers that turn a `bot-response` into rows.
//
// The two variants differ in which axis drives the tile size, and that is the
// whole reason they can't share one layout:
//
// - `tile` — rows hug their content and the list scrolls inside a dashboard
//   tile that may be any height. Tile size follows the list's *width*.
// - `overlay` — the window is small and its height is the scarce axis. Rows
//   split the available height between them and each tile fills its own row, so
//   the list always reaches the bottom edge and the tiles grow when the user
//   drags the window taller. (Width-driven sizing here left a dead band under
//   the last row and tiles too small to read at a glance.)

type Props = {
  items: ShowItem[]
  /** `tile` variant only: the list element whose width drives tile sizing. */
  containerRef?: RefObject<HTMLOListElement | null>
  variant?: 'tile' | 'overlay'
  className?: string
}

export function BotShowList({ items, containerRef, variant = 'tile', className }: Props) {
  const overlay = variant === 'overlay'

  return (
    <ol
      ref={containerRef}
      className={cn('flex flex-col gap-1', overlay && 'h-full', className)}
      style={overlay ? { gap: 'calc(4px * var(--overlay-scale, 1))' } : undefined}
    >
      {items.map((it, i) => {
        const seq = it.tiles ?? (it.pais ? mjaiToMahgen(it.pais) : '')
        return (
          <li
            key={i}
            className={cn(
              'flex items-center gap-2 overflow-hidden rounded-md border border-border px-2',
              // flex-1 + min-h-0: the row's height comes from the window, not
              // from the tile inside it — which is what stops the tile's own
              // growth from feeding back into the measurement.
              overlay ? 'min-h-0 flex-1' : 'py-1.5',
            )}
            style={{
              backgroundColor: hexToRgba(it.color, 0.1),
              borderLeftColor: it.color,
              borderLeftWidth: it.color ? 3 : undefined,
            }}
          >
            {seq && (
              <Mahgen
                seq={seq}
                kind={overlay ? 'overlay-show' : 'bot-show'}
                // Overlay tiles measure their own row, so they must NOT be
                // handed the list element: Mahgen falls back to the parent <li>.
                containerRef={overlay ? undefined : containerRef}
              />
            )}
            <div className="flex min-w-0 flex-1 flex-col">
              {it.label && (
                <span
                  className="truncate text-sm text-foreground"
                  style={overlay ? {
                    fontSize: 'calc(14px * var(--overlay-scale, 1))',
                    lineHeight: 'calc(20px * var(--overlay-scale, 1))',
                  } : undefined}
                >
                  {it.label}
                </span>
              )}
              {it.note && (
                <span
                  className="truncate text-[10px] text-muted-foreground"
                  style={overlay ? {
                    fontSize: 'calc(10px * var(--overlay-scale, 1))',
                    lineHeight: 'calc(15px * var(--overlay-scale, 1))',
                  } : undefined}
                >
                  {it.note}
                </span>
              )}
            </div>
            {it.value && (
              <span
                style={overlay ? {
                  fontSize: 'calc(14px * var(--overlay-scale, 1))',
                  lineHeight: 'calc(20px * var(--overlay-scale, 1))',
                } : undefined}
                className={cn(
                  'font-mono tabular-nums text-foreground/90',
                  overlay ? 'text-sm' : 'text-xs',
                )}
              >
                {it.value}
              </span>
            )}
          </li>
        )
      })}
    </ol>
  )
}
