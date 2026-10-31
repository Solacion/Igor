import type { RuntimeEvent } from './events'
export interface Activity {
  callId: string
  server: string
  tool: string
  arguments?: unknown
  state: string
  detail: string
}
/** Only core events affect activity. This display never dispatches tools. */
export function updateActivity(rows: Record<string, Activity>, event: RuntimeEvent, epoch: number) {
  if (!('call_id' in event) || !event.event.startsWith('tool_call_'))
    return
  const key = `${epoch}:${event.call_id}`
  const tool = 'tool' in event ? event.tool : undefined
  const row = rows[key] ??= { callId: event.call_id, server: tool?.server ?? 'Unknown server', tool: tool?.name ?? ('alias' in event ? event.alias ?? 'Unknown tool' : 'Unknown tool'), state: 'Proposed', detail: '' }
  switch (event.event) {
    case 'tool_call_proposed':
      row.arguments = event.arguments
      row.state = 'Proposed'
      break
    case 'tool_call_rejected':
      row.state = 'Refused'
      row.detail = `${String(event.reason)}. Nothing was sent.`
      break
    case 'tool_call_started':
      row.state = 'Started'
      break
    case 'tool_call_completed':
      row.state = event.is_error ? 'Server reported an error' : 'Completed'
      row.detail = event.truncated ? 'Result was truncated at the size limit.' : ''
      break
    case 'tool_call_failed':
      row.state = 'Failed'
      row.detail = `${String(event.message)}. ${event.outcome === 'unknown' ? 'The action may have run; its effect is unknown. Check before retrying.' : 'The action did not run.'}`
      break
  }
}
