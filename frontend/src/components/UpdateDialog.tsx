import { useTranslation } from 'react-i18next'

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { openExternal } from '@/lib/external'
import { selectHasNotifiableUpdate, useUpdaterStore } from '@/stores/updaterStore'

/// Modal dialog for the in-app update flow. Opened by the sidebar
/// red-dot click, the "View details" toast action, or the Settings
/// "Update available" button. Footer actions match the four user
/// choices (Skip / Later / Open release page).
export function UpdateDialog() {
  const { t } = useTranslation()
  const open = useUpdaterStore((s) => s.dialogOpen)
  const closeDialog = useUpdaterStore((s) => s.closeDialog)
  const pending = useUpdaterStore((s) => s.pendingUpdate)
  const skip = useUpdaterStore((s) => s.skip)
  const hasNotifiable = useUpdaterStore(selectHasNotifiableUpdate)

  // Closing the dialog without any pending update is a no-op; mount-state
  // is intentionally driven entirely by the store so other components
  // (sidebar / settings) can trigger it without prop drilling.
  if (!pending) return null

  return (
    <Dialog open={open && hasNotifiable} onOpenChange={(v) => !v && closeDialog()}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle>{t('updates.dialog.title', { version: pending.latest_version })}</DialogTitle>
          <DialogDescription>{t('updates.dialog.security_build')}</DialogDescription>
        </DialogHeader>
        <div className="grid gap-2 max-h-[50vh] overflow-y-auto">
          <h3 className="text-sm font-semibold text-muted-foreground">
            {t('updates.dialog.notes_heading')}
          </h3>
          {/* Plain pre-formatted text. We don't pull a markdown renderer
              for this single use — release notes are short and the
              monospace block reads fine. */}
          <pre className="whitespace-pre-wrap break-words rounded-md border border-border bg-muted/40 p-3 text-xs leading-relaxed font-mono">
            {pending.body.trim() || t('updates.dialog.no_notes')}
          </pre>
        </div>
        <DialogFooter className="bg-transparent p-0 border-0 mx-0 mb-0 flex-wrap gap-2">
          <Button
            variant="outline"
            size="sm"
            onClick={() => openExternal(pending.html_url)}
          >
            {t('updates.dialog.open_release')}
          </Button>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => skip(pending.latest_tag)}
          >
            {t('updates.dialog.skip')}
          </Button>
          <Button variant="secondary" size="sm" onClick={closeDialog}>
            {t('updates.dialog.later')}
          </Button>

        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
