import { flushPromises, mount } from '@vue/test-utils'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import type { Channel } from '@tauri-apps/api/core'
import { afterEach, expect, it } from 'vitest'
import ApprovalCard from './components/conversation/ApprovalCard.vue'
import ConversationView from './ConversationView.vue'
import type { CoreEvent } from './session'
import { parseEvent } from './events'

const request = { approval_id: 'approval', call_id: 'call', tool: { server: 'sales', name: 'write' }, canonical_arguments: '{}', action_hash: 'a'.repeat(64) }
afterEach(clearMocks)

it.each([undefined, false, true])('offers the conversation grant only when explicitly allowed (%s)', async allowed => {
  const item = { key: 'a', state: 'pending', request: { ...request, session_grant_allowed: allowed } }
  const wrapper = mount(ApprovalCard, { props: { item, busy: true } })
  const button = wrapper.findAll('button').find(button => button.text() === 'Allow for this conversation')
  expect(!!button).toBe(allowed === true)
  if (button) {
    await button.trigger('click')
    expect(wrapper.emitted('decide')).toEqual([['approve_for_session']])
    await wrapper.setProps({ item: { ...item, state: 'sending' } })
    expect(wrapper.findAll('button').every(button => button.attributes('disabled') !== undefined)).toBe(true)
  }
  wrapper.unmount()
})

it('sends the exact approval/hash, renders grant events as text, and starts a new native session after reset', async () => {
  let channel: Channel<CoreEvent> | undefined
  let sequence = 0
  let sessions = 0
  let complete: ((value: { outcome: string }) => void) | undefined
  const decisions: unknown[] = []
  const closed: string[] = []
  mockIPC((command, args: any) => {
    if (command === 'conversation_providers') return { providers: [{ id: 'local', model: 'demo', unavailable: null }] }
    if (command === 'settings_view') return { servers: [] }
    if (command === 'start_session') {
      channel = args.events
      sequence = 0
      return { session_id: `session-${++sessions}`, provider_id: 'local', model: 'demo', servers: [] }
    }
    if (command === 'send_message') return new Promise(resolve => { complete = resolve })
    if (command === 'resolve_approval') decisions.push(args.request)
    if (command === 'close_session') closed.push(args.request.session_id)
  })
  function emit(event: unknown) {
    channel?.onmessage({ schema_version: 1, sequence: sequence++, event })
  }
  const wrapper = mount(ConversationView)
  await flushPromises()
  await wrapper.get('#message').setValue('Do it')
  await wrapper.get('form').trigger('submit')
  await flushPromises()
  emit({ event: 'approval_requested', request: { ...request, session_grant_allowed: true } })
  await flushPromises()
  await wrapper.findAll('button').find(button => button.text() === 'Allow for this conversation')!.trigger('click')
  await flushPromises()
  expect(decisions).toEqual([{ session_id: 'session-1', approval_id: request.approval_id, action_hash: request.action_hash, decision: 'approve_for_session' }])
  emit({ event: 'approval_resolved', approval_id: request.approval_id, call_id: request.call_id, outcome: 'approved_for_session' })
  emit({ event: 'approved_for_session', call_id: 'later', tool: { server: '<img>', name: '<b>write</b>' } })
  await flushPromises()
  expect(wrapper.get('.approval-card [role=status]').text()).toBe('Allowed for this conversation')
  expect(wrapper.get('[data-kind=session-grant]').text()).toContain('Ran without asking — allowed for this conversation')
  expect(wrapper.get('[data-kind=session-grant]').text()).toContain('<b>write</b>')
  expect(wrapper.find('img').exists()).toBe(false)
  expect(wrapper.find('[data-kind=session-grant] b').exists()).toBe(false)
  complete?.({ outcome: 'completed' })
  await flushPromises()
  await wrapper.findAll('button').find(button => button.text() === 'New conversation')!.trigger('click')
  await wrapper.get('[role=alertdialog] button').trigger('click')
  await flushPromises()
  expect(closed).toEqual(['session-1'])
  await wrapper.get('#message').setValue('Again')
  await wrapper.get('form').trigger('submit')
  await flushPromises()
  expect(sessions).toBe(2)
  emit({ event: 'approval_requested', request: { ...request, session_grant_allowed: false } })
  await flushPromises()
  expect(wrapper.findAll('button').some(button => button.text() === 'Allow for this conversation')).toBe(false)
  expect(wrapper.get('.approval-card [role=status]').text()).toContain('Waiting for your decision')
  complete?.({ outcome: 'completed' })
  await flushPromises()
  wrapper.unmount()
})

it('rejects malformed grant flags and malformed grant events at the transport boundary', () => {
  expect(parseEvent({ event: 'approval_requested', request: { ...request, session_grant_allowed: 'true' } })).toBeUndefined()
  expect(parseEvent({ event: 'approved_for_session', call_id: 'c', tool: { server: 'sales' } })).toBeUndefined()
})
