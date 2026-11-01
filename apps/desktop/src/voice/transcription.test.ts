import { afterEach, expect, it, vi } from 'vitest'
import { flushPromises } from '@vue/test-utils'
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks'
afterEach(() => { clearMocks(); vi.resetModules() })
it('sends 16 kHz mono samples through native transcribe and keeps empty transcripts empty', async () => {
  const calls: unknown[] = []
  mockIPC((command, args) => {
    if (command === 'transcription_status') return { installed: true, download_bytes: 574041195 }
    if (command === 'transcription_prepare') return null
    if (command === 'transcribe') { calls.push(args); return { text: '', language: null } }
  })
  const { transcription } = await import('./transcription')
  await transcription.refresh?.()
  expect(transcription.available).toBe(true)
  expect(await transcription.transcribe({ samples: new Float32Array([.5, -.5]), sampleRate: 16000, channels: 1, language: null })).toEqual({ text: '', language: null })
  expect(calls).toEqual([{ samples: [.5, -.5], language: null }])
})
it('disables capture before download and while the model prepares, loading only once', async () => {
  let installed = false
  let finish: (() => void) | undefined
  let prepares = 0
  mockIPC(command => {
    if (command === 'transcription_status') return { installed, download_bytes: 574041195 }
    if (command === 'transcription_prepare') { prepares++; return new Promise<void>(resolve => { finish = resolve }) }
  })
  const { transcription } = await import('./transcription')
  await transcription.refresh?.()
  expect(transcription.available).toBe(false)
  installed = true
  const ready = transcription.refresh?.()
  await flushPromises()
  expect(transcription.preparing).toBe(true)
  expect(transcription.available).toBe(false)
  finish?.()
  await ready
  await transcription.refresh?.()
  expect(transcription.available).toBe(true)
  expect(prepares).toBe(1)
})
it('downloads only after consent, reports progress, then prepares input', async () => {
  const commands: string[] = []
  mockIPC((command, args: any) => {
    commands.push(command)
    if (command === 'transcription_status') return { installed: false, download_bytes: 574041195 }
    if (command === 'transcription_download') {
      args.progress.onmessage({ received: 200, total: 574041195 })
      return { installed: true, download_bytes: 574041195 }
    }
  })
  const { transcription, downloadInput, inputProgress } = await import('./transcription')
  await transcription.refresh?.()
  expect(commands).toEqual(['transcription_status'])
  await downloadInput()
  expect(commands).toEqual(['transcription_status', 'transcription_download', 'transcription_prepare'])
  expect(inputProgress.value).toBeNull()
  expect(transcription.available).toBe(true)
})
it('recovers from a failed preparation on retry', async () => {
  let attempts = 0
  mockIPC(command => {
    if (command === 'transcription_status') return { installed: true, download_bytes: 574041195 }
    if (command === 'transcription_prepare' && attempts++ === 0) throw new Error('load failed')
  })
  const { transcription, inputError } = await import('./transcription')
  await transcription.refresh?.()
  expect(transcription.available).toBe(false)
  expect(inputError.value).toContain('could not load')
  await transcription.refresh?.()
  expect(transcription.available).toBe(true)
  expect(inputError.value).toBe('')
})

it.each(['de', null])('sends language %s to the native command unchanged', async language => {
  let request: unknown
  mockIPC((command, args: any) => {
    if (command === 'transcribe') request = args
    return { text: 'Guten Tag', language: 'de' }
  })
  const { transcription } = await import('./transcription')
  await expect(transcription.transcribe({ samples: new Float32Array([0.5]), sampleRate: 16000, channels: 1, language }))
    .resolves.toEqual({ text: 'Guten Tag', language: 'de' })
  expect(request).toEqual({ samples: [0.5], language })
})

it('times out a stuck native preparation and permits retry', async () => {
  vi.useFakeTimers()
  try {
    let attempts = 0
    mockIPC(command => {
      if (command === 'transcription_status') return { installed: true, download_bytes: 574041195 }
      if (command === 'transcription_prepare' && attempts++ === 0) return new Promise(() => {})
    })
    const { transcription, inputError } = await import('./transcription')
    const pending = transcription.refresh?.()
    await vi.advanceTimersByTimeAsync(60000)
    await pending
    expect(transcription.preparing).toBe(false)
    expect(transcription.available).toBe(false)
    expect(inputError.value).toContain('could not load')
    await transcription.refresh?.()
    expect(transcription.available).toBe(true)
  }
  finally { vi.useRealTimers() }
})
