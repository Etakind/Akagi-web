import { beforeEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { createMemoryRouter, RouterProvider } from 'react-router-dom'
import { Settings } from './Settings'
import { Setup } from './Setup'
import { useConfigStore } from '@/stores/configStore'
import { appConfigFixture } from '@/testing/appConfigFixture'
import type { AppConfig } from '@/types'

const backend = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@/lib/tauri', () => ({ HAS_TAURI: true, invoke: backend.invoke }))
vi.mock('@/hooks/useTauriBridge', () => ({ useTauriBridge: () => {} }))
vi.mock('@tauri-apps/api/app', () => ({ getVersion: async () => '0.1.1' }))
vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: 'en', changeLanguage: vi.fn() } }),
}))

let config: AppConfig
function mount(path = '/settings') {
  const router = createMemoryRouter([
    { path: '/settings', element: <Settings /> },
    { path: '/setup', element: <Setup /> },
    { path: '/', element: <p>dashboard</p> },
  ], { initialEntries: [path] })
  render(<RouterProvider router={router} />)
  return router
}
function headerButton(name: string) {
  const heading = screen.getByRole('heading', { name: 'settings.title' })
  const header = heading.closest('header')!
  return [...header.querySelectorAll('button')].find(b => b.textContent === name)!
}

beforeEach(() => {
  config = appConfigFixture()
  useConfigStore.setState({ config })
  backend.invoke.mockReset().mockImplementation(async (command, args) => {
    if (command === 'get_config') return config
    if (command === 'update_config') { config = args.newConfig; return null }
    return []
  })
})

describe('Settings actions', () => {
  it('saves overlay changes while retaining the one-time upgrade revision', async () => {
    mount()
    const opacity = await screen.findByLabelText('settings.overlay_opacity')
    expect(headerButton('common.save').disabled).toBe(true)
    fireEvent.change(opacity, { target: { value: '0.7' } })
    expect(headerButton('common.save').disabled).toBe(false)
    fireEvent.click(headerButton('common.save'))
    await waitFor(() => expect(useConfigStore.getState().config?.overlay.opacity).toBe(0.7))
    expect(config.general.overlay_defaults_revision).toBe(1)
    await waitFor(() => expect(headerButton('common.save').disabled).toBe(true))
  })

  it('resets the draft without persisting changes', async () => {
    mount()
    const opacity = await screen.findByLabelText('settings.overlay_opacity')
    fireEvent.change(opacity, { target: { value: '0.6' } })
    fireEvent.click(headerButton('common.reset'))
    expect((opacity as HTMLInputElement).value).toBe('0.95')
    expect(backend.invoke.mock.calls.some(([command]) => command === 'update_config')).toBe(false)
    expect(headerButton('common.save').disabled).toBe(true)
  })

  it('still blocks navigation to the wizard when settings are unsaved', async () => {
    const router = mount()
    const opacity = await screen.findByLabelText('settings.overlay_opacity')
    fireEvent.change(opacity, { target: { value: '0.6' } })
    fireEvent.click(screen.getByRole('link', { name: 'settings.rerun_setup' }))
    expect(await screen.findByRole('dialog')).toBeTruthy()
    expect(router.state.location.pathname).toBe('/settings')
    expect(screen.getByText('settings.unsaved_title')).toBeTruthy()
  })
})

describe('Setup overlay defaults', () => {
  it('completes onboarding with an enabled overlay and retained migration revision', async () => {
    config.general.first_run_completed = false
    mount('/setup')
    const button = await screen.findByRole('button', { name: 'common.save' })
    fireEvent.click(button)
    await screen.findByText('dashboard')
    expect(config.overlay.enabled).toBe(true)
    expect(config.general.first_run_completed).toBe(true)
    expect(config.general.overlay_defaults_revision).toBe(1)
  })
})
