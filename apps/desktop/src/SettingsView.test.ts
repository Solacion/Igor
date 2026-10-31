import { flushPromises, mount } from '@vue/test-utils'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import { afterEach, describe, expect, it, vi } from 'vitest'
import App from './App.vue'
import SettingsView from './SettingsView.vue'
import type { ServerView, SettingsView as View } from './bridge'

afterEach(() => {
  clearMocks()
  vi.useRealTimers()
})

const pos: ServerView = {
  entry: {
    id: 'pos', command: '/opt/pos-mcp', args: ['--store', 'main'], cwd: null,
    env_allowlist: [], env: {}, startup_timeout_ms: null, call_timeout_ms: null,
  },
  status: { state: 'connected' },
  tools: [
    { name: 'get_sales', description: 'Sales <b>per day</b>', read_only_hint: true, destructive_hint: null },
    { name: 'create_order', description: null, read_only_hint: false, destructive_hint: true },
  ],
  permissions: { get_sales: 'read' },
}

function view(servers: ServerView[] = [pos]): View {
  return { config_path: '/Users/op/igor/config.toml', servers }
}

type Calls = { command: string; args: unknown }[]

function backend(handlers: Record<string, (args: any) => unknown>): Calls {
  const calls: Calls = []
  mockIPC((command, args) => {
    calls.push({ command, args })
    if (command === 'settings_providers') return { providers: [] }
    const handler = handlers[command]
    if (!handler) throw new Error(`unexpected ${command}`)
    return handler(args)
  })
  return calls
}

describe('settings', () => {
  it('shows newly discovered tools on refresh with permission still blocked', async () => {
    vi.useFakeTimers()
    let changed = false
    backend({ settings_view: () => view([{ ...pos, tools: changed ? [...pos.tools, { name: 'new_tool', description: null, read_only_hint: true, destructive_hint: null }] : pos.tools }]) })
    const wrapper = mount(SettingsView); await flushPromises()
    changed = true; await vi.advanceTimersByTimeAsync(3000); await flushPromises()
    const row = wrapper.findAll('tbody tr').find(row => row.text().includes('new_tool'))!
    expect(row.exists()).toBe(true)
    expect((row.get('select').element as HTMLSelectElement).value).toBe('')
    wrapper.unmount()
  })

  it('opens from the top bar and closes with Done', async () => {
    backend({
      desktop_status: () => ({ app_version: '0.1.0', core_event_schema: 1 }),
      settings_view: () => view(),
    })
    const wrapper = mount(App, { global: { stubs: { ConversationView: true } } })
    await flushPromises()
    await wrapper.get('.topbar button').trigger('click')
    await flushPromises()
    expect(wrapper.get('h1').text()).toBe('Settings')
    expect(wrapper.text()).toContain('MCP servers')
    await wrapper.get('.settings-header button').trigger('click')
    expect(wrapper.find('.settings').exists()).toBe(false)
    wrapper.unmount()
  })

  it('lists servers with status, command and tool permissions, rendering server text literally', async () => {
    backend({ settings_view: () => view() })
    const wrapper = mount(SettingsView)
    await flushPromises()
    const card = wrapper.get('.server-card')
    expect(card.text()).toContain('pos')
    expect(card.text()).toContain('/opt/pos-mcp --store main')
    expect(card.get('.status').text()).toBe('Connected')
    expect(card.text()).toContain('Sales <b>per day</b>')
    expect(card.find('b').exists()).toBe(false)
    expect(card.text()).toContain('Server says: may change or delete data.')
    expect((card.get('tbody tr:nth-child(1) select').element as HTMLSelectElement).value).toBe('read')
    expect((card.get('tbody tr:nth-child(2) select').element as HTMLSelectElement).value).toBe('')
    expect(wrapper.text()).toContain('Saved in /Users/op/igor/config.toml')
    wrapper.unmount()
  })

  it('shows a failed connection with its reason', async () => {
    backend({ settings_view: () => view([{ ...pos, status: { state: 'failed', detail: 'cannot start /opt/pos-mcp' }, tools: [] }]) })
    const wrapper = mount(SettingsView)
    await flushPromises()
    expect(wrapper.get('.status').text()).toBe('Not connected')
    expect(wrapper.get('.status-detail').text()).toBe('cannot start /opt/pos-mcp')
    expect(wrapper.text()).toContain('Tools and permissions (1)')
    expect(wrapper.text()).toContain('Not reported by the server right now.')
    wrapper.unmount()
  })

  it('sets and clears a tool permission', async () => {
    const calls = backend({
      settings_view: () => view(),
      settings_set_permission: () => view(),
    })
    const wrapper = mount(SettingsView)
    await flushPromises()
    await wrapper.get('tbody tr:nth-child(2) select').setValue('write')
    await flushPromises()
    await wrapper.get('tbody tr:nth-child(1) select').setValue('')
    await flushPromises()
    const sets = calls.filter((call) => call.command === 'settings_set_permission').map((call) => call.args)
    expect(sets).toEqual([
      { request: { server: 'pos', tool: 'create_order', class: 'write' } },
      { request: { server: 'pos', tool: 'get_sales', class: null } },
    ])
    wrapper.unmount()
  })

  it('adds a server only after confirming the program it runs', async () => {
    const calls = backend({
      settings_view: () => view([]),
      settings_save_server: () => view(),
    })
    const wrapper = mount(SettingsView)
    await flushPromises()
    expect(wrapper.text()).toContain('No servers yet')
    await wrapper.get('#mcp-servers .section-head button').trigger('click')
    await wrapper.get('input[name="id"]').setValue('pos')
    await wrapper.get('input[name="command"]').setValue('/opt/pos-mcp')
    await wrapper.get('textarea[name="args"]').setValue('--store\nmain\n')
    await wrapper.get('textarea[name="env"]').setValue('POS_TOKEN = IGOR_POS_TOKEN')
    await wrapper.get('form').trigger('submit')
    expect(calls.some((call) => call.command === 'settings_save_server')).toBe(false)
    const confirm = wrapper.get('[role="alertdialog"]')
    expect(confirm.get('code').text()).toBe('/opt/pos-mcp --store main')
    await confirm.get('button').trigger('click')
    await flushPromises()
    const save = calls.find((call) => call.command === 'settings_save_server')
    expect(save?.args).toEqual({
      request: {
        replacing: null,
        entry: {
          id: 'pos', command: '/opt/pos-mcp', args: ['--store', 'main'], cwd: null, env_allowlist: [],
          env: { POS_TOKEN: 'IGOR_POS_TOKEN' }, startup_timeout_ms: null, call_timeout_ms: null,
        },
      },
    })
    expect(wrapper.find('form').exists()).toBe(false)
    expect(wrapper.get('.server-card').text()).toContain('pos')
    wrapper.unmount()
  })

  it('shows a validation error next to its field', async () => {
    backend({
      settings_view: () => view([]),
      settings_test_server: () => { throw { field: 'id', message: 'invalid server id "Bad Id"' } },
    })
    const wrapper = mount(SettingsView)
    await flushPromises()
    await wrapper.get('#mcp-servers .section-head button').trigger('click')
    await wrapper.get('input[name="id"]').setValue('Bad Id')
    await wrapper.get('input[name="command"]').setValue('./x')
    await wrapper.findAll('button').find((b) => b.text() === 'Test connection')?.trigger('click')
    await flushPromises()
    const id = wrapper.get('input[name="id"]')
    expect(id.attributes('aria-invalid')).toBe('true')
    expect(id.element.parentElement?.textContent).toContain('invalid server id "Bad Id"')
    wrapper.unmount()
  })

  it('rejects malformed secret lines before calling the backend', async () => {
    const calls = backend({ settings_view: () => view([]) })
    const wrapper = mount(SettingsView)
    await flushPromises()
    await wrapper.get('#mcp-servers .section-head button').trigger('click')
    await wrapper.get('input[name="id"]').setValue('pos')
    await wrapper.get('input[name="command"]').setValue('./x')
    await wrapper.get('textarea[name="env"]').setValue('just-a-value')
    await wrapper.get('form').trigger('submit')
    expect(wrapper.text()).toContain('Write one NAME=IGOR_VARIABLE per line.')
    expect(calls.filter(call => call.command !== 'settings_providers').map((call) => call.command)).toEqual(['settings_view'])
    wrapper.unmount()
  })

  it('tests a connection and lists the tools found without saving', async () => {
    const calls = backend({
      settings_view: () => view([]),
      settings_test_server: () => pos.tools,
    })
    const wrapper = mount(SettingsView)
    await flushPromises()
    await wrapper.get('#mcp-servers .section-head button').trigger('click')
    await wrapper.get('input[name="id"]').setValue('pos')
    await wrapper.get('input[name="command"]').setValue('/opt/pos-mcp')
    await wrapper.findAll('button').find((b) => b.text() === 'Test connection')?.trigger('click')
    await flushPromises()
    expect(wrapper.get('.tested').text()).toContain('2 tools found')
    expect(calls.some((call) => call.command === 'settings_save_server')).toBe(false)
    wrapper.unmount()
  })

  it('removes a server after confirmation', async () => {
    const calls = backend({
      settings_view: () => view(),
      settings_remove_server: () => view([]),
    })
    const wrapper = mount(SettingsView)
    await flushPromises()
    await wrapper.findAll('.server-card button').find((b) => b.text() === 'Remove')?.trigger('click')
    expect(calls.some((call) => call.command === 'settings_remove_server')).toBe(false)
    await wrapper.get('[role="alertdialog"] button').trigger('click')
    await flushPromises()
    expect(calls.find((call) => call.command === 'settings_remove_server')?.args).toEqual({ request: { id: 'pos' } })
    expect(wrapper.text()).toContain('No servers yet')
    wrapper.unmount()
  })

  it('refreshes status while open and stops when closed', async () => {
    vi.useFakeTimers()
    let reads = 0
    backend({ settings_view: () => { reads++; return view() } })
    const wrapper = mount(SettingsView)
    await flushPromises()
    await vi.advanceTimersByTimeAsync(3000)
    expect(reads).toBe(2)
    wrapper.unmount()
    await vi.advanceTimersByTimeAsync(9000)
    expect(reads).toBe(2)
  })
})

for (const state of ['connected', 'failed'] as const) {
  it(`renders ${state} server output as collapsed plain text and refreshes it`, async () => {
    vi.useFakeTimers()
    let lines = ['<img src=x onerror=alert(1)>', 'token=[REDACTED]']
    backend({ settings_view: () => view([{ ...pos, status: state === 'failed' ? { state, detail: 'Stopped' } : { state }, recent_stderr: lines }]) })
    const wrapper = mount(SettingsView); await flushPromises()
    const output = wrapper.get('.server-output')
    expect(output.get('summary').text()).toBe('Server output (last lines)')
    expect(output.attributes('open')).toBeUndefined()
    expect(output.get('pre').text()).toBe(lines.join('\n'))
    expect(output.find('img').exists()).toBe(false)
    expect(output.find('a').exists()).toBe(false)
    lines = []; await vi.advanceTimersByTimeAsync(3000); await flushPromises()
    expect(wrapper.find('.server-output').exists()).toBe(false)
    wrapper.unmount()
  })
}

it('does not offer process output before a server has started', async () => {
  backend({ settings_view: () => view([{ ...pos, status: { state: 'not_connected' } }, { ...pos, entry: { ...pos.entry, id: 'starting' }, status: { state: 'starting' } }]) })
  const wrapper = mount(SettingsView); await flushPromises()
  expect(wrapper.find('.server-output').exists()).toBe(false)
  wrapper.unmount()
})
