import { expect, it } from 'vitest'
import { transcription } from './transcription'
it('keeps the shipping adapter explicitly unavailable until a native Whisper contract is supplied', async () => {
  expect(transcription.available).toBe(false)
  await expect(transcription.transcribe({ samples: new Float32Array(), sampleRate: 16000, channels: 1 })).rejects.toThrow('not available in this build')
})
