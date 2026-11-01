import { afterEach, expect, it } from 'vitest'
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks'
import { transcription } from './transcription'
afterEach(clearMocks)
it('prepares native recognition and sends plain 16 kHz mono samples', async () => {
  const calls: { command: string, args: unknown }[] = []
  mockIPC((command, args) => { calls.push({ command, args }); return command === 'transcribe' ? { text: 'Hello Igor', language: 'en' } : undefined })
  await transcription.prepare?.()
  expect(transcription.available).toBe(true)
  await expect(transcription.transcribe({ samples: new Float32Array([0.5]), sampleRate: 16000, channels: 1, language: null })).resolves.toEqual({ text: 'Hello Igor', language: 'en' })
  expect(calls).toEqual([
    { command: 'prepare_transcription', args: {} },
    { command: 'transcribe', args: { request: { samples: [0.5], sampleRate: 16000, channels: 1, language: null } } },
  ])
})
it('propagates preparation failure for a visible retry', async () => {
  mockIPC(() => Promise.reject('Speech model missing'))
  await expect(transcription.prepare?.()).rejects.toBe('Speech model missing')
})

it.each(['de', null])('sends language %s to the native command unchanged', async language => {
  let request: unknown
  mockIPC((command, args: any) => {
    if (command === 'transcribe') request = args.request
    return { text: 'Guten Tag', language: 'de' }
  })
  await expect(transcription.transcribe({ samples: new Float32Array([0.5]), sampleRate: 16000, channels: 1, language }))
    .resolves.toEqual({ text: 'Guten Tag', language: 'de' })
  expect(request).toEqual({ samples: [0.5], sampleRate: 16000, channels: 1, language })
})
