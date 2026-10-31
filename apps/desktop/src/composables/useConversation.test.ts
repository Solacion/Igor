import { defineComponent, h } from 'vue'
import { mount, flushPromises } from '@vue/test-utils'
import { afterEach, expect, it } from 'vitest'
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks'
import { useConversation } from './useConversation'
import { parseEvent, eventGuards } from '../events'

afterEach(clearMocks)
async function host() {
  mockIPC(command => command === 'conversation_providers' ? { providers: [] } : { servers: [] })
  let state: ReturnType<typeof useConversation> | undefined
  const wrapper = mount(defineComponent({ setup() {
    state = useConversation()
    return () => h('div')
  } }))
  await flushPromises()
  if (!state) throw new Error('Composable not mounted')
  return { state, wrapper }
}
it('reduces guarded events, ignores unknown/malformed variants and duplicate sequence numbers', async () => {
  const { state, wrapper } = await host()
  state.receive({ schema_version: 1, sequence: 0, event: { event: 'tool_call_proposed', call_id: 'c', tool: { server: 'sales', name: 'stock' }, arguments: { sku: 'x' } } })
  state.receive({ schema_version: 1, sequence: 1, event: { event: 'tool_call_completed', call_id: 'c', is_error: false } })
  expect(state.activity.value['0:c']?.state).toBe('Completed')
  state.receive({ schema_version: 1, sequence: 1, event: { event: 'assistant_text', text: 'duplicate' } })
  state.receive({ schema_version: 1, sequence: 2, event: { event: 'future_event' } })
  state.receive({ schema_version: 1, sequence: 3, event: { event: 'approval_requested', request: { approval_id: 'missing hash' } } })
  expect(state.approvals.value).toEqual([])
  expect(state.messages.value).toEqual([])
  state.receive({ schema_version: 1, sequence: 4, event: { event: 'assistant_text', text: '<img>literal' } })
  expect(state.messages.value).toEqual([{ role: 'Igor', text: '<img>literal' }])
  state.receive({ schema_version: 1, sequence: 6, event: { event: 'assistant_text', text: 'gap' } })
  expect(state.error.value).toContain('event stream was interrupted')
  expect(state.messages.value).toHaveLength(1)
  wrapper.unmount()
})
it('keeps lifecycle/reducer state scoped to each instance and ignores events after disposal', async () => {
  const first = await host()
  const second = await host()
  first.state.receive({ schema_version: 1, sequence: 0, event: { event: 'clarification_requested', request_id: 'q', question: 'When?' } })
  expect(second.state.questions.value).toEqual([])
  first.wrapper.unmount()
  first.state.receive({ schema_version: 1, sequence: 1, event: { event: 'assistant_text', text: 'late' } })
  expect(first.state.messages.value).toEqual([])
  second.wrapper.unmount()
})
it('has a guard for every schema variant, rejecting arrays, nulls and incomplete payloads', () => {
  expect(eventGuards).toHaveLength(14)
  for (const guard of eventGuards) {
    expect(guard(null)).toBe(false)
    expect(guard([])).toBe(false)
    expect(guard({})).toBe(false)
  }
  expect(parseEvent({ event: 'connection_changed', server: 's', state: ['connected'] })).toBeUndefined()
  expect(parseEvent({ event: 'approval_resolved', approval_id: 'a', call_id: 'c', outcome: {} })).toBeUndefined()
})
