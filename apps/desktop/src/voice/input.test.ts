import { defineComponent, h, ref } from 'vue'
import { mount, flushPromises } from '@vue/test-utils'
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { useVoiceInput } from '../composables/useVoiceInput'
import { monoFrame, resample16k } from './audio'
import { transcription, type TranscriptionAudio } from './transcription'
import { voiceStorageKey } from './output'
import ConversationView from '../ConversationView.vue'

vi.mock('./transcription', async importOriginal => ({ ...await importOriginal<typeof import('./transcription')>(), transcription: { available: true, transcribe: vi.fn(async () => ({ text: 'Voice message', language: 'en' })) } }))
const contexts: MockContext[] = []
class MockContext {
  static resumeGate: Promise<void> | undefined
  sampleRate = 48000
  destination = {}
  source = { connect: vi.fn(), disconnect: vi.fn() }
  processor = { connect: vi.fn(), disconnect: vi.fn(), onaudioprocess: null as ((event: { inputBuffer: AudioBuffer }) => void) | null }
  mute = { connect: vi.fn(), disconnect: vi.fn(), gain: { value: 1 } }
  resume = vi.fn(() => MockContext.resumeGate ?? Promise.resolve())
  close = vi.fn(async () => {})
  constructor() { contexts.push(this) }
  createMediaStreamSource() { return this.source }
  createScriptProcessor() { return this.processor }
  createGain() { return this.mute }
}
function audioBuffer(channels: Float32Array[]) {
  return { length: channels[0]?.length ?? 0, numberOfChannels: channels.length, getChannelData: (index: number) => channels[index]! } as AudioBuffer
}
function feed(value = .2) {
  const processor = contexts.at(-1)?.processor
  for (let index = 0; index < 3; index++) processor?.onaudioprocess?.({ inputBuffer: audioBuffer([new Float32Array(4096).fill(value)]) })
}
let stop = vi.fn()
let getUserMedia = vi.fn()
beforeEach(() => {
  contexts.length = 0
  MockContext.resumeGate = undefined
  stop = vi.fn()
  getUserMedia = vi.fn(async () => ({ getTracks: () => [{ stop }] }))
  vi.stubGlobal('navigator', { mediaDevices: { getUserMedia } })
  vi.stubGlobal('AudioContext', MockContext)
  vi.mocked(transcription.transcribe).mockReset().mockResolvedValue({ text: 'Voice message', language: 'en' })
  localStorage.clear()
})
afterEach(() => { vi.unstubAllGlobals(); vi.useRealTimers(); clearMocks(); localStorage.clear() })
async function host() {
  let state: ReturnType<typeof useVoiceInput> | undefined
  const active = ref(true)
  const onText = vi.fn()
  const onError = vi.fn()
  const onStart = vi.fn()
  const wrapper = mount(defineComponent({ setup() {
    state = useVoiceInput({ active: () => active.value, busy: () => false, onText, onError, onStart })
    return () => h('div')
  } }))
  if (!state) throw new Error('Voice input not mounted')
  await flushPromises()
  return { state, wrapper, active, onText, onError, onStart }
}
it('mixes stereo and resamples to bounded 16 kHz mono Float32 PCM', () => {
  const stereo = monoFrame(audioBuffer([new Float32Array([.6, .2]), new Float32Array([.2, .6])]))
  expect(Array.from(stereo)).toEqual([expect.closeTo(.4), expect.closeTo(.4)])
  const pcm = resample16k(new Float32Array(48000).fill(.3), 48000)
  expect(pcm).toBeInstanceOf(Float32Array)
  expect(pcm).toHaveLength(16000)
  expect(pcm[0]).toBeCloseTo(.3)
  expect(Array.from(resample16k(new Float32Array([NaN, Infinity, 2, -2]), 16000))).toEqual([0, 0, 1, -1])
})
it('captures until stopped, meters volume, stops every track and passes PCM when finished', async () => {
  const { state, wrapper, onText, onStart } = await host()
  await state.start()
  expect(state.state.value).toBe('Listening')
  expect(getUserMedia).toHaveBeenCalledWith({ audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true }, video: false })
  feed()
  expect(state.level.value).toBeGreaterThan(0)
  await state.release()
  expect(stop).toHaveBeenCalled()
  expect(contexts[0]?.close).toHaveBeenCalled()
  const audio = vi.mocked(transcription.transcribe).mock.calls[0]?.[0] as TranscriptionAudio
  expect(audio.sampleRate).toBe(16000)
  expect(audio.channels).toBe(1)
  expect(audio.samples).toHaveLength(4096)
  expect(onText).toHaveBeenCalledWith('Voice message')
  expect(onStart).toHaveBeenCalledOnce()
  expect(state.state.value).toBe('Idle')
  wrapper.unmount()
})
it('reports denied permission and silence without inventing a transcript', async () => {
  const { state, wrapper, onError } = await host()
  getUserMedia.mockRejectedValueOnce(new DOMException('private detail', 'NotAllowedError'))
  await state.start()
  expect(state.error.value).toContain('permission was denied')
  expect(state.error.value).not.toContain('private detail')
  await state.start()
  feed(0)
  await state.release()
  expect(state.error.value).toContain('No speech heard')
  expect(onError).toHaveBeenCalledTimes(2)
  expect(transcription.transcribe).not.toHaveBeenCalled()
  wrapper.unmount()
})
it('stops a late permission stream after release and cancels recording on unmount', async () => {
  const { state, wrapper } = await host()
  let grant: (stream: unknown) => void = () => {}
  getUserMedia.mockImplementationOnce(() => new Promise(resolve => { grant = resolve }))
  const pending = state.start()
  await state.release()
  grant({ getTracks: () => [{ stop }] })
  await pending
  expect(stop).toHaveBeenCalled()
  expect(transcription.transcribe).not.toHaveBeenCalled()
  await state.start()
  wrapper.unmount()
  expect(contexts.at(-1)?.close).toHaveBeenCalled()
  expect(state.state.value).toBe('Idle')
})
it('toggles Space outside controls, ignores editable/interactive targets and cancels on blur', async () => {
  const { state, wrapper } = await host()
  const input = document.createElement('textarea')
  document.body.append(input)
  input.dispatchEvent(new KeyboardEvent('keydown', { code: 'Space', bubbles: true, cancelable: true }))
  await flushPromises()
  expect(getUserMedia).not.toHaveBeenCalled()
  window.dispatchEvent(new KeyboardEvent('keydown', { code: 'Space', cancelable: true }))
  await flushPromises()
  expect(state.state.value).toBe('Listening')
  window.dispatchEvent(new Event('blur'))
  expect(stop).toHaveBeenCalled()
  expect(state.state.value).toBe('Idle')
  input.remove()
  wrapper.unmount()
})
it('drops late transcription after navigation and bounds recordings to one minute', async () => {
  vi.useFakeTimers()
  const { state, active, wrapper, onText } = await host()
  await state.start()
  feed()
  let resolve: (text: { text: string, language: string | null }) => void = () => {}
  vi.mocked(transcription.transcribe).mockImplementationOnce(() => new Promise(done => { resolve = done }))
  const release = state.release()
  expect(state.state.value).toBe('Transcribing')
  active.value = false
  await flushPromises()
  resolve({ text: 'Late words', language: 'en' })
  await release
  expect(onText).not.toHaveBeenCalled()
  active.value = true
  await flushPromises()
  await state.start()
  feed()
  await vi.advanceTimersByTimeAsync(60000)
  expect(state.error.value).toContain('one-minute limit')
  expect(transcription.transcribe).toHaveBeenCalledTimes(1)
  expect(stop).toHaveBeenCalled()
  expect(state.state.value).toBe('Idle')
  wrapper.unmount()
})
function backend() {
  const sends: string[] = []
  mockIPC((command, args: any) => {
    if (command === 'speech_languages') return [{ code: 'de', name: 'german' }, { code: 'en', name: 'english' }]
    if (command === 'conversation_providers') return { providers: [{ id: 'any-provider', model: 'any-model', unavailable: null }] }
    if (command === 'settings_view') return { servers: [] }
    if (command === 'start_session') return { session_id: 'one', provider_id: 'any-provider', model: 'any-model', servers: [] }
    if (command === 'send_message') { sends.push(args.request.text); return { outcome: 'completed' } }
  })
  return sends
}
it.each([false, true])('automatically sends a finished recording even with legacy review=%s', async review => {
  localStorage.setItem(voiceStorageKey, JSON.stringify({ reviewBeforeSending: review }))
  const sends = backend()
  const wrapper = mount(ConversationView)
  await flushPromises()
  await wrapper.get('.mic-button').trigger('click')
  await flushPromises()
  expect(wrapper.get('.voice-input [role=status]').text()).toBe('Listening')
  expect(wrapper.find('meter').exists()).toBe(true)
  feed()
  await wrapper.get('.mic-button').trigger('click')
  await flushPromises()
  expect(sends).toEqual(['Voice message'])
  expect(wrapper.text()).not.toContain('Heard in')
  expect((wrapper.get('#message').element as HTMLTextAreaElement).value).toBe('')
  wrapper.unmount()
})
it('speaks an inline permission error when output is enabled and stops speech when talking starts', async () => {
  const engine = { getVoices: () => [], speak: vi.fn(), cancel: vi.fn(), addEventListener: vi.fn(), removeEventListener: vi.fn() }
  vi.stubGlobal('speechSynthesis', engine)
  vi.stubGlobal('SpeechSynthesisUtterance', class { constructor(public text: string) {} })
  localStorage.setItem(voiceStorageKey, JSON.stringify({ enabled: true }))
  backend()
  const wrapper = mount(ConversationView)
  await flushPromises()
  getUserMedia.mockRejectedValueOnce(new DOMException('private', 'NotAllowedError'))
  await wrapper.get('.mic-button').trigger('click')
  await flushPromises()
  expect(engine.cancel).toHaveBeenCalled()
  expect(wrapper.get('.voice-input [role=alert]').text()).toContain('permission was denied')
  expect(engine.speak.mock.calls.at(-1)?.[0].text).toContain('permission was denied')
  expect(wrapper.get('.voice-input [role=status]').text()).toBe('Speaking')
  wrapper.unmount()
})

it('stops the granted microphone immediately if released while AudioContext is resuming', async () => {
  const { state, wrapper } = await host()
  let resume: () => void = () => {}
  MockContext.resumeGate = new Promise(resolve => { resume = resolve })
  const pending = state.start()
  await flushPromises()
  expect(contexts).toHaveLength(1)
  await state.release()
  expect(stop).toHaveBeenCalled()
  expect(contexts[0]?.close).toHaveBeenCalled()
  resume()
  await pending
  expect(transcription.transcribe).not.toHaveBeenCalled()
  wrapper.unmount()
})
it('reports an empty recognized transcript instead of sending an empty message', async () => {
  const { state, wrapper, onText } = await host()
  vi.mocked(transcription.transcribe).mockResolvedValueOnce({ text: '  ', language: null })
  await state.start()
  feed()
  await state.release()
  expect(state.error.value).toContain('No speech heard')
  expect(onText).not.toHaveBeenCalled()
  wrapper.unmount()
})

it('enables talking after native recognition preparation', async () => {
  let resolve!: () => void
  let input!: ReturnType<typeof useVoiceInput>
  const prepare = vi.fn(() => new Promise<void>(done => { resolve = done }))
  const wrapper = mount(defineComponent({ setup() {
    input = useVoiceInput({ active: () => true, busy: () => false, onStart: vi.fn(), onText: vi.fn(), onError: vi.fn() }, { available: true, prepare, transcribe: vi.fn() })
    return () => h('div')
  } }))
  expect(input.preparing.value).toBe(true)
  expect(input.available).toBe(false)
  resolve()
  await flushPromises()
  expect(input.preparing.value).toBe(false)
  expect(input.available).toBe(true)
  await input.start()
  expect(getUserMedia).toHaveBeenCalled()
  wrapper.unmount()
})

it('ends a stuck preparation and allows retry', async () => {
  vi.useFakeTimers()
  let input!: ReturnType<typeof useVoiceInput>
  const prepare = vi.fn<() => Promise<void>>().mockImplementationOnce(() => new Promise(() => {})).mockResolvedValue(undefined)
  const wrapper = mount(defineComponent({ setup() {
    input = useVoiceInput({ active: () => true, busy: () => false, onStart: vi.fn(), onText: vi.fn(), onError: vi.fn() }, { available: true, prepare, transcribe: vi.fn() })
    return () => h('div')
  } }))
  await vi.advanceTimersByTimeAsync(60000)
  expect(input.preparing.value).toBe(false)
  expect(input.available).toBe(false)
  expect(input.error.value).toContain('too long')
  await input.prepare()
  expect(input.available).toBe(true)
  expect(input.error.value).toBe('')
  wrapper.unmount()
})

it('sends once after speech followed by a pause, while tolerating short pauses', async () => {
  const sends = backend()
  const wrapper = mount(ConversationView)
  await flushPromises()
  await wrapper.get('.mic-button').trigger('click')
  await flushPromises()
  expect(wrapper.get('.mic-button').attributes('aria-pressed')).toBe('true')
  feed()
  feed(0)
  await flushPromises()
  expect(sends).toEqual([])
  expect(wrapper.get('.mic-button').text()).toBe('Stop listening')
  feed()
  for (let i = 0; i < 5; i++) feed(0)
  await flushPromises()
  expect(sends).toEqual(['Voice message'])
  expect(wrapper.text()).not.toContain('Heard in')
  expect(stop).toHaveBeenCalledOnce()
  expect(wrapper.get('.mic-button').attributes('aria-pressed')).toBe('false')
  expect(wrapper.get('.mic-button').text()).toBe('Talk')
  wrapper.unmount()
})

it('stops listening without sending when no speech is detected', async () => {
  const { state, wrapper, onText } = await host()
  await state.toggle()
  for (let i = 0; i < 40; i++) feed(0)
  await flushPromises()
  expect(state.state.value).toBe('Idle')
  expect(state.error.value).toContain('No speech heard')
  expect(stop).toHaveBeenCalledOnce()
  expect(transcription.transcribe).not.toHaveBeenCalled()
  expect(onText).not.toHaveBeenCalled()
  wrapper.unmount()
})

it('Space toggles listening without stopping on key release or repeated keydown', async () => {
  const { state, wrapper, onText } = await host()
  window.dispatchEvent(new KeyboardEvent('keydown', { code: 'Space' }))
  await flushPromises()
  feed()
  window.dispatchEvent(new KeyboardEvent('keyup', { code: 'Space' }))
  window.dispatchEvent(new KeyboardEvent('keydown', { code: 'Space', repeat: true }))
  expect(state.state.value).toBe('Listening')
  window.dispatchEvent(new KeyboardEvent('keydown', { code: 'Space' }))
  await flushPromises()
  expect(onText).toHaveBeenCalledOnce()
  expect(state.state.value).toBe('Idle')
  wrapper.unmount()
})

it.each(['de', null])('passes speech language %s from preferences to recognition and shows detected German', async language => {
  localStorage.setItem(voiceStorageKey, JSON.stringify({ speechLanguage: language }))
  const sends = backend()
  vi.mocked(transcription.transcribe).mockResolvedValueOnce({ text: 'Guten Tag', language: 'de' })
  const wrapper = mount(ConversationView)
  await flushPromises()
  await wrapper.get('.mic-button').trigger('click')
  await flushPromises()
  feed()
  await wrapper.get('.mic-button').trigger('click')
  await flushPromises()
  expect(vi.mocked(transcription.transcribe).mock.calls[0]?.[0].language).toBe(language)
  expect(sends).toEqual(['Guten Tag'])
  expect(wrapper.text()).toContain('Heard in German')
  wrapper.unmount()
})

it('enables the toggle when the native download and preparation finish after mounting', async () => {
  const ready = ref(false)
  let input!: ReturnType<typeof useVoiceInput>
  const wrapper = mount(defineComponent({ setup() {
    input = useVoiceInput({ active: () => true, busy: () => false, onStart: vi.fn(), onText: vi.fn(), onError: vi.fn() }, {
      get available() { return ready.value },
      refresh: vi.fn(async () => {}),
      transcribe: vi.fn(),
    })
    return () => h('div')
  } }))
  await input.toggle()
  expect(getUserMedia).not.toHaveBeenCalled()
  ready.value = true
  await input.toggle()
  expect(input.state.value).toBe('Listening')
  expect(getUserMedia).toHaveBeenCalledOnce()
  wrapper.unmount()
})
