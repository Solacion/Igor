import { isRecord } from './events'
import type { Activity } from './activity'
import type { ApprovalPreview } from './session'
import type { ApprovalItem, QuestionItem } from './composables/useConversation'
export interface Message { role: string; text: string }
export interface Connection { key: string; server: string; state: string; detail: string }
export type TimelineEntry =
  | { kind: 'message'; key: string; message: Message }
  | { kind: 'tool'; key: string; call: Activity }
  | { kind: 'connection'; key: string; connection: Connection }
  | { kind: 'approval'; key: string; item: ApprovalItem }
  | { kind: 'question'; key: string; item: QuestionItem }

/** Display-only parsing: preserve number lexemes, including integers above JS's safe range. */
export function previewArguments(canonical: string): Record<string, unknown> | undefined {
  try {
    const lossless = canonical.replace(/"(?:\\.|[^"\\])*"|-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?/g, token => token.startsWith('"') ? token : JSON.stringify(token))
    const value: unknown = JSON.parse(lossless)
    return isRecord(value) ? value : undefined
  } catch { return undefined }
}
export function displayValue(value: unknown): string {
  if (typeof value === 'string') return value
  return JSON.stringify(value) ?? 'Not provided'
}
export function approvalSummary(request: ApprovalPreview): string {
  const args = previewArguments(request.canonical_arguments)
  if (request.tool.name === 'create_draft_order') {
    const supplier = typeof args?.supplier_id === 'string' ? ` for ${args.supplier_id}` : ''
    const date = typeof args?.delivery_date === 'string' ? `, delivery ${args.delivery_date}` : ''
    return `Create a draft order${supplier}${date} using ${request.tool.server}.`
  }
  return `Allow ${request.tool.server} to run ${request.tool.name}?`
}
export function toolSummary(call: Activity): string {
  const labels: Record<string, string> = { get_stock_levels: 'Checked stock', get_sales_history: 'Checked sales', get_catalogue: 'Checked catalogue', list_notes: 'Read notes', create_draft_order: 'Draft order' }
  const label = labels[call.tool] ?? call.tool.replaceAll('_', ' ')
  const indicator = call.state === 'Completed' ? '✓' : ['Failed', 'Refused', 'Server reported an error'].includes(call.state) ? '!' : '…'
  return `${label} · ${call.server} ${indicator}`
}
