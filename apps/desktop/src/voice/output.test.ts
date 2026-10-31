import { afterEach, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks'
import type { Channel } from '@tauri-apps/api/core'
import { bestVoice, createVoiceOutput, stripMarkdown, voiceStorageKey } from './output'
import VoiceSettings from '../VoiceSettings.vue'
import ConversationView from '../ConversationView.vue'
import type { CoreEvent } from '../session'

const voices = [
  { name: 'French Premium', lang: 'fr-FR', voiceURI: 'fr', default: true, localService: true },
  { name: 'English Basic', lang: 'en-US', voiceURI: 'basic', default: true, localService: true },
  { name: 'English Enhanced', lang: 'en-GB', voiceURI: 'enhanced', default: false, localService: true },
]
function synthesis() {
  const engine = {
    getVoices: vi.fn(() => voices),
    speak: vi.fn<(utterance: SpeechSynthesisUtterance) => void>(),
    cancel: vi.fn(),
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
  }
  vi.stubGlobal('speechSynthesis', engine)
  vi.stubGlobal('SpeechSynthesisUtterance', class {
    text: string
    voice = null
    rate = 1
    lang = ''
    onend: (() => void) | null = null
    onerror: (() => void) | null = null
    constructor(text: string) { this.text = text }
  })
  return engine
}
afterEach(() => { localStorage.clear(); vi.unstubAllGlobals(); clearMocks() })
it('prefers available English Enhanced/Premium voices and strips Markdown without speaking link targets', () => {
  expect(bestVoice(voices as SpeechSynthesisVoice[])?.voiceURI).toBe('enhanced')
  expect(stripMarkdown('# Hello **operator**\n[stock](https://example.com) `done`')).toBe('Hello operator stock done')
})
it('persists only local voice preferences, accepts async voice lists and plays a test voice', async () => {
  const engine = synthesis()
  engine.getVoices.mockReturnValueOnce([])
  let ipc = 0
  mockIPC(() => { ipc++; throw new Error('Voice settings must not use native configuration') })
  const wrapper = mount(VoiceSettings)
  await flushPromises()
  expect(wrapper.findAll('#voice-picker option')).toHaveLength(1)
  const changed = engine.addEventListener.mock.calls[0]?.[1]
  changed()
  await flushPromises()
  expect(wrapper.findAll('#voice-picker option')).toHaveLength(4)
  await wrapper.findAll('input[type=checkbox]')[0]!.setValue(true)
  await wrapper.get('#voice-picker').setValue('enhanced')
  await wrapper.get('#voice-rate').setValue('1.3')
  await wrapper.findAll('button').find(button => button.text() === 'Test voice')!.trigger('click')
  expect(engine.speak.mock.calls[0]?.[0].voice?.voiceURI).toBe('enhanced')
  expect(engine.speak.mock.calls[0]?.[0].rate).toBe(1.3)
  expect(JSON.parse(localStorage.getItem(voiceStorageKey)!)).toMatchObject({ enabled: true, voiceURI: 'enhanced', rate: 1.3 })
  expect(ipc).toBe(0)
  await wrapper.findAll('button').find(button => button.text() === 'Stop speaking')!.trigger('click')
  expect(engine.cancel).toHaveBeenCalled()
  wrapper.unmount()
  expect(engine.removeEventListener).toHaveBeenCalled()
})
it('cancels the queue and ignores late speech callbacks after stopping', () => {
  const engine = synthesis()
  const voice = createVoiceOutput()
  voice.connect()
  voice.preferences.value.enabled = true
  voice.speak('First')
  voice.speak('Second')
  const first = engine.speak.mock.calls[0]?.[0]
  voice.stop()
  first?.onend?.call(first, new Event('end') as SpeechSynthesisEvent)
  expect(engine.speak).toHaveBeenCalledTimes(1)
  expect(voice.speaking.value).toBe(false)
  voice.dispose()
})
function conversationBackend() {
  let channel: Channel<CoreEvent> | undefined
  let sequence = 0
  let finish: (value: unknown) => void = () => {}
  const calls: string[] = []
  mockIPC((command, args: any) => {
    calls.push(command)
    if (command === 'conversation_providers') return { providers: [{ id: 'local', model: 'demo', unavailable: null }] }
    if (command === 'settings_view') return { servers: [] }
    if (command === 'start_session') { channel = args.events; return { session_id: 'one', provider_id: 'local', model: 'demo', servers: [] } }
    if (command === 'send_message') return new Promise(resolve => { finish = resolve })
  })
  return { calls, emit(event: unknown) { channel?.onmessage({ schema_version: 1, sequence: sequence++, event }) }, finish() { finish({ outcome: 'completed' }) } }
}
async function send(wrapper: ReturnType<typeof mount>) {
  await flushPromises()
  await wrapper.get('#message').setValue('Hello')
  await wrapper.get('form').trigger('submit')
  await flushPromises()
}
it('speaks each assistant text only after completion, with queued order and a visible stop control', async () => {
  const engine = synthesis()
  localStorage.setItem(voiceStorageKey, JSON.stringify({ enabled: true }))
  const backend = conversationBackend()
  const wrapper = mount(ConversationView)
  await send(wrapper)
  backend.emit({ event: 'assistant_text', text: '**First** response' })
  backend.emit({ event: 'assistant_text', text: 'Second response' })
  await flushPromises()
  expect(engine.speak).not.toHaveBeenCalled()
  backend.finish()
  await flushPromises()
  expect(engine.speak.mock.calls[0]?.[0].text).toBe('First response')
  const first = engine.speak.mock.calls[0]![0]
  first.onend?.call(first, new Event('end') as SpeechSynthesisEvent)
  expect(engine.speak.mock.calls[1]?.[0].text).toBe('Second response')
  expect(wrapper.findAll('button').some(button => button.text() === 'Stop speaking')).toBe(true)
  await wrapper.findAll('button').find(button => button.text() === 'New conversation')!.trigger('click')
  expect(engine.cancel).toHaveBeenCalled()
  wrapper.unmount()
})
it('speaks an approval summary but never accepts spoken approval, and suppresses speech after Stop turn', async () => {
  const engine = synthesis()
  localStorage.setItem(voiceStorageKey, JSON.stringify({ enabled: true }))
  const backend = conversationBackend()
  const wrapper = mount(ConversationView)
  await send(wrapper)
  backend.emit({ event: 'approval_requested', request: { approval_id: 'a', call_id: 'c', tool: { server: 'sales', name: 'create_draft_order' }, canonical_arguments: '{"supplier_id":"supplier-x"}', action_hash: 'h' } })
  await flushPromises()
  expect(engine.speak.mock.calls.at(-1)?.[0].text).toMatch(/Approve on screen\?$/)
  expect(backend.calls).not.toContain('resolve_approval')
  await wrapper.findAll('button').find(button => button.text() === 'Stop turn')!.trigger('click')
  const count = engine.speak.mock.calls.length
  backend.emit({ event: 'assistant_text', text: 'Late text' })
  backend.finish()
  await flushPromises()
  expect(engine.speak).toHaveBeenCalledTimes(count)
  expect(engine.cancel).toHaveBeenCalled()
  wrapper.unmount()
})

it('speaks late channel text once when the send reply arrives before the events', async () => {
  const engine = synthesis()
  localStorage.setItem(voiceStorageKey, JSON.stringify({ enabled: true }))
  const backend = conversationBackend()
  const wrapper = mount(ConversationView)
  await send(wrapper)
  backend.finish()
  await flushPromises()
  backend.emit({ event: 'assistant_text', text: 'Late channel reply' })
  backend.emit({ event: 'turn_finished', outcome: { outcome: 'completed' } })
  await flushPromises()
  expect(engine.speak).toHaveBeenCalledTimes(1)
  expect(engine.speak.mock.calls[0]?.[0].text).toBe('Late channel reply')
  wrapper.unmount()
})
