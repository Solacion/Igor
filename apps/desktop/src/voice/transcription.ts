import { ref } from 'vue'
import { Channel, invoke } from '@tauri-apps/api/core'
/** Capture is resampled in the webview before crossing IPC. */
export interface TranscriptionAudio {
  samples: Float32Array
  sampleRate: 16000
  channels: 1
}
export interface Transcriber {
  readonly available: boolean
  readonly preparing?: boolean
  refresh?(): Promise<void>
  transcribe(audio: TranscriptionAudio): Promise<string>
}
interface InputStatus { installed: boolean; download_bytes: number }
export const inputStatus = ref<InputStatus | null>(null)
export const inputPreparing = ref(false)
export const inputProgress = ref<{ received: number; total: number } | null>(null)
export const inputError = ref('')
let preparation: Promise<void> | undefined
function prepare() {
  if (preparation) return preparation
  inputError.value = ''
  inputPreparing.value = true
  preparation = invoke<void>('transcription_prepare').catch(() => {
    preparation = undefined
    inputError.value = 'Speech input could not load. Try again or type your message.'
  }).finally(() => { inputPreparing.value = false })
  return preparation
}
export async function refreshInput() {
  try {
    inputStatus.value = await invoke<InputStatus>('transcription_status')
    if (inputStatus.value?.installed) await prepare()
  }
  catch { inputStatus.value = null }
}
/** Only called by the explicit download button after its size/privacy explanation. */
export async function downloadInput() {
  if (inputProgress.value) return
  inputError.value = ''
  inputProgress.value = { received: 0, total: inputStatus.value?.download_bytes ?? 574_041_195 }
  const progress = new Channel<{ received: number; total: number }>()
  progress.onmessage = update => { inputProgress.value = update }
  try {
    inputStatus.value = await invoke<InputStatus>('transcription_download', { progress })
    await prepare()
  }
  catch { inputError.value = 'Speech input could not be downloaded. Try again.' }
  finally { inputProgress.value = null }
}
export const transcription: Transcriber = {
  get available() { return inputStatus.value?.installed === true && !inputPreparing.value && !inputError.value },
  get preparing() { return inputPreparing.value },
  refresh: refreshInput,
  async transcribe(audio) {
    if (audio.sampleRate !== 16000 || audio.channels !== 1)
      throw new Error('Expected 16 kHz mono speech')
    const result = await invoke<{ text: string }>('transcribe', { samples: Array.from(audio.samples) })
    return result.text
  },
}
