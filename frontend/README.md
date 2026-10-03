# Frontend development

React / TypeScript / Vite / Zustand / i18next desktop UI. Root user and maintenance guides are maintained in English and Simplified Chinese; existing UI languages remain.

From this directory, `npm ci` then `npm run build` produces the production frontend. Run `npm test` and `npm run lint` for regression checks.

`types.ts` mirrors Rust config/schema; `lib/tauri.ts` handles IPC; `useTauriBridge.ts` subscribes to local events.
Setup and Settings select Majsoul/Tenhou and attach/isolated Chromium. Cloud, billing, sharing and external-bot routes are removed.
Local themes accept constrained JSON colors; production CSP is in root tauri.conf.json. No remote theme fetch or cached arbitrary CSS injection.
Inspector displays redacted reasons while retaining old record compatibility. Old platform/source labels and the saved `proxy-control` layout ID are compatibility data only.
Run `node scripts/test-tiles-browser.mjs` for actual tile rendering under production CSP in a disposable browser. `scripts/mahgen-csp.mjs` patches the known embedded worker initialization without allowing dynamic code execution; dependency changes require reviewing this adapter.
