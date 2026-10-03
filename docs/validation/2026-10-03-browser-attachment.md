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
were executed. Manual connection of the rebuilt application has been initiated;
authorization/page subscription results will be recorded separately when known.
