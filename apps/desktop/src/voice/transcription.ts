/** Desktop capture contract, deliberately independent of AI/model providers. */
export interface TranscriptionAudio {
  samples: Float32Array
  sampleRate: 16000
  channels: 1
}
export interface Transcriber {
  available: boolean
  transcribe(audio: TranscriptionAudio): Promise<string>
}
/** Replace only this adapter after the native Whisper `transcribe` IPC contract lands.
* Do not invent an IPC payload or expose an unrestricted native capability in the meantime. */
export const transcription: Transcriber = {
  available: false,
  async transcribe() { throw new Error('Voice input is not available in this build. You can still type.'); },
}
