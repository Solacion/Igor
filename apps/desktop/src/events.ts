import type { ApprovalPreview, TurnOutcome } from './session'
/** Transport is untrusted. Narrow its event before touching variant fields. */
export interface EventEnvelope {
  schema_version: number
  sequence: number
  event: unknown
}
export interface ToolId {
  server: string
  name: string
}
export type RuntimeEvent = {
  event: 'connection_changed'
  server: string
  state: 'connecting' | 'connected' | 'disconnected' | 'failed'
  detail?: string | null
} | {
  event: 'tools_changed'
  server: string
} | {
  event: 'tool_call_proposed'
  call_id: string
  tool: ToolId
  arguments: unknown
} | {
  event: 'tool_call_rejected'
  call_id: string
  alias?: string
  tool?: ToolId | null
  reason: string
} | {
  event: 'tool_call_started'
  call_id: string
} | {
  event: 'tool_call_completed'
  call_id: string
  is_error: boolean
  truncated?: boolean
} | {
  event: 'tool_call_failed'
  call_id: string
  outcome: 'not_executed' | 'unknown'
  message: string
} | {
  event: 'assistant_text'
  text: string
} | {
  event: 'approval_requested'
  request: ApprovalPreview
} | {
  event: 'approved_for_session'
  call_id: string
  tool: ToolId
} | {
  event: 'approval_resolved'
  approval_id: string
  call_id: string
  outcome: 'approved' | 'approved_for_session' | 'denied' | 'cancelled' | 'invalidated'
} | {
  event: 'clarification_requested'
  request_id: string
  question: string
} | {
  event: 'clarification_answered'
  request_id: string
} | {
  event: 'input_rejected'
  id: string
  reason: string
} | {
  event: 'turn_finished'
  outcome: unknown
}
export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}
function kind(value: unknown, event: string): value is Record<string, unknown> {
  return isRecord(value) && value.event === event
}
function tool(value: unknown): value is ToolId {
  return isRecord(value) && typeof value.server === 'string' && typeof value.name === 'string'
}
export function isApproval(value: unknown): value is ApprovalPreview {
  return isRecord(value) && typeof value.approval_id === 'string' && typeof value.call_id === 'string'
    && tool(value.tool) && typeof value.canonical_arguments === 'string' && typeof value.action_hash === 'string'
    && (value.session_grant_allowed === undefined || typeof value.session_grant_allowed === 'boolean')
}
export function isTurnOutcome(value: unknown): value is TurnOutcome {
  if (!isRecord(value))
    return false
  switch (value.outcome) {
    case 'completed':
    case 'cancelled': return true
    case 'completed_with_gaps': return typeof value.unsuccessful_calls === 'number' && Number.isSafeInteger(value.unsuccessful_calls) && value.unsuccessful_calls > 0
    case 'limit_reached': return typeof value.limit === 'string'
    case 'failed':
    case 'tools_unsupported': return typeof value.message === 'string'
    default: return false
  }
}
type Variant<K extends RuntimeEvent['event']> = Extract<RuntimeEvent, {
  event: K
}>
export function isConnectionChanged(v: unknown): v is Variant<'connection_changed'> {
  return kind(v, 'connection_changed') && typeof v.server === 'string' && ['connecting', 'connected', 'disconnected', 'failed'].includes(typeof v.state === 'string' ? v.state : '') && (v.detail == null || typeof v.detail === 'string')
}
export function isToolsChanged(v: unknown): v is Variant<'tools_changed'> {
  return kind(v, 'tools_changed') && typeof v.server === 'string'
}
export function isToolCallProposed(v: unknown): v is Variant<'tool_call_proposed'> {
  return kind(v, 'tool_call_proposed') && typeof v.call_id === 'string' && tool(v.tool) && 'arguments' in v
}
export function isToolCallRejected(v: unknown): v is Variant<'tool_call_rejected'> {
  return kind(v, 'tool_call_rejected') && typeof v.call_id === 'string' && (v.alias === undefined || typeof v.alias === 'string') && (v.tool == null || tool(v.tool)) && typeof v.reason === 'string'
}
export function isToolCallStarted(v: unknown): v is Variant<'tool_call_started'> {
  return kind(v, 'tool_call_started') && typeof v.call_id === 'string'
}
export function isToolCallCompleted(v: unknown): v is Variant<'tool_call_completed'> {
  return kind(v, 'tool_call_completed') && typeof v.call_id === 'string' && typeof v.is_error === 'boolean' && (v.truncated === undefined || typeof v.truncated === 'boolean')
}
export function isToolCallFailed(v: unknown): v is Variant<'tool_call_failed'> {
  return kind(v, 'tool_call_failed') && typeof v.call_id === 'string' && ['not_executed', 'unknown'].includes(typeof v.outcome === 'string' ? v.outcome : '') && typeof v.message === 'string'
}
export function isAssistantText(v: unknown): v is Variant<'assistant_text'> {
  return kind(v, 'assistant_text') && typeof v.text === 'string'
}
export function isApprovalRequested(v: unknown): v is Variant<'approval_requested'> {
  return kind(v, 'approval_requested') && isApproval(v.request)
}
export function isApprovedForSession(v: unknown): v is Variant<'approved_for_session'> {
  return kind(v, 'approved_for_session') && typeof v.call_id === 'string' && tool(v.tool)
}
export function isApprovalResolved(v: unknown): v is Variant<'approval_resolved'> {
  return kind(v, 'approval_resolved') && typeof v.approval_id === 'string' && typeof v.call_id === 'string' && ['approved', 'approved_for_session', 'denied', 'cancelled', 'invalidated'].includes(typeof v.outcome === 'string' ? v.outcome : '')
}
export function isClarificationRequested(v: unknown): v is Variant<'clarification_requested'> {
  return kind(v, 'clarification_requested') && typeof v.request_id === 'string' && typeof v.question === 'string'
}
export function isClarificationAnswered(v: unknown): v is Variant<'clarification_answered'> {
  return kind(v, 'clarification_answered') && typeof v.request_id === 'string'
}
export function isInputRejected(v: unknown): v is Variant<'input_rejected'> {
  return kind(v, 'input_rejected') && typeof v.id === 'string' && typeof v.reason === 'string'
}
export function isTurnFinished(v: unknown): v is Variant<'turn_finished'> {
  return kind(v, 'turn_finished') && 'outcome' in v
}
export const eventGuards = [isConnectionChanged, isToolsChanged, isToolCallProposed, isToolCallRejected, isToolCallStarted, isToolCallCompleted, isToolCallFailed, isAssistantText, isApprovalRequested, isApprovedForSession, isApprovalResolved, isClarificationRequested, isClarificationAnswered, isInputRejected, isTurnFinished]
export function parseEvent(value: unknown): RuntimeEvent | undefined {
  for (const guard of eventGuards) {
    if (guard(value))
      return value
  }
}
