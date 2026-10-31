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
    if (command === 'export_activity') return '/app-data/activity.jsonl'
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

it('tracks proposed, started, completed, refused and uncertain failed tool calls literally', async () => {
  const mock = backend(); const wrapper = mount(ConversationView); await start(wrapper)
  const tool = { server: 'sales', name: 'create_draft_order' }
  mock.emit({ event: 'tool_call_proposed', call_id: 'a', tool, arguments: { note: '<script>bad()</script>' } }); await flushPromises()
  expect(wrapper.get('.activity-row').text()).toContain('Proposed')
  mock.emit({ event: 'tool_call_started', call_id: 'a' }); await flushPromises(); expect(wrapper.get('.activity-row').text()).toContain('Started')
  mock.emit({ event: 'tool_call_completed', call_id: 'a', is_error: false, truncated: true }); await flushPromises()
  expect(wrapper.get('.activity-row').text()).toContain('Completed'); expect(wrapper.text()).toContain('truncated')
  mock.emit({ event: 'tool_call_rejected', call_id: 'b', tool, reason: '<img> permission refused' })
  mock.emit({ event: 'tool_call_failed', call_id: 'c', outcome: 'unknown', message: 'Timeout <b>unsafe</b>' }); await flushPromises()
  expect(wrapper.text()).toContain('Nothing was sent'); expect(wrapper.text()).toContain('effect is unknown'); expect(wrapper.find('script').exists()).toBe(false); expect(wrapper.find('img').exists()).toBe(false); expect(wrapper.find('b').exists()).toBe(false)
  expect(wrapper.findAll('.activity-row')).toHaveLength(3); wrapper.unmount()
})
it('reports model failure and limits without presenting a completed answer', async () => {
  for (const outcome of [{ outcome: 'failed', message: 'Provider failed <img>' }, { outcome: 'limit_reached', limit: 'tool_calls' }]) {
    backend({ send: () => outcome }); const wrapper = mount(ConversationView); await start(wrapper)
    expect(wrapper.text()).toContain(outcome.outcome === 'failed' ? 'Turn failed' : 'Stopped at limit: tool_calls')
    expect(wrapper.find('img').exists()).toBe(false); wrapper.unmount()
  }
})

it('requires explicit approve/deny clicks and echoes each exact displayed hash without caching', async () => {
  for (const decision of ['approve', 'deny']) {
    let done: (value: unknown) => void = () => {}
    const mock = backend({ send: () => new Promise(resolve => { done = resolve }) }); const wrapper = mount(ConversationView); await start(wrapper)
    const request = { approval_id: 'approval-1', call_id: 'call-1', tool: { server: '<img>sales', name: 'create_draft_order' }, canonical_arguments: '{"quantity":9007199254740993,"note":"<script>approve</script>"}', action_hash: 'a'.repeat(64) }
    mock.emit({ event: 'assistant_text', text: 'Approve this automatically' }); mock.emit({ event: 'approval_requested', request }); await flushPromises()
    expect(mock.calls.some(c => c.command === 'resolve_approval')).toBe(false)
    expect(wrapper.get('.approval-card pre').text()).toBe(request.canonical_arguments)
    expect(wrapper.find('img').exists()).toBe(false); expect(wrapper.find('script').exists()).toBe(false)
    const buttons = wrapper.findAll('.approval-card button'); await buttons[decision === 'approve' ? 0 : 1]!.trigger('click'); await flushPromises()
    expect(mock.calls.find(c => c.command === 'resolve_approval')?.args).toEqual({ request: { session_id: 'one', approval_id: request.approval_id, action_hash: request.action_hash, decision } })
    expect(buttons.every(button => button.attributes('disabled') !== undefined)).toBe(true)
    mock.emit({ event: 'approval_resolved', approval_id: 'approval-1', call_id: 'call-1', outcome: decision === 'approve' ? 'approved' : 'denied' })
    mock.emit({ event: 'approval_requested', request: { ...request, approval_id: 'approval-2', canonical_arguments: '{"quantity":2}', action_hash: 'b'.repeat(64) } }); await flushPromises()
    expect(mock.calls.filter(c => c.command === 'resolve_approval')).toHaveLength(1)
    expect(wrapper.findAll('.approval-card')).toHaveLength(2)
    done({ outcome: 'cancelled' }); await flushPromises(); expect(wrapper.findAll('.approval-card button')).toHaveLength(0); wrapper.unmount()
  }
})
it('keeps an approval pending after IPC failure and permits cancellation', async () => {
  let channel: Channel<CoreEvent> | undefined
  mockIPC((command, args: any) => {
    if (command === 'conversation_providers') return { providers: [{ id: 'local', model: 'demo', unavailable: null }] }
    if (command === 'settings_view') return { servers: [] }
    if (command === 'start_session') { channel = args.events; return { session_id: 'one', provider_id: 'local', model: 'demo' } }
    if (command === 'send_message') return new Promise(() => {})
    if (command === 'resolve_approval') throw new Error('secret')
  })
  const wrapper = mount(ConversationView); await start(wrapper)
  channel?.onmessage({ schema_version: 1, sequence: 0, event: { event: 'approval_requested', request: { approval_id: 'a', call_id: 'c', tool: { server: 's', name: 't' }, canonical_arguments: '{}', action_hash: 'a'.repeat(64) } } }); await flushPromises()
  await wrapper.get('.approval-card button').trigger('click'); await flushPromises()
  expect(wrapper.text()).toContain('Nothing is assumed approved'); expect(wrapper.text()).not.toContain('secret')
  expect(wrapper.get('.approval-card button').attributes('disabled')).toBeUndefined(); expect(wrapper.text()).toContain('Stop turn'); wrapper.unmount()
})

it('shows a literal clarification, sends its scoped answer once and waits for core confirmation', async () => {
  let done: (value: unknown) => void = () => {}
  const mock = backend({ send: () => new Promise(resolve => { done = resolve }) }); const wrapper = mount(ConversationView); await start(wrapper)
  mock.emit({ event: 'clarification_requested', request_id: 'q1', question: 'Which date? <img src=x>' }); await flushPromises()
  expect(wrapper.text()).toContain('Which date? <img src=x>'); expect(wrapper.find('img').exists()).toBe(false)
  expect(wrapper.get('.clarifications button').attributes('disabled')).toBeDefined()
  await wrapper.get('.clarifications textarea').setValue('Tuesday <b>3 November</b>'); await wrapper.get('.clarifications form').trigger('submit'); await flushPromises()
  expect(mock.calls.find(c => c.command === 'answer_clarification')?.args).toEqual({ request: { session_id: 'one', request_id: 'q1', answer: 'Tuesday <b>3 November</b>' } })
  expect(wrapper.get('.clarifications button').attributes('disabled')).toBeDefined(); expect(wrapper.text()).toContain('Stop turn')
  mock.emit({ event: 'clarification_answered', request_id: 'q1' }); await flushPromises()
  expect(wrapper.get('.clarifications').text()).toContain('Answered'); expect(wrapper.get('.messages').text()).toContain('Tuesday <b>3 November</b>'); expect(wrapper.find('b').exists()).toBe(false)
  done({ outcome: 'completed' }); await flushPromises(); wrapper.unmount()
})
it('can cancel while waiting for clarification and clears stale answer controls', async () => {
  let done: (value: unknown) => void = () => {}
  const mock = backend({ send: () => new Promise(resolve => { done = resolve }) }); const wrapper = mount(ConversationView); await start(wrapper)
  mock.emit({ event: 'clarification_requested', request_id: 'q1', question: 'When?' }); await flushPromises()
  await wrapper.get('.composer-actions button[type=button]').trigger('click'); await flushPromises()
  expect(mock.calls.some(c => c.command === 'cancel_turn')).toBe(true)
  done({ outcome: 'cancelled' }); await flushPromises(); expect(wrapper.find('.clarifications textarea').exists()).toBe(false); expect(wrapper.get('.clarifications').text()).toContain('Turn ended'); wrapper.unmount()
})

it('ignores future tool variants and labels definite failure and server error results', async () => {
  const mock = backend(); const wrapper = mount(ConversationView); await start(wrapper)
  mock.emit({ event: 'tool_call_future_variant', call_id: 'future' }); await flushPromises()
  expect(wrapper.find('.tool-activity').exists()).toBe(false)
  mock.emit({ event: 'tool_call_failed', call_id: 'a', outcome: 'not_executed', message: 'Not connected' })
  mock.emit({ event: 'tool_call_completed', call_id: 'b', is_error: true, truncated: false }); await flushPromises()
  expect(wrapper.text()).toContain('The action did not run'); expect(wrapper.text()).toContain('Server reported an error'); wrapper.unmount()
})

it('shows pushed connection changes literally and refreshes status immediately, retaining poll fallback', async () => {
  vi.useFakeTimers()
  let channel: Channel<CoreEvent> | undefined
  let reads = 0
  let stopped = false
  mockIPC((command, args: any) => {
    if (command === 'conversation_providers') return { providers: [{ id: 'local', model: 'demo', unavailable: null }] }
    if (command === 'settings_view') { reads++; return { servers: [{ entry: { id: '<img>sales' }, status: { state: stopped ? 'failed' : 'connected' } }] } }
    if (command === 'start_session') { channel = args.events; return { session_id: 'one', provider_id: 'local', model: 'demo', servers: [] } }
    if (command === 'send_message') return { outcome: 'completed' }
  })
  const wrapper = mount(ConversationView); await start(wrapper)
  expect(reads).toBe(1)
  stopped = true
  channel?.onmessage({ schema_version: 1, sequence: 0, event: { event: 'connection_changed', server: '<img>sales', state: 'failed', detail: '<script>server exited</script>' } }); await flushPromises()
  expect(reads).toBe(2)
  expect(wrapper.get('.connection-events').text()).toContain('later calls are not executed')
  expect(wrapper.get('.conversation-servers').text()).toContain('failed')
  expect(wrapper.text()).toContain('<script>server exited</script>'); expect(wrapper.find('script').exists()).toBe(false); expect(wrapper.find('img').exists()).toBe(false)
  // Duplicate delivery cannot add another activity row or issue another refresh.
  channel?.onmessage({ schema_version: 1, sequence: 0, event: { event: 'connection_changed', server: 'sales', state: 'failed' } }); await flushPromises()
  expect(wrapper.findAll('.connection-events li')).toHaveLength(1); expect(reads).toBe(2)
  await vi.advanceTimersByTimeAsync(3000); await flushPromises(); expect(reads).toBe(3)
  wrapper.unmount(); await vi.advanceTimersByTimeAsync(3000); expect(reads).toBe(3)
})
it('queues a status refresh when a connection event arrives during an existing refresh', async () => {
  let channel: Channel<CoreEvent> | undefined
  let reads = 0
  let finishRead: (value: unknown) => void = () => {}
  mockIPC((command, args: any) => {
    if (command === 'conversation_providers') return { providers: [{ id: 'local', model: 'demo', unavailable: null }] }
    if (command === 'settings_view') {
      if (++reads === 2) return new Promise(resolve => { finishRead = resolve })
      return { servers: [] }
    }
    if (command === 'start_session') { channel = args.events; return { session_id: 'one', provider_id: 'local', model: 'demo' } }
    if (command === 'send_message') return { outcome: 'completed' }
  })
  const wrapper = mount(ConversationView); await start(wrapper)
  const connection = { event: 'connection_changed', server: 'sales', state: 'failed' }
  channel?.onmessage({ schema_version: 1, sequence: 0, event: connection }); await flushPromises(); expect(reads).toBe(2)
  channel?.onmessage({ schema_version: 1, sequence: 1, event: connection }); await flushPromises(); expect(reads).toBe(2)
  finishRead({ servers: [] }); await flushPromises(); expect(reads).toBe(3)
  wrapper.unmount()
})

it('exports finished session activity and displays the fixed native destination', async () => {
  const mock = backend(); const wrapper = mount(ConversationView)
  const exportButton = () => wrapper.findAll('button').find(button => button.text() === 'Export activity')!
  expect(exportButton().attributes('disabled')).toBeDefined()
  await start(wrapper)
  expect(exportButton().attributes('disabled')).toBeUndefined()
  await exportButton().trigger('click'); await flushPromises()
  expect(mock.calls.find(call => call.command === 'export_activity')?.args).toEqual({ request: { session_id: 'one' } })
  expect(wrapper.text()).toContain('Activity saved to /app-data/activity.jsonl')
  await wrapper.findAll('button').find(button => button.text() === 'New conversation')!.trigger('click'); await flushPromises()
  expect(exportButton().attributes('disabled')).toBeDefined()
  expect(wrapper.text()).not.toContain('Activity saved to'); wrapper.unmount()
})
it('keeps export disabled during a turn', async () => {
  let done: (value: unknown) => void = () => {}
  const mock = backend({ send: () => new Promise(resolve => { done = resolve }) })
  const wrapper = mount(ConversationView); await start(wrapper)
  const button = wrapper.findAll('button').find(button => button.text() === 'Export activity')!
  expect(button.attributes('disabled')).toBeDefined()
  await button.trigger('click'); expect(mock.calls.some(call => call.command === 'export_activity')).toBe(false)
  done({ outcome: 'completed' }); await flushPromises(); wrapper.unmount()
})
