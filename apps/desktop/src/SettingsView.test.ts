import { emitSettingsChanged } from './test/settingsEvents'
import { flushPromises, mount } from '@vue/test-utils'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import { afterEach, describe, expect, it, vi } from 'vitest'
import App from './App.vue'
import SettingsView from './SettingsView.vue'
import type { ServerEntry, ServerView, SettingsView as View } from './bridge'

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
    if (command === 'speech_languages') return []
    if (command === 'settings_providers') return { providers: [] }
    if (command === 'speech_status') return { installed: false, download_bytes: 0, default_voice: 'af_heart', voices: [] }
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
    changed = true; emitSettingsChanged(); await flushPromises(); await flushPromises()
    const row = wrapper.findAll('.tool-row').find(row => row.text().includes('new_tool'))!
    expect(row.exists()).toBe(true)
    expect(row.findAll('input:checked')).toHaveLength(0)
    expect(row.text()).toContain('Not allowed yet')
    wrapper.unmount()
  })

  it('opens from the conversation toolbar and closes with the back button', async () => {
    backend({
      desktop_status: () => ({ app_version: '0.1.0', core_event_schema: 1 }),
      settings_view: () => view(),
    })
    const wrapper = mount(App, { global: { stubs: { ConversationView: true } } })
    await flushPromises()
    wrapper.findComponent({ name: 'ConversationView' }).vm.$emit('settings')
    await flushPromises()
    expect(wrapper.get('h1').text()).toBe('Settings')
    expect(wrapper.text()).toContain('MCP servers')
    await wrapper.get('.settings-back').trigger('click')
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
    const rows = card.findAll('.tool-row')
    expect(rows[0]!.get('input:checked').attributes('value')).toBe('always')
    expect(rows[0]!.get('label.is-on').text()).toBe('Always allow')
    expect(rows[1]!.findAll('input:checked')).toHaveLength(0)
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

  it('sets tool permissions', async () => {
    const calls = backend({
      settings_view: () => view(),
      settings_set_permission: () => view(),
    })
    const wrapper = mount(SettingsView)
    await flushPromises()
    await wrapper.findAll('.tool-row')[1]!.get('input[value="ask"]').setValue(true)
    await flushPromises()
    await wrapper.findAll('.tool-row')[0]!.get('input[value="never"]').setValue(true)
    await flushPromises()
    const sets = calls.filter((call) => call.command === 'settings_set_permission').map((call) => call.args)
    expect(sets).toEqual([
      { request: { server: 'pos', tool: 'create_order', class: 'write' } },
      { request: { server: 'pos', tool: 'get_sales', class: 'denied' } },
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
          id: 'pos', transport: 'stdio', command: '/opt/pos-mcp', args: ['--store', 'main'], cwd: null, env_allowlist: [],
          env: { POS_TOKEN: 'IGOR_POS_TOKEN' }, startup_timeout_ms: null, call_timeout_ms: null,
        },
        secret: null,
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
    expect(calls.filter(call => call.command !== 'settings_providers' && call.command !== 'speech_status' && call.command !== 'speech_languages' && call.command !== 'transcription_status').map((call) => call.command)).toEqual(['settings_view'])
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

  it('refreshes status on settings changes and stops when closed', async () => {
    vi.useFakeTimers()
    let reads = 0
    backend({ settings_view: () => { reads++; return view() } })
    const wrapper = mount(SettingsView)
    await flushPromises()
    emitSettingsChanged(); await flushPromises()
    expect(reads).toBe(2)
    wrapper.unmount()
    emitSettingsChanged(); await flushPromises()
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
    lines = []; emitSettingsChanged(); await flushPromises(); await flushPromises()
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

it('filters a long tool list by name or description', async () => {
  const many = Array.from({ length: 8 }, (_, i) => ({ name: `tool_${i}`, description: i === 3 ? 'Stock on hand' : null, read_only_hint: true, destructive_hint: null }))
  backend({ settings_view: () => view([{ ...pos, tools: many }]) })
  const wrapper = mount(SettingsView)
  await flushPromises()
  const search = wrapper.get('.tool-search input')
  await search.setValue('stock')
  expect(wrapper.findAll('.tool-row').map(row => row.get('strong').text())).toEqual(['tool_3'])
  expect(wrapper.get('.tool-search').text()).toContain('1 of 9')
  await search.setValue('tool 5')
  expect(wrapper.findAll('.tool-row').map(row => row.get('strong').text())).toEqual(['tool_5'])
  await search.setValue('nothing')
  expect(wrapper.findAll('.tool-row')).toHaveLength(0)
  expect(wrapper.text()).toContain('No tools match.')
  wrapper.unmount()
})

it('adds a remote server with its key in the Keychain and edits it without retyping the key', async () => {
  const remote: ServerView = { ...pos, entry: { ...pos.entry, transport: 'http', url: 'https://mcp.example.com/mcp', command: '', args: [], bearer_token: { keychain: 'mcp-crm' }, headers: { 'X-Tenant': { env: 'IGOR_TENANT' } }, id: 'crm' } }
  const calls = backend({ settings_view: () => view([]), settings_save_server: () => view([remote]) })
  const wrapper = mount(SettingsView)
  await flushPromises()
  await wrapper.get('#mcp-servers .section-head button').trigger('click')
  await wrapper.get('input[value="http"]').setValue(true)
  await wrapper.get('input[name="id"]').setValue('crm')
  await wrapper.get('input[name="url"]').setValue('https://mcp.example.com/mcp')
  await wrapper.get('select[name="auth"]').setValue('bearer')
  await wrapper.get('form').trigger('submit')
  expect(wrapper.text()).toContain('Enter the key.')
  await wrapper.get('input[name="key"]').setValue('tok-ui-sentinel')
  await wrapper.get('form').trigger('submit')
  const confirm = wrapper.get('[role="alertdialog"]')
  expect(confirm.text()).toContain('Connect to this server?')
  expect(confirm.get('code').text()).toBe('https://mcp.example.com/mcp')
  await confirm.get('button').trigger('click')
  await flushPromises()
  expect(calls.find(call => call.command === 'settings_save_server')?.args).toEqual({
    request: {
      replacing: null,
      entry: {
        id: 'crm', transport: 'http', url: 'https://mcp.example.com/mcp', headers: {}, auth: null, bearer_token: { keychain: 'mcp-crm' },
        command: '', args: [], cwd: null, env_allowlist: [], env: {}, startup_timeout_ms: null, call_timeout_ms: null,
      },
      secret: 'tok-ui-sentinel',
    },
  })
  // The card shows the URL; editing keeps the saved key and the hand-written header.
  const card = wrapper.get('.server-card')
  expect(card.get('.command-line').text()).toBe('https://mcp.example.com/mcp')
  expect(wrapper.text()).not.toContain('tok-ui-sentinel')
  await card.findAll('button').find(b => b.text() === 'Edit')!.trigger('click')
  expect(wrapper.get('input[name="key"]').attributes('placeholder')).toContain('Saved in Keychain')
  await wrapper.get('form').trigger('submit')
  await flushPromises()
  const edit = calls.filter(call => call.command === 'settings_save_server').at(-1)?.args as { request: { entry: ServerEntry, secret: string | null } }
  expect(edit.request.secret).toBeNull()
  expect(edit.request.entry.bearer_token).toEqual({ keychain: 'mcp-crm' })
  expect(edit.request.entry.headers).toEqual({ 'X-Tenant': { env: 'IGOR_TENANT' } })
  wrapper.unmount()
})

it('writes allow only for tools the server says change data, and shows read on such a tool as Ask first', async () => {
  const start = view([{ ...pos, permissions: { create_order: 'read' } }])
  const calls = backend({ settings_view: () => start, settings_set_permission: () => start })
  const wrapper = mount(SettingsView)
  await flushPromises()
  const rows = wrapper.findAll('.tool-row')
  expect(rows[1]!.get('input:checked').attributes('value')).toBe('ask')
  expect(rows[1]!.text()).toContain('choose Always allow to stop asking')
  await rows[1]!.get('input[value="always"]').setValue(true)
  await flushPromises()
  await wrapper.findAll('.tool-row')[0]!.get('input[value="always"]').setValue(true)
  await flushPromises()
  expect(calls.filter(call => call.command === 'settings_set_permission').map(call => call.args)).toEqual([
    { request: { server: 'pos', tool: 'create_order', class: 'allow' } },
    { request: { server: 'pos', tool: 'get_sales', class: 'read' } },
  ])
  wrapper.unmount()
})

it('signs in to an OAuth server from its card and offers sign-out once connected', async () => {
  const oauth: ServerView = { ...pos, status: { state: 'sign_in_needed' }, tools: [], permissions: {}, entry: { ...pos.entry, id: 'crm', transport: 'http', url: 'https://mcp.example.com/mcp', command: '', args: [], auth: 'oauth' } }
  const calls = backend({
    settings_view: () => view([oauth]),
    settings_sign_in_server: () => view([{ ...oauth, status: { state: 'connected' } }]),
  })
  const wrapper = mount(SettingsView)
  await flushPromises()
  const card = wrapper.get('.server-card')
  expect(card.get('.status').text()).toBe('Sign-in needed')
  expect(card.findAll('button').map(b => b.text())).not.toContain('Sign out')
  await card.findAll('button').find(b => b.text() === 'Sign in')!.trigger('click')
  await flushPromises()
  expect(calls.find(call => call.command === 'settings_sign_in_server')?.args).toEqual({ request: { id: 'crm' } })
  const buttons = wrapper.get('.server-card').findAll('button').map(b => b.text())
  expect(buttons).toContain('Sign out')
  expect(buttons).not.toContain('Sign in')
  wrapper.unmount()
})

