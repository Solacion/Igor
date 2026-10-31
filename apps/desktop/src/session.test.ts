import { afterEach, expect, it, vi } from 'vitest'
import { Channel } from '@tauri-apps/api/core'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import { startSession, type CoreEvent } from './session'

afterEach(clearMocks)

it('scopes every command to its session and echoes the displayed approval hash', async () => {
  const calls: Array<[string, unknown]> = []
  let channel: Channel<CoreEvent> | undefined
  mockIPC((command, args) => {
    calls.push([command, args])
    if (command === 'start_session') {
      channel = (args as Record<string, unknown>)?.events as Channel<CoreEvent>
      return { session_id: 'desktop-1', provider_id: 'local', model: 'demo', servers: [] }
    }
    if (command === 'send_message') return { outcome: 'completed' }
  })
  const receive = vi.fn()
  const session = await startSession('local', receive)
  const event: CoreEvent = { schema_version: 1, sequence: 0, event: { event: 'future_variant' } }
  channel?.onmessage(event)
  expect(receive).toHaveBeenCalledWith(event)
  expect(await session.send('Hello')).toEqual({ outcome: 'completed' })
  await session.approve('approval-1', 'a'.repeat(64), 'deny')
  await session.answer('question-1', 'Tomorrow')
  await session.exportActivity()
  await session.cancel()
  await session.close()
  channel?.onmessage(event)
  expect(receive).toHaveBeenCalledTimes(1)
  expect(calls.map(([name]) => name)).toEqual(['start_session', 'send_message', 'resolve_approval', 'answer_clarification', 'export_activity', 'cancel_turn', 'close_session'])
  expect(calls[2]?.[1]).toEqual({ request: { session_id: 'desktop-1', approval_id: 'approval-1', action_hash: 'a'.repeat(64), decision: 'deny' } })
  expect(calls[4]?.[1]).toEqual({ request: { session_id: 'desktop-1' } })
  expect(calls[3]?.[1]).toEqual({ request: { session_id: 'desktop-1', request_id: 'question-1', answer: 'Tomorrow' } })
})

it('stops delivering events when startup fails', async () => {
  let channel: Channel<CoreEvent> | undefined
  mockIPC((_command, args) => { channel = (args as Record<string, unknown>)?.events as Channel<CoreEvent>; throw { code: 'configuration' } })
  const receive = vi.fn()
  await expect(startSession('local', receive)).rejects.toEqual({ code: 'configuration' })
  channel?.onmessage({ schema_version: 1, sequence: 0, event: { event: 'assistant_text', text: 'late' } })
  expect(receive).not.toHaveBeenCalled()
})
