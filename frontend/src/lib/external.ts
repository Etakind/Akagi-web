import { invoke } from '@/lib/tauri'

// Project-wide community / source links. Used by the first-run wizard
// banners and the sidebar footer; kept here so the URLs aren't
// duplicated across components.
export const AKAGI_GITHUB_URL = 'https://github.com/shinkuan/Akagi'
export const AKAGI_DISCORD_URL = 'https://discord.gg/Z2wjXUK8bN'

// Tauri 2's webview doesn't reliably honour `<a target="_blank">`
// without the opener plugin, so route external links through the
// `open_external_url` backend command, which hands the URL to the OS
// default-browser handler (ShellExecuteW / open / xdg-open).
export function openExternal(url: string): void {
  invoke('open_external_url', { url }).catch(() => {
    /* surfaced via Sonner toast hooked into the tauri error bridge */
  })
}
