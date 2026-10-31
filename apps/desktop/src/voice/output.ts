import { computed, inject, onScopeDispose, provide, ref, watch, type InjectionKey } from 'vue'
export interface VoicePreferences {
  enabled: boolean
  voiceURI: string
  rate: number
  reviewBeforeSending: boolean
}
export const voiceStorageKey = 'igor.desktop.voice.v1'
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
  const speaking = ref(false)
  const error = ref('')
  const supported = !!synthesis && typeof globalThis.SpeechSynthesisUtterance === 'function'
  const selectedVoice = computed(() => voices.value.find(voice => voice.voiceURI === preferences.value.voiceURI) ?? bestVoice(voices.value))
  let users = 0
  let generation = 0
  const queue: string[] = []
  let current: SpeechSynthesisUtterance | undefined
  function refreshVoices() {
    if (supported)
      voices.value = synthesis.getVoices()
  }
  function stop() {
    generation++
    queue.length = 0
    current = undefined
    speaking.value = false
    if (supported)
      synthesis.cancel()
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
    if (!supported) {
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
    if (supported)
      synthesis.addEventListener('voiceschanged', refreshVoices)
  }
  function disconnect() {
    users--
    if (users)
      return
    if (supported)
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
  }, { deep: true, flush: 'sync' })
  function dispose() { save(); if (supported)
    synthesis.removeEventListener('voiceschanged', refreshVoices); stop(); }
  return { preferences, voices, speaking, selectedVoice, supported, error, speak, stop, connect, disconnect, dispose }
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
