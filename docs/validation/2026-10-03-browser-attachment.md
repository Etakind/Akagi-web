# Edge attachment discovery — 2026-10-03

The existing Edge service listens on loopback port 9222 but does not expose
`/json/version` (HTTP 404). A port-matching `DevToolsActivePort` exists in Edge's
standard user-data root. Previously, Akagi read that locator only when a profile
was explicitly configured, so automatic configuration failed before authorization.

The fix tries HTTP discovery first for automatic profiles, then reads only known
browser locator files. It validates file ownership/type/link safety and the port,
deduplicates candidates and refuses ambiguity. Explicit invalid locators fail
closed. Reconnection repeats discovery; authorization rejection is not retried.
No Cookie, account, session database or session-restore file is inspected.

Executed: production frontend build and `cargo build --locked --release --features
custom-protocol` succeeded on the current macOS host. No automated tests or CI
were executed. The manual result below belongs to the standalone discovery fix
(commit `51a936e`), before the broader local-web/platform changes.

Manual result: the new release reached the browser authorization handshake,
confirming locator discovery succeeded. The handshake timed out after 120 seconds
(`approval_timeout=true`, `cdp_connected=false`). Official page subscription was
not reached. No rejection/Origin bypass, browser restart, navigation or game action
was attempted. The check process was then stopped; the attached Edge was left open.
