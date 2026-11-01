import { afterEach, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks'
import type { Channel } from '@tauri-apps/api/core'
import { bestVoice, createVoiceOutput, stripMarkdown, voiceStorageKey } from './output'
import VoiceSettings from '../VoiceSettings.vue'
import ConversationView from '../ConversationView.vue'
import type { CoreEvent } from '../session'
import { lookingPhrases, retryPhrases } from '../composables/useConversationSpeech'

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
const igorVoices = [
  { id: 'af_heart', name: 'Heart', accent: 'American' },
  { id: 'bf_emma', name: 'Emma', accent: 'British' },
]
const notInstalled = { installed: false, download_bytes: 354_000_000, default_voice: 'af_heart', voices: igorVoices }
const installed = { ...notInstalled, installed: true }
/** Audio elements that finish only when the test says so. */
function audioPlayer() {
  const played: { src: string, onended: (() => void) | null, pause: ReturnType<typeof vi.fn> }[] = []
  vi.stubGlobal('Audio', class {
    src: string
    onended: (() => void) | null = null
    onerror: (() => void) | null = null
    pause = vi.fn()
    constructor(src: string) { this.src = src; played.push(this) }
    play() { return Promise.resolve() }
  })
  let blobs = 0
  vi.stubGlobal('URL', Object.assign(Object.create(URL), { createObjectURL: () => `blob:${blobs++}`, revokeObjectURL: vi.fn() }))
  return played
}
function igorBackend(status = installed) {
  const spoken: { text: string, voice: string, speed: number }[] = []
  const calls: string[] = []
  mockIPC((command, args: any) => {
    calls.push(command)
    if (command === 'speech_status') return status
    if (command === 'speech_prepare') return null
    if (command === 'speech_sentences') return args.text.split(/(?<=[.!?;])\s+/)
    if (command === 'speak') { spoken.push(args.request); return new ArrayBuffer(8) }
    if (command === 'speech_download') {
      args.progress.onmessage({ received: 177_000_000, total: 354_000_000 })
      return installed
    }
    throw new Error(`unexpected ${command}`)
  })
  return { spoken, calls }
}
it('lists Igor\'s voices first and the basic system voices last, after a separator', async () => {
  synthesis()
  igorBackend()
  const wrapper = mount(VoiceSettings)
  await flushPromises()
  const options = wrapper.findAll('#voice-picker option').map(option => option.text())
  expect(options).toEqual(['Automatic (Igor\'s voice)', 'Heart — American', 'Emma — British', '──────────', 'French Premium · fr-FR', 'English Basic · en-US', 'English Enhanced · en-GB'])
  expect(wrapper.findAll('#voice-picker optgroup').map(group => group.attributes('label'))).toEqual(['Igor\'s voices', 'macOS voices (basic)'])
  expect(wrapper.text()).not.toContain('Download Igor')
  wrapper.unmount()
})
it('speaks sentence by sentence with Igor\'s voice, synthesizing ahead, and stop drops the rest', async () => {
  const engine = synthesis()
  const played = audioPlayer()
  const { spoken, calls } = igorBackend()
  localStorage.setItem(voiceStorageKey, JSON.stringify({ enabled: true, voiceURI: 'igor:bf_emma', rate: 1.2 }))
  const voice = createVoiceOutput()
  voice.connect()
  await flushPromises()
  expect(calls).toContain('speech_prepare')
  voice.speak('**First** sentence. Second sentence. Third sentence.')
  await flushPromises()
  expect(engine.speak).not.toHaveBeenCalled()
  expect(spoken).toEqual([
    { text: 'First sentence.', voice: 'bf_emma', speed: 1.2 },
    { text: 'Second sentence.', voice: 'bf_emma', speed: 1.2 },
  ])
  expect(played).toHaveLength(1)
  expect(voice.speaking.value).toBe(true)
  played[0]!.onended?.()
  await flushPromises()
  expect(played).toHaveLength(2)
  expect(spoken.map(request => request.text)).toContain('Third sentence.')
  voice.stop()
  await flushPromises()
  expect(played[1]!.pause).toHaveBeenCalled()
  expect(played).toHaveLength(2)
  expect(voice.speaking.value).toBe(false)
  voice.speak('After stop.', { interrupt: true })
  await flushPromises()
  expect(spoken.at(-1)?.text).toBe('After stop.')
  expect(played).toHaveLength(3)
  voice.dispose()
})
it('uses the system voice until Igor\'s voice is downloaded, and downloads it with progress', async () => {
  const engine = synthesis()
  const { calls } = igorBackend(notInstalled)
  localStorage.setItem(voiceStorageKey, JSON.stringify({ enabled: true }))
  const wrapper = mount(VoiceSettings)
  await flushPromises()
  await wrapper.findAll('button').find(button => button.text() === 'Test voice')!.trigger('click')
  expect(engine.speak).toHaveBeenCalledTimes(1)
  expect(calls).not.toContain('speak')
  await wrapper.findAll('button').find(button => button.text().startsWith('Download Igor'))!.trigger('click')
  await flushPromises()
  expect(calls).toContain('speech_download')
  expect(wrapper.text()).not.toContain('Download Igor')
  expect(wrapper.find('#voice-picker optgroup').attributes('label')).toBe('Igor\'s voices')
  wrapper.unmount()
})
it('prefers available English Enhanced/Premium voices and strips Markdown without speaking link targets', () => {
  expect(bestVoice(voices as SpeechSynthesisVoice[])?.voiceURI).toBe('enhanced')
  expect(stripMarkdown('# Hello **operator**\n[stock](https://example.com) `done`')).toBe('Hello operator stock done')
})
it('persists only local voice preferences, accepts async voice lists and plays a test voice', async () => {
  const engine = synthesis()
  engine.getVoices.mockReturnValueOnce([])
  let ipc = 0
  mockIPC(command => {
    if (command === 'speech_status') return notInstalled
    ipc++
    throw new Error('Voice settings must not use native configuration')
  })
  const wrapper = mount(VoiceSettings)
  await flushPromises()
  expect(wrapper.findAll('#voice-picker option')).toHaveLength(1)
  const changed = engine.addEventListener.mock.calls[0]?.[1]
  changed()
  await flushPromises()
  // Automatic, the separator and the three system voices.
  expect(wrapper.findAll('#voice-picker option')).toHaveLength(5)
  expect(wrapper.text()).toContain('Download Igor\'s voice (354 MB)')
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
  backend.emit({ event: 'approval_requested', request: { approval_id: 'a', call_id: 'c', tool: { server: 'sales', name: 'create_draft_order' }, canonical_arguments: '{"supplier_id":"supplier-x"}', action_hash: 'h', session_grant_allowed: true } })
  await flushPromises()
  expect(engine.speak.mock.calls.at(-1)?.[0].text).toMatch(/Approve on screen\?$/)
  expect(engine.speak.mock.calls.at(-1)?.[0].text).not.toContain('Allow for this conversation')
  expect(backend.calls).not.toContain('resolve_approval')
  expect(wrapper.get('.voice-input [role=status]').text()).toBe('Speaking')
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

it('says it is looking when tools start and that it is trying again after a failure, then the reply', async () => {
  const engine = synthesis()
  localStorage.setItem(voiceStorageKey, JSON.stringify({ enabled: true }))
  const backend = conversationBackend()
  const wrapper = mount(ConversationView)
  await send(wrapper)
  const tool = { server: 'weather', name: 'get_current_weather' }
  backend.emit({ event: 'tool_call_proposed', call_id: 'c1', tool, arguments: { city: 'Hopetown' } })
  backend.emit({ event: 'tool_call_started', call_id: 'c1' })
  await flushPromises()
  expect(engine.speak.mock.calls.map(call => call[0].text)).toEqual([lookingPhrases[0]])
  backend.emit({ event: 'tool_call_failed', call_id: 'c1', outcome: 'not_executed', message: 'timed out' })
  backend.emit({ event: 'tool_call_proposed', call_id: 'c2', tool, arguments: { city: 'Hopetown, South Africa' } })
  backend.emit({ event: 'tool_call_started', call_id: 'c2' })
  backend.emit({ event: 'tool_call_failed', call_id: 'c2', outcome: 'not_executed', message: 'timed out' })
  backend.emit({ event: 'assistant_text', text: 'It is about 27 degrees and overcast.' })
  backend.finish()
  await flushPromises()
  // The system voice speaks one queued phrase at a time.
  expect(engine.speak).toHaveBeenCalledTimes(1)
  const first = engine.speak.mock.calls[0]![0]
  first.onend?.call(first, new Event('end') as SpeechSynthesisEvent)
  const second = engine.speak.mock.calls[1]![0]
  expect(second.text).toBe(retryPhrases[1])
  second.onend?.call(second, new Event('end') as SpeechSynthesisEvent)
  expect(engine.speak.mock.calls.map(call => call[0].text)).toEqual([lookingPhrases[0], retryPhrases[1], 'It is about 27 degrees and overcast.'])
  wrapper.unmount()
})

it('sends spoken: true only while voice output is on', async () => {
  synthesis()
  const requests: { spoken: boolean }[] = []
  mockIPC((command, args: any) => {
    if (command === 'conversation_providers') return { providers: [{ id: 'local', model: 'demo', unavailable: null }] }
    if (command === 'settings_view') return { servers: [] }
    if (command === 'start_session') return { session_id: 'one', provider_id: 'local', model: 'demo', servers: [] }
    if (command === 'send_message') { requests.push(args.request); return { outcome: 'completed' } }
  })
  localStorage.setItem(voiceStorageKey, JSON.stringify({ enabled: true }))
  const spoken = mount(ConversationView)
  await send(spoken)
  spoken.unmount()
  localStorage.setItem(voiceStorageKey, JSON.stringify({ enabled: false }))
  const written = mount(ConversationView)
  await send(written)
  written.unmount()
  expect(requests.map(request => request.spoken)).toEqual([true, false])
})

it('preserves reply order across asynchronous sentence requests and drops a late split after stop', async () => {
  synthesis()
  const played = audioPlayer()
  const spoken: string[] = []
  let split: ((sentences: string[]) => void) | undefined
  mockIPC((command, args: any) => {
    if (command === 'speech_status') return installed
    if (command === 'speech_prepare') return null
    if (command === 'speech_sentences') return new Promise(resolve => { split = resolve })
    if (command === 'speak') { spoken.push(args.request.text); return new ArrayBuffer(8) }
  })
  const voice = createVoiceOutput()
  voice.connect()
  voice.preferences.value.enabled = true
  await flushPromises()
  voice.speak('First; second.')
  voice.speak('Third.')
  await flushPromises()
  split?.(['First;', 'second.'])
  await flushPromises()
  expect(spoken).toEqual(['First;', 'second.'])
  split?.(['Third.'])
  await flushPromises()
  played[0]!.onended?.()
  await flushPromises()
  expect(spoken).toEqual(['First;', 'second.', 'Third.'])
  voice.speak('Cancelled.')
  await flushPromises()
  voice.stop()
  split?.(['Cancelled.'])
  await flushPromises()
  expect(spoken).not.toContain('Cancelled.')
  voice.dispose()
})
