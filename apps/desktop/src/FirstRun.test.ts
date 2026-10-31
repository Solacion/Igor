import { mount, flushPromises } from '@vue/test-utils'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import { afterEach, expect, it } from 'vitest'
import App from './App.vue'

afterEach(() => { clearMocks(); document.body.innerHTML = '' })

it.each([
  { providers: [], servers: [], link: 'Open Model providers', target: 'model-providers' },
  { providers: [], servers: [{ entry: { id: 'sales', command: '/fixture', args: [], env: {}, env_allowlist: [] }, status: { state: 'connected' }, tools: [], permissions: {} }], link: 'Open Model providers', target: 'model-providers' },
  { providers: [{ id: 'local', model: 'model', unavailable: null }], servers: [], link: 'Open Settings', target: 'mcp-servers' },
])('explains missing setup and focuses the matching section: $target', async ({ providers, servers, link, target }) => {
  mockIPC(command => {
    if (command === 'desktop_status') return { app_version: 'test', core_event_schema: 1 }
    if (command === 'conversation_providers') return { providers }
    if (command === 'settings_view') return { config_path: '/temporary/config.toml', servers }
    if (command === 'settings_providers') return { providers: [] }
    throw new Error(`unexpected ${command}`)
  })
  const wrapper = mount(App, { attachTo: document.body }); await flushPromises()
  if (!providers.length) expect(wrapper.text()).toContain('No model providers configured')
  if (!servers.length) expect(wrapper.text()).toContain('No MCP servers configured')
  expect(wrapper.text()).not.toContain('Could not load'); expect(wrapper.text()).not.toContain('restart Igor')
  expect(wrapper.find('#main [role=alert]').exists()).toBe(false)
  await wrapper.findAll('button').find(button => button.text() === link)!.trigger('click'); await flushPromises()
  expect(wrapper.get(`a[href="#${target}"]`).attributes('aria-current')).toBe('location')
  expect(document.activeElement?.id).toBe(target)
  expect(wrapper.get(`#${target}`).attributes('tabindex')).toBe('-1')
  wrapper.unmount()
})
