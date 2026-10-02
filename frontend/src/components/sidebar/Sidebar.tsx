import { useEffect, useState } from 'react'
import { Link, useLocation } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { ChevronLeft, Megaphone, X } from 'lucide-react'

import { cn } from '@/lib/utils'
import { Button } from '@/components/ui/button'
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/components/ui/tooltip'
import { useSidebar } from '@/hooks/useSidebar'
import { useIsNarrow } from '@/hooks/useIsNarrow'
import { GithubMark, DiscordMark } from '@/components/BrandMarks'
import { AkagiIcon, AkagiWordmark } from '@/components/BrandLogo'
import { AKAGI_GITHUB_URL, AKAGI_DISCORD_URL, openExternal } from '@/lib/external'
import { getAppVersion, VERSION_FALLBACK } from '@/lib/appVersion'
import { LANG_LABELS, SUPPORTED_LANGS, type SupportedLang } from '@/i18n'
import { selectHasNotifiableUpdate, useUpdaterStore } from '@/stores/updaterStore'
import { useAnnouncementStore } from '@/stores/announcementStore'
import { Menu } from './Menu'

export function Sidebar() {
  const { t, i18n } = useTranslation()
  const isOpen = useSidebar((s) => s.isOpen)
  const toggleOpen = useSidebar((s) => s.toggleOpen)
  const setIsHover = useSidebar((s) => s.setIsHover)
  const isHover = useSidebar((s) => s.isHover)
  const settings = useSidebar((s) => s.settings)
  const isDrawerOpen = useSidebar((s) => s.isDrawerOpen)
  const setDrawerOpen = useSidebar((s) => s.setDrawerOpen)
  const isNarrow = useIsNarrow()
  // `key`, not `pathname`: clicking the nav item for the route you're already
  // on pushes a new history entry without changing the path, and the drawer
  // still has to get out of the way.
  const { key: locationKey } = useLocation()
  const hasUpdate = useUpdaterStore(selectHasNotifiableUpdate)
  const openUpdateDialog = useUpdaterStore((s) => s.openDialog)
  const [version, setVersion] = useState(VERSION_FALLBACK)
  useEffect(() => {
    getAppVersion().then(setVersion)
  }, [])

  // Navigating closes the drawer — otherwise it would sit on top of the very
  // page the user just asked for.
  useEffect(() => {
    setDrawerOpen(false)
  }, [locationKey, setDrawerOpen])

  // Growing back past `lg` re-docks the sidebar. Drop the drawer state so the
  // backdrop can't linger, and so shrinking again starts from closed.
  useEffect(() => {
    if (!isNarrow) setDrawerOpen(false)
  }, [isNarrow, setDrawerOpen])

  useEffect(() => {
    if (!isNarrow || !isDrawerOpen) return
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setDrawerOpen(false)
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [isNarrow, isDrawerOpen, setDrawerOpen])

  // `open` includes the transient hover-open state. Only `isOpen` (pinned)
  // affects main content margin in App.tsx — hover-open expands the sidebar
  // visually as an overlay above main, so width-sensitive widgets like
  // react-grid-layout don't thrash on every cursor pass.
  //
  // As a drawer it is always fully expanded: a 5.625rem rail of bare icons is
  // a pointless middle state for something that's already an overlay.
  const open = isNarrow || isOpen || (settings.isHoverOpen && isHover)
  const showBackdrop = isNarrow && isDrawerOpen && !settings.disabled

  return (
    <>
      {showBackdrop && (
        <div
          className="fixed inset-0 z-30 bg-black/50"
          onClick={() => setDrawerOpen(false)}
          aria-hidden="true"
        />
      )}
      <aside
        // Off-screen drawer contents stay in the tree (so the slide-out
        // animates) but must not be reachable by Tab or screen readers.
        inert={isNarrow && !isDrawerOpen}
        className={cn(
          'fixed top-0 left-0 h-screen transition-[width,transform] ease-in-out duration-300',
          isNarrow
            ? cn('z-40 w-[18rem]', isDrawerOpen ? 'translate-x-0' : '-translate-x-full')
            : cn('z-20 translate-x-0', open ? 'w-[18rem]' : 'w-[5.625rem]'),
          settings.disabled && 'hidden',
        )}
      >
        <div
          // Hover-peek is a docked-sidebar affordance. In drawer mode the
          // element is off-screen, so hovering it is meaningless.
          onMouseEnter={() => !isNarrow && setIsHover(true)}
          onMouseLeave={() => !isNarrow && setIsHover(false)}
          className="relative h-full flex flex-col px-3 py-4 overflow-hidden bg-sidebar text-sidebar-foreground border-r border-border shadow-md dark:shadow-zinc-800"
        >
        <div
          className={cn(
            'flex items-center mb-2 shrink-0',
            open ? 'justify-between gap-2' : 'justify-center',
          )}
        >
          <Link
            to="/"
            className={cn(
              'flex items-center gap-2 rounded-md px-1 py-1 hover:opacity-80 transition-opacity',
              !open && 'justify-center',
            )}
            aria-label="Akagi"
          >
            {open ? (
              <span className="flex items-center gap-1.5 whitespace-nowrap">
                <AkagiWordmark className="h-7" />
                <span className="text-xs text-muted-foreground font-normal">V3</span>
              </span>
            ) : (
              <AkagiIcon className="h-7" />
            )}
          </Link>
          {open &&
            (isNarrow ? (
              // Pinning has no meaning for an overlay — the only useful
              // action here is dismissing it.
              <Button
                variant="ghost"
                size="icon"
                onClick={() => setDrawerOpen(false)}
                className="h-7 w-7 text-muted-foreground hover:text-foreground"
                aria-label={t('sidebar.closeMenu')}
              >
                <X className="h-4 w-4" />
              </Button>
            ) : (
              <Button
                variant="ghost"
                size="icon"
                onClick={toggleOpen}
                className="h-7 w-7 text-muted-foreground hover:text-foreground"
                aria-label={isOpen ? t('sidebar.collapse') : t('sidebar.pin')}
              >
                <ChevronLeft
                  className={cn(
                    'h-4 w-4 transition-transform duration-300',
                    !isOpen && 'rotate-180',
                  )}
                />
              </Button>
            ))}
        </div>
        <Menu isOpen={open} />
        {/* Footer icon row — announcements plus the GitHub / Discord /
            AkagiMS links. Always rendered, even when collapsed, so all
            stay reachable in both states. The version + language picker
            rides along below it but only when there's room (open state). */}
        <div
          className={cn(
            // flex-wrap: the collapsed rail (5.625rem) fits two icon
            // buttons per row, so these wrap onto two rows.
            'mt-2 shrink-0 flex flex-wrap items-center gap-1 border-t border-border pt-3',
            open ? 'justify-start px-1' : 'justify-center',
          )}
        >
          <SidebarIconButton
            label={t('sidebar.announcements')}
            collapsed={!open}
            onClick={() => useAnnouncementStore.getState().openHistory()}
          >
            <Megaphone className="h-4 w-4" />
          </SidebarIconButton>
          <SidebarIconButton
            label="GitHub"
            collapsed={!open}
            onClick={() => openExternal(AKAGI_GITHUB_URL)}
          >
            <GithubMark className="h-4 w-4" />
          </SidebarIconButton>
          <SidebarIconButton
            label="Discord"
            collapsed={!open}
            onClick={() => openExternal(AKAGI_DISCORD_URL)}
          >
            <DiscordMark className="h-4 w-4" />
          </SidebarIconButton>
        </div>
        {open && (
          <div className="mt-2 shrink-0 flex items-center justify-between gap-2 text-xs text-muted-foreground">
            <button
              type="button"
              onClick={hasUpdate ? openUpdateDialog : undefined}
              className={cn(
                'flex items-center gap-1.5 rounded px-1 py-0.5',
                hasUpdate
                  ? 'cursor-pointer hover:text-foreground'
                  : 'cursor-default',
              )}
              aria-label={
                hasUpdate ? t('updates.toast.action') : undefined
              }
              disabled={!hasUpdate}
            >
              <span>v{version}</span>
              {hasUpdate && (
                <span
                  className="inline-block h-1.5 w-1.5 rounded-full bg-red-500"
                  aria-hidden="true"
                />
              )}
            </button>
            <select
              className="bg-transparent border border-border rounded px-1.5 py-0.5"
              value={i18n.language}
              onChange={(e) => void i18n.changeLanguage(e.target.value)}
            >
              {SUPPORTED_LANGS.map((lang) => (
                <option key={lang} value={lang}>
                  {LANG_LABELS[lang as SupportedLang]}
                </option>
              ))}
            </select>
          </div>
        )}
        </div>
      </aside>
    </>
  )
}

function SidebarIconButton({
  label,
  collapsed,
  onClick,
  children,
}: {
  label: string
  collapsed: boolean
  onClick: () => void
  children: React.ReactNode
}) {
  // Only show the right-side tooltip when the sidebar is collapsed —
  // when expanded, the button sits in plain view and tooltips would
  // just be visual noise.
  return (
    <TooltipProvider disableHoverableContent>
      <Tooltip delayDuration={100}>
        <TooltipTrigger asChild>
          <Button
            variant="ghost"
            size="icon"
            className="h-9 w-9 text-muted-foreground hover:text-foreground"
            onClick={onClick}
            aria-label={label}
          >
            {children}
          </Button>
        </TooltipTrigger>
        {collapsed && <TooltipContent side="right">{label}</TooltipContent>}
      </Tooltip>
    </TooltipProvider>
  )
}
