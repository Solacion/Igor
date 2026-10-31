import { Channel, invoke } from '@tauri-apps/api/core'

/** Core schema v1. Unknown event variants must be ignored by consumers. */
export interface CoreEvent {
  schema_version: number
  sequence: number
  event: { event: string; [key: string]: unknown }
}
export interface SessionInfo {
  session_id: string
  provider_id: string
  model: string
  servers: Array<{ server_id: string; connected: boolean; tool_count: number }>
}
export type TurnOutcome =
  | { outcome: 'completed' | 'cancelled' }
  | { outcome: 'limit_reached'; limit: string }
  | { outcome: 'failed'; message: string }

/** One channel per session; no global listeners or frontend tool execution. */
export async function startSession(
  providerId: string,
  onEvent: (event: CoreEvent) => void,
) {
  const events = new Channel<CoreEvent>()
  let listening = true
  events.onmessage = (event) => { if (listening) onEvent(event) }
  let info: SessionInfo
  try {
    info = await invoke<SessionInfo>('start_session', {
      request: { provider_id: providerId }, events,
    })
  } catch (error) {
    listening = false
    throw error
  }
  const request = { session_id: info.session_id }
  return {
    info,
    send: (text: string) => invoke<TurnOutcome>('send_message', { request: { ...request, text } }),
    approve: (approvalId: string, actionHash: string, decision: 'approve' | 'deny') =>
      invoke<void>('resolve_approval', { request: { ...request, approval_id: approvalId, action_hash: actionHash, decision } }),
    answer: (requestId: string, answer: string) =>
      invoke<void>('answer_clarification', { request: { ...request, request_id: requestId, answer } }),
    cancel: () => invoke<void>('cancel_turn', { request }),
    exportActivity: () => invoke<string>('export_activity', { request }),
    close: async () => {
      await invoke<void>('close_session', { request })
      listening = false
    },
  }
}

export interface ProviderView { id: string; model: string; unavailable: string | null }
export function readProviders() {
  return invoke<{ providers: ProviderView[] }>('conversation_providers')
}

export interface ApprovalPreview {
  approval_id: string
  call_id: string
  tool: { server: string; name: string }
  canonical_arguments: string
  action_hash: string
}
