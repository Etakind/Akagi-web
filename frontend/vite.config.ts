import path from 'node:path'
// `vitest/config` re-exports Vite's `defineConfig` widened with the `test`
// field, so one config file still drives both `vite build` and `vitest`.
import { defineConfig } from 'vitest/config'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import { mahgenCsp, MAHGEN_DATA_PREFIX } from './scripts/mahgen-csp.mjs'

const HOST = process.env.TAURI_DEV_HOST

export default defineConfig({
  plugins: [
    mahgenCsp(),
    {
      name: 'akagi-client-only-directives',
      apply: 'build',
      enforce: 'pre',
      transform(source) {
        // This bundle runs entirely in browser webviews, never React Server
        // Components. Remove only the leading RSC client-boundary annotation;
        // keep real JS directives and fail on any other bundler warning.
        const code = source.replace(
          /^((?:\s|\/\*[\s\S]*?\*\/|\/\/[^\n]*(?:\n|$)|"use strict";?|'use strict';?)*)(["'])use client\2;?\r?\n/,
          '$1',
        )
        return code === source ? null : { code, map: null }
      },
    },
    react(),
    tailwindcss(),
  ],
  optimizeDeps: { exclude: ['mahgen'] },
  resolve: {
    alias: {
      '@': path.resolve(import.meta.dirname, './src'),
    },
  },
  build: {
    rolldownOptions: {
      preserveEntrySignatures: 'allow-extension',
      onLog(level, log, handler) {
        // Vite's handler only logs `error`; throwing is required to fail a build.
        if (level === 'warn') throw new Error(log.message)
        handler(level, log)
      },
      output: {
        strictExecutionOrder: true,
        codeSplitting: {
          groups: [
            {
              name: (id) => `mahgen-data-${id.slice(MAHGEN_DATA_PREFIX.length)}`,
              debugName: 'mahgen-data',
              test: (id) => id.startsWith(MAHGEN_DATA_PREFIX),
              priority: 20,
              includeDependenciesRecursively: false,
            },
            {
              name: 'vendor',
              test: /node_modules/,
              maxSize: 300_000,
              priority: 10,
              includeDependenciesRecursively: false,
            },
            {
              name: 'app',
              test: /[\\/]src[\\/]/,
              maxSize: 300_000,
              includeDependenciesRecursively: false,
            },
          ],
        },
      },
    },
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: HOST || false,
    hmr: HOST
      ? { protocol: 'ws', host: HOST, port: 1421 }
      : undefined,
    watch: { ignored: ['**/src-tauri/**'] },
  },
  envPrefix: ['VITE_', 'TAURI_'],
  test: {
    environment: 'jsdom',
    // Note: not `src/test/` — the repo's .gitignore has a bare `test` rule
    // that matches a directory of that name at any depth.
    setupFiles: ['./src/testing/setup.ts'],
    include: ['src/**/*.test.{ts,tsx}'],
  },
})
