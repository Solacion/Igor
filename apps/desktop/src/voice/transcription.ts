import { invoke } from '@tauri-apps/api/core'
/** Desktop capture contract, deliberately independent of AI/model providers. */
export interface TranscriptionAudio {
  samples: Float32Array
  sampleRate: 16000
  channels: 1
  language: string | null
}
export interface Transcript {
  text: string
  language: string | null
}
export interface SpeechLanguage {
  code: string
  name: string
}
export function readSpeechLanguages(): Promise<SpeechLanguage[]> {
  return invoke<SpeechLanguage[]>('speech_languages')
}
export function capitaliseLanguage(name: string): string {
  return name.charAt(0).toUpperCase() + name.slice(1)
}
export interface Transcriber {
  available: boolean
  prepare?: () => Promise<void>
  transcribe(audio: TranscriptionAudio): Promise<Transcript>
}
/** Native Whisper adapter. Audio stays on this computer. */
export const transcription: Transcriber = {
  available: true,
  prepare: () => invoke<void>('prepare_transcription'),
  transcribe: audio => invoke<Transcript>('transcribe', {
    request: { ...audio, samples: Array.from(audio.samples) },
  }),
}
