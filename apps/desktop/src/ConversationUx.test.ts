import { mount, flushPromises } from '@vue/test-utils'
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks'
import { afterEach, expect, it, vi } from 'vitest'
import type { Channel } from '@tauri-apps/api/core'
import type { CoreEvent } from './session'
import ConversationView from './ConversationView.vue'
import MessageList from './components/conversation/MessageList.vue'
import ApprovalCard from './components/conversation/ApprovalCard.vue'
import type { TimelineEntry } from './conversationDisplay'

afterEach(() => { clearMocks(); vi.restoreAllMocks() })
const request = { approval_id: 'a', call_id: 'c', tool: { server: 'sales', name: 'create_draft_order' }, canonical_arguments: '{"supplier_id":"supplier-x","lines":[{"sku":"<img>","quantity":9007199254740993,"rationale":"Low stock"}]}', action_hash: 'a'.repeat(64) }
function backend(pending = false) {
  let channel: Channel<CoreEvent> | undefined
  let sequence = 0
  const sends: string[] = []
  mockIPC((command, args: any) => {
    if (command === 'conversation_providers') return { providers: [{ id: 'local', model: 'demo', unavailable: null }] }
    if (command === 'settings_view') return { servers: [{ entry: { id: 'sales' }, status: { state: 'connected' } }] }
    if (command === 'start_session') { channel = args.events; return { session_id: 'one', provider_id: 'local', model: 'demo', servers: [] } }
    if (command === 'send_message') { sends.push(args.request.text); return pending ? new Promise(() => {}) : { outcome: 'completed' } }
  })
  return { sends, emit(event: unknown) { channel?.onmessage({ schema_version: 1, sequence: sequence++, event }) } }
}
async function send(wrapper: ReturnType<typeof mount>) {
  await flushPromises()
  await wrapper.get('#message').setValue('Hello')
  await wrapper.get('form').trigger('submit')
  await flushPromises()
}
it('keeps one chronology, compact tool rows and thinking state; focuses and announces inline actions', async () => {
  const mock = backend(true)
  const scroll = vi.spyOn(HTMLElement.prototype, 'scrollIntoView').mockImplementation(() => {})
  const wrapper = mount(ConversationView, { attachTo: document.body })
  await send(wrapper)
  mock.emit({ event: 'assistant_text', text: 'Checking now' })
  mock.emit({ event: 'tool_call_proposed', call_id: 'stock', tool: { server: 'sales', name: 'get_stock_levels' }, arguments: {} })
  mock.emit({ event: 'tool_call_completed', call_id: 'stock', is_error: false })
  mock.emit({ event: 'approval_requested', request })
  await flushPromises()
  expect(wrapper.text()).toContain('Checked stock · sales ✓')
  expect(wrapper.text()).toContain('Igor is thinking')
  expect(document.activeElement).toBe(wrapper.get('.approval-card button').element)
  expect(wrapper.get('.sr-only').text()).toContain('Review and decide on screen')
  mock.emit({ event: 'clarification_requested', request_id: 'q', question: 'Which day?' })
  await flushPromises()
  expect(wrapper.findAll('.timeline > li').map(row => row.attributes('data-kind'))).toEqual(['message', 'message', 'tool', 'approval', 'question'])
  expect(document.activeElement).toBe(wrapper.get('.clarifications textarea').element)
  expect(wrapper.get('.sr-only').text()).toBe('Which day?')
  expect(scroll).toHaveBeenCalled()
  expect(wrapper.find('.conversation-toolbar #provider').exists()).toBe(true)
  expect(wrapper.get('.conversation-servers .readiness-dot').classes()).toContain('is-ready')
  wrapper.unmount()
})
it('summarizes canonical order lines without rounding numbers or changing approval data', async () => {
  const wrapper = mount(ApprovalCard, { props: { busy: true, item: { key: 'a', request, state: 'pending' } } })
  expect(wrapper.get('.approval-summary').text()).toContain('supplier-x')
  expect(wrapper.get('tbody').text()).toContain('9007199254740993')
  expect(wrapper.get('tbody').text()).toContain('Low stock')
  expect(wrapper.find('img').exists()).toBe(false)
  expect(wrapper.get('details').attributes('open')).toBeUndefined()
  expect(wrapper.get('pre').text()).toBe(request.canonical_arguments)
  await wrapper.get('button').trigger('click')
  expect(wrapper.emitted('decide')).toEqual([['approve']])
  await wrapper.setProps({ item: { key: 'b', state: 'pending', request: { ...request, tool: { server: 'other', name: 'write' }, canonical_arguments: '{"path":"<b>literal</b>","count":2}' } } })
  expect(wrapper.get('dl').text()).toContain('<b>literal</b>')
  expect(wrapper.find('b').exists()).toBe(false)
  wrapper.unmount()
})
it('sends Enter, preserves Shift+Enter and composition, and confirms history reset', async () => {
  const mock = backend()
  const wrapper = mount(ConversationView)
  await flushPromises()
  await wrapper.get('#message').setValue('Keyboard message')
  await wrapper.get('#message').trigger('keydown', { key: 'Enter', shiftKey: true })
  await wrapper.get('#message').trigger('keydown', { key: 'Enter', isComposing: true })
  expect(mock.sends).toEqual([])
  await wrapper.get('#message').trigger('keydown', { key: 'Enter' })
  await flushPromises()
  expect(mock.sends).toEqual(['Keyboard message'])
  await wrapper.findAll('button').find(b => b.text() === 'New conversation')!.trigger('click')
  expect(wrapper.get('[role=alertdialog]').text()).toContain('Start a new conversation?')
  await wrapper.findAll('[role=alertdialog] button')[1]!.trigger('click')
  expect(wrapper.text()).toContain('Keyboard message')
  await wrapper.findAll('button').find(b => b.text() === 'New conversation')!.trigger('click')
  await wrapper.get('[role=alertdialog] button').trigger('click')
  await flushPromises()
  expect(wrapper.findAll('.timeline > li')).toHaveLength(0)
  wrapper.unmount()
})
it('follows content unless scrolled up, with an explicit jump back to the latest entry', async () => {
  const wrapper = mount(MessageList, { props: { entries: [], busy: false } })
  const list = wrapper.get('.timeline').element
  Object.defineProperties(list, { scrollHeight: { value: 1000, configurable: true }, clientHeight: { value: 100, configurable: true } })
  const entries: TimelineEntry[] = [{ kind: 'message', key: 'one', message: { role: 'Igor', text: 'First' } }]
  await wrapper.setProps({ entries })
  await flushPromises()
  expect(list.scrollTop).toBe(1000)
  list.scrollTop = 100
  await wrapper.get('.timeline').trigger('scroll')
  await wrapper.setProps({ entries: [...entries, { kind: 'message', key: 'two', message: { role: 'Igor', text: 'Second' } }] })
  await flushPromises()
  expect(list.scrollTop).toBe(100)
  await wrapper.get('.jump-latest').trigger('click')
  expect(list.scrollTop).toBe(1000)
  wrapper.unmount()
})
