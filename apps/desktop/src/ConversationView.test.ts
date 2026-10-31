import { mount, flushPromises } from '@vue/test-utils'
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks'
import { Channel } from '@tauri-apps/api/core'
import { afterEach, expect, it, vi } from 'vitest'
import ConversationView from './ConversationView.vue'
import type { CoreEvent } from './session'

afterEach(() => { clearMocks(); vi.useRealTimers() })
function backend(options: { unavailable?: string; send?: () => unknown } = {}) {
  let channel: Channel<CoreEvent> | undefined
  let sequence = 0
  const calls: Array<{ command: string; args: any }> = []
  mockIPC((command, args: any) => {
    calls.push({ command, args })
    if (command === 'conversation_providers') return { providers: [{ id: 'local', model: 'demo', unavailable: options.unavailable ?? null }] }
    if (command === 'settings_view') return { config_path: '/config', servers: [] }
    if (command === 'start_session') { channel = args.events; sequence = 0; return { session_id: 'one', provider_id: 'local', model: 'demo', servers: [] } }
    if (command === 'send_message') return options.send?.() ?? { outcome: 'completed' }
  })
  return { calls, emit(event: CoreEvent['event']) { channel?.onmessage({ schema_version: 1, sequence: sequence++, event }) } }
}
async function start(wrapper: ReturnType<typeof mount>) {
  await flushPromises(); await wrapper.get('#message').setValue('Hello'); await wrapper.get('form').trigger('submit'); await flushPromises()
}
it('sends messages, renders model text literally and keeps a multi-turn session', async () => {
  const mock = backend(); const wrapper = mount(ConversationView)
  await start(wrapper)
  mock.emit({ event: 'assistant_text', text: '<img src=x onerror=alert(1)>' })
  await flushPromises()
  expect(wrapper.text()).toContain('Hello'); expect(wrapper.text()).toContain('<img src=x onerror=alert(1)>'); expect(wrapper.find('img').exists()).toBe(false)
  await wrapper.get('#message').setValue('Again'); await wrapper.get('form').trigger('submit'); await flushPromises()
  expect(mock.calls.filter(c => c.command === 'start_session')).toHaveLength(1)
  expect(mock.calls.filter(c => c.command === 'send_message').map(c => c.args.request.text)).toEqual(['Hello', 'Again'])
  expect(wrapper.text()).toContain('Completed'); wrapper.unmount()
})
it('shows missing environment variable and prevents sending; links empty servers to Settings', async () => {
  const mock = backend({ unavailable: 'Environment variable IGOR_API_KEY is missing.' }); const wrapper = mount(ConversationView)
  await start(wrapper)
  expect(wrapper.text()).toContain('IGOR_API_KEY'); expect(mock.calls.some(c => c.command === 'send_message')).toBe(false)
  await wrapper.get('.conversation-servers button').trigger('click'); expect(wrapper.emitted('settings')).toHaveLength(1); wrapper.unmount()
})
it('keeps cancellation available and reports incomplete outcomes', async () => {
  let done: (value: unknown) => void = () => {}
  const mock = backend({ send: () => new Promise(resolve => { done = resolve }) }); const wrapper = mount(ConversationView)
  await start(wrapper)
  expect(wrapper.get('#provider').attributes('disabled')).toBeDefined()
  await wrapper.get('.composer-actions button[type=button]').trigger('click'); await flushPromises()
  expect(mock.calls.some(c => c.command === 'cancel_turn')).toBe(true)
  done({ outcome: 'cancelled' }); await flushPromises(); expect(wrapper.text()).toContain('answer may be incomplete'); wrapper.unmount()
})
it('uses fixed IPC failure text without exposing native diagnostics', async () => {
  backend({ send: () => { throw new Error('secret-native-detail') } }); const wrapper = mount(ConversationView)
  await start(wrapper); expect(wrapper.text()).toContain('turn could not finish'); expect(wrapper.text()).not.toContain('secret-native-detail'); wrapper.unmount()
})
it('starts a fresh session after Settings change, explaining history reset before retry', async () => {
  let attempts = 0
  const mock = backend({ send: () => { if (++attempts === 2) throw { code: 'settings_changed' }; return { outcome: 'completed' } } })
  const wrapper = mount(ConversationView); await start(wrapper)
  await wrapper.get('#message').setValue('Updated'); await wrapper.get('form').trigger('submit'); await flushPromises()
  expect(wrapper.text()).toContain('Settings changed. A new model conversation')
  expect(mock.calls.filter(c => c.command === 'start_session')).toHaveLength(2)
  expect(mock.calls.filter(c => c.command === 'send_message')).toHaveLength(3)
  expect(wrapper.findAll('.messages li')).toHaveLength(2); wrapper.unmount()
})
