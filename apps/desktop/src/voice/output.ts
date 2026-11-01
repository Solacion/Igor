import { computed, inject, onScopeDispose, provide, ref, watch, type InjectionKey } from 'vue'
import { Channel, invoke } from '@tauri-apps/api/core'
export interface VoicePreferences {
  enabled: boolean
  voiceURI: string
  rate: number
  reviewBeforeSending: boolean
}
export const voiceStorageKey = 'igor.desktop.voice.v1'
/** Prefix of a `voiceURI` that names one of Igor's own (Kokoro) voices rather than a system voice. */
export const igorVoicePrefix = 'igor:'
export interface IgorVoice {
  id: string
  name: string
  accent: 'American' | 'British'
}
/** Igor's embedded voice model, as reported by the native `speech_status` command. */
export interface SpeechStatus {
  installed: boolean
  download_bytes: number
  default_voice: string
  voices: IgorVoice[]
}
export function stripMarkdown(text: string): string {
  return text.replace(/```[^\n]*\n([\s\S]*?)```/g, '$1')
    .replace(/!\[([^\]]*)\]\([^)]*\)/g, '$1')
    .replace(/\[([^\]]+)\]\([^)]*\)/g, '$1')
    .replace(/<[^>]*>/g, ' ')
    .replace(/^\s{0,3}(?:#{1,6}\s+|>\s*|[-*+]\s+|\d+\.\s+)/gm, '')
    .replace(/[`*_~]/g, '')
    .replace(/\\([\\`*_{}\[\]()#+.!>-])/g, '$1')
    .replace(/\s+/g, ' ').trim()
}
export function bestVoice(voices: SpeechSynthesisVoice[]) {
  const english = voices.filter(voice => /^en(?:[-_]|$)/i.test(voice.lang))
  const score = (voice: SpeechSynthesisVoice) => (/premium/i.test(voice.name) ? 30 : /enhanced/i.test(voice.name) ? 20 : 0) + (voice.default ? 2 : 0) + (voice.localService ? 1 : 0)
  return [...(english.length ? english : voices)].sort((a, b) => score(b) - score(a))[0]
}
function loadPreferences(): VoicePreferences {
  const fallback = { enabled: false, voiceURI: '', rate: 1, reviewBeforeSending: false }
  try {
    const saved: unknown = JSON.parse(localStorage.getItem(voiceStorageKey) ?? 'null')
    if (!saved || typeof saved !== 'object')
      return fallback
    const value = saved as Record<string, unknown>
    return {
      enabled: value.enabled === true,
      voiceURI: typeof value.voiceURI === 'string' ? value.voiceURI : '',
      rate: typeof value.rate === 'number' && Number.isFinite(value.rate) && value.rate >= .5 && value.rate <= 2 ? value.rate : 1,
      reviewBeforeSending: value.reviewBeforeSending === true,
    }
  }
  catch {
    return fallback
  }
}
export function createVoiceOutput(synthesis = globalThis.speechSynthesis) {
  const preferences = ref(loadPreferences())
  const voices = ref<SpeechSynthesisVoice[]>([])
  const igor = ref<SpeechStatus | null>(null)
  const download = ref<{ received: number, total: number } | null>(null)
  const speaking = ref(false)
  const error = ref('')
  const systemSupported = !!synthesis && typeof globalThis.SpeechSynthesisUtterance === 'function'
  /** Igor's own voice, when downloaded: the chosen one, or the model default for Automatic. */
  const igorVoice = computed(() => {
    if (!igor.value?.installed)
      return undefined
    const uri = preferences.value.voiceURI
    if (uri.startsWith(igorVoicePrefix))
      return igor.value.voices.find(voice => igorVoicePrefix + voice.id === uri) ?? igor.value.voices.find(voice => voice.id === igor.value?.default_voice)
    return uri ? undefined : igor.value.voices.find(voice => voice.id === igor.value?.default_voice)
  })
  const selectedVoice = computed(() => voices.value.find(voice => voice.voiceURI === preferences.value.voiceURI) ?? bestVoice(voices.value))
  const supported = computed(() => systemSupported || !!igor.value?.installed)
  let users = 0
  let generation = 0
  const queue: string[] = []
  let current: SpeechSynthesisUtterance | undefined
  /** Igor-voice playback: the running queue, the playing sentence and how to end it early. */
  let igorRunning = false
  let audio: HTMLAudioElement | undefined
  let endAudio: (() => void) | undefined
  let prepared = false
  let sentenceRequests = Promise.resolve()
  async function refreshIgor() {
    try {
      igor.value = await invoke<SpeechStatus>('speech_status')
      if (igor.value.installed && preferences.value.enabled)
        prepareIgor()
    }
    catch {
      igor.value = null
    }
  }
  function prepareIgor() {
    if (prepared)
      return
    prepared = true
    // Loading takes about a third of a second; do it before the first reply needs it.
    invoke('speech_prepare').catch(() => { prepared = false })
  }
  async function downloadIgor() {
    if (download.value)
      return
    error.value = ''
    download.value = { received: 0, total: igor.value?.download_bytes ?? 0 }
    const progress = new Channel<{ received: number, total: number }>()
    progress.onmessage = update => { download.value = update }
    try {
      igor.value = await invoke<SpeechStatus>('speech_download', { progress })
      prepareIgor()
    }
    catch (cause) {
      error.value = typeof cause === 'object' && cause && 'message' in cause ? String(cause.message) : 'Igor\'s voice could not be downloaded. Try again.'
    }
    finally {
      download.value = null
    }
  }
  function refreshVoices() {
    if (systemSupported)
      voices.value = synthesis.getVoices()
  }
  function stop() {
    generation++
    sentenceRequests = Promise.resolve()
    queue.length = 0
    current = undefined
    igorRunning = false
    speaking.value = false
    audio?.pause()
    endAudio?.()
    if (systemSupported)
      synthesis.cancel()
  }
  function synthesize(text: string, voice: IgorVoice) {
    return invoke<ArrayBuffer>('speak', { request: { text, voice: voice.id, speed: preferences.value.rate } })
  }
  function play(wav: ArrayBuffer, token: number) {
    return new Promise<void>((resolve, reject) => {
      if (generation !== token)
        return resolve()
      const url = URL.createObjectURL(new Blob([wav], { type: 'audio/wav' }))
      const element = new Audio(url)
      audio = element
      const finish = (failed: boolean) => {
        URL.revokeObjectURL(url)
        if (audio === element) {
          audio = undefined
          endAudio = undefined
        }
        if (failed)
          reject(new Error('playback failed'))
        else
          resolve()
      }
      endAudio = () => finish(false)
      element.onended = () => finish(false)
      element.onerror = () => finish(true)
      element.play().catch(() => finish(true))
    })
  }
  /** Plays queued sentences in order, synthesizing the next while the current one plays. */
  async function runIgor(voice: IgorVoice) {
    igorRunning = true
    speaking.value = true
    const token = generation
    const take = () => {
      const text = queue.shift()
      return text ? synthesize(text, voice) : undefined
    }
    try {
      let pending = take()
      while (pending) {
        const wav = await pending
        if (generation !== token)
          return
        const following = take()
        // A rejected look-ahead is reported when awaited, not as an unhandled rejection.
        following?.catch(() => {})
        await play(wav, token)
        if (generation !== token)
          return
        pending = following ?? take()
      }
    }
    catch (cause) {
      if (generation === token) {
        stop()
        error.value = typeof cause === 'object' && cause && 'message' in cause ? `Igor's voice could not speak: ${String(cause.message)}` : 'Igor\'s voice could not speak. Try another voice.'
      }
    }
    finally {
      if (generation === token) {
        igorRunning = false
        speaking.value = false
      }
    }
  }
  function next() {
    const text = queue.shift()
    if (!text) {
      current = undefined
      speaking.value = false
      return
    }
    const token = generation
    const utterance = new SpeechSynthesisUtterance(text)
    current = utterance; // Keep a strong reference while the engine speaks.
    utterance.voice = selectedVoice.value ?? null
    utterance.lang = selectedVoice.value?.lang ?? 'en'
    utterance.rate = preferences.value.rate
    utterance.onend = () => { if (generation === token)
      next(); }
    utterance.onerror = () => {
      if (generation !== token)
        return
      stop()
      error.value = 'Voice output could not finish. Try another voice.'
    }
    speaking.value = true
    try {
      synthesis.speak(utterance)
    }
    catch {
      stop()
      error.value = 'Voice output is unavailable in this window.'
    }
  }
  function speak(source: string, options: {
    interrupt?: boolean
    test?: boolean
  } = {}) {
    if (!preferences.value.enabled && !options.test)
      return
    const voice = igorVoice.value
    if (voice) {
      const text = stripMarkdown(source)
      if (!text)
        return
      if (options.interrupt)
        stop()
      error.value = ''
      const token = generation
      sentenceRequests = sentenceRequests.then(async () => {
        if (generation !== token)
          return
        const sentences = await invoke<string[]>('speech_sentences', { text })
        if (generation !== token)
          return
        queue.push(...sentences)
        if (!igorRunning && queue.length)
          void runIgor(voice)
      }).catch(() => {
        if (generation === token)
          error.value = "Igor's voice could not prepare this reply."
      })
      return
    }
    if (!systemSupported) {
      error.value = 'Voice output is unavailable in this window.'
      return
    }
    const text = stripMarkdown(source)
    if (!text)
      return
    if (options.interrupt)
      stop()
    error.value = ''
    // Short utterances avoid engine-specific long-text stalls; retain every word in order.
    const chunks = text.match(/.{1,240}(?:\s|$)|\S{1,240}/g) ?? [text]
    queue.push(...chunks.map(chunk => chunk.trim()).filter(Boolean))
    if (!current)
      next()
  }
  function connect() {
    users++
    if (users !== 1)
      return
    refreshVoices()
    void refreshIgor()
    if (systemSupported)
      synthesis.addEventListener('voiceschanged', refreshVoices)
  }
  function disconnect() {
    users--
    if (users)
      return
    if (systemSupported)
      synthesis.removeEventListener('voiceschanged', refreshVoices)
    stop()
  }
  const save = watch(preferences, value => {
    try {
      localStorage.setItem(voiceStorageKey, JSON.stringify(value))
    }
    catch {
      error.value = 'Voice preferences could not be saved on this device.'
    }
    if (!value.enabled)
      stop()
    else if (igor.value?.installed)
      prepareIgor()
  }, { deep: true, flush: 'sync' })
  function dispose() { save(); if (systemSupported)
    synthesis.removeEventListener('voiceschanged', refreshVoices); stop(); }
  return { preferences, voices, igor, igorVoice, download, speaking, selectedVoice, supported, systemSupported, error, speak, stop, connect, disconnect, dispose, downloadIgor }
}
export type VoiceOutput = ReturnType<typeof createVoiceOutput>
const voiceKey: InjectionKey<VoiceOutput> = Symbol('voice-output')
export function provideVoiceOutput() {
  const voice = createVoiceOutput()
  provide(voiceKey, voice)
  onScopeDispose(voice.dispose)
  return voice
}
export function useVoiceOutput() {
  const provided = inject(voiceKey, undefined)
  const voice = provided ?? createVoiceOutput()
  voice.connect()
  onScopeDispose(() => { voice.disconnect(); if (!provided)
    voice.dispose(); })
  return voice
}
