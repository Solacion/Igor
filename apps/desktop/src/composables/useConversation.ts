import { updateActivity, type Activity } from '../activity'
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { parseEvent } from '../events'
import { useSettingsStore } from '../stores/settings'
import { startSession, type ApprovalPreview, type CoreEvent } from '../session'
export interface QuestionItem {
  key: string
  id: string
  question: string
  answer: string
  state: string
}
export interface ApprovalItem {
  key: string
  request: ApprovalPreview
  state: string
}
export function useConversation() {
  const store = useSettingsStore()
  const providers = store.providers
  const providerId = ref('')
  const servers = computed(() => store.settings.value?.servers ?? [])
  const loaded = ref(false)
  const busy = ref(false)
  const text = ref('')
  const error = ref('')
  const notice = ref('')
  const status = ref('Ready for a message')
  const toolsUnsupported = ref(false)
  const exportNotice = ref('')
  const exporting = ref(false)
  const hasSession = ref(false)
  const questions = ref<Array<{
    key: string
    id: string
    question: string
    answer: string
    state: string
  }>>([])
  const approvals = ref<Array<{
    key: string
    request: ApprovalPreview
    state: string
  }>>([])
  const connections = ref<Array<{
    key: string
    server: string
    state: string
    detail: string
  }>>([])
  const activity = ref<Record<string, Activity>>({})
  let epoch = 0
  const messages = ref<Array<{
    role: string
    text: string
  }>>([])
  const selected = computed(() => providers.value.find(p => p.id === providerId.value))
  let session: Awaited<ReturnType<typeof startSession>> | null = null
  let sessionRevision: string | undefined
  let lastSequence = -1
  let disposed = false
  let refreshing = false
  let refreshAgain = false
  let stopRequested = false
  async function refresh(afterEvent = false) {
    if (refreshing) {
      refreshAgain ||= afterEvent
      return
    }
    refreshing = true
    try {
      const [options] = await Promise.all([store.refreshProviders(), store.refreshSettings()])
      if (disposed)
        return
      providers.value = options.providers
      if (!options.providers.some(provider => provider.id === providerId.value))
        providerId.value = options.providers[0]?.id ?? ''
      loaded.value = true
    }
    catch {
      if (!disposed) {
        loaded.value = false
        error.value = 'Could not load conversation settings. Check your configuration and try again.'
      }
    }
    finally {
      refreshing = false
      if (refreshAgain && !disposed) {
        refreshAgain = false
        void refresh()
      }
    }
  }
  function finish(value: unknown) {
    const outcome = value && typeof value === 'object' ? value as Record<string, unknown> : {}
    for (const item of questions.value)
      if (item.state === 'pending' || item.state === 'sending')
        item.state = 'Turn ended'
    for (const item of approvals.value)
      if (item.state === 'pending' || item.state === 'sending')
        item.state = 'Turn ended'
    toolsUnsupported.value = outcome.outcome === 'tools_unsupported'
    switch (outcome.outcome) {
      case 'completed':
        status.value = 'Completed'
        break
      case 'completed_with_gaps':
        status.value = typeof outcome.unsuccessful_calls === 'number' && Number.isSafeInteger(outcome.unsuccessful_calls) && outcome.unsuccessful_calls > 0
          ? `The answer may be incomplete: ${outcome.unsuccessful_calls} tool calls did not succeed.`
          : 'The answer may be incomplete: some tool calls did not succeed.'
        break
      case 'cancelled':
        status.value = 'Cancelled — the answer may be incomplete'
        break
      case 'limit_reached':
        status.value = `Stopped at limit: ${outcome.limit}. The answer may be incomplete.`
        break
      case 'tools_unsupported':
        {
          const message = typeof outcome.message === 'string' ? outcome.message.trim() : 'The model cannot call tools'
          status.value = /[.!?]$/.test(message) ? message : `${message}.`
        }
        break
      case 'failed':
        status.value = `Turn failed: ${outcome.message}`
        break
      default:
        status.value = 'Not a full answer — the runtime returned an unrecognized outcome.'
        break
    }
  }
  function receive(envelope: CoreEvent) {
    if (disposed || envelope.sequence <= lastSequence)
      return
    if (envelope.schema_version !== 1 || envelope.sequence !== lastSequence + 1) {
      error.value = 'The event stream was interrupted. Stop this turn and start a new conversation.'
      void session?.cancel().catch(() => { })
      return
    }
    lastSequence = envelope.sequence
    const event = parseEvent(envelope.event)
    if (!event)
      return
    updateActivity(activity.value, event, epoch)
    if (event.event === 'connection_changed' && typeof event.server === 'string' && typeof event.state === 'string') {
      connections.value.push({ key: `${epoch}:${envelope.sequence}`, server: event.server, state: event.state, detail: typeof event.detail === 'string' ? event.detail : '' })
      void refresh(true)
    }
    if (event.event === 'tools_changed' && typeof event.server === 'string') {
      connections.value.push({ key: `${epoch}:${envelope.sequence}`, server: event.server, state: 'tools_changed', detail: '' })
      void refresh(true)
    }
    if (event.event === 'approval_requested') {
      const request = event.request
      approvals.value.push({ key: `${epoch}:${request.approval_id}`, request, state: 'pending' })
      status.value = 'Waiting for your approval'
    }
    if (event.event === 'approval_resolved') {
      const item = approvals.value.find(item => item.key === `${epoch}:${String(event.approval_id)}`)
      if (item)
        item.state = String(event.outcome)
      status.value = 'Working…'
    }
    if (event.event === 'clarification_requested' && typeof event.question === 'string' && typeof event.request_id === 'string') {
      questions.value.push({ key: `${epoch}:${event.request_id}`, id: event.request_id, question: event.question, answer: '', state: 'pending' })
      status.value = 'Waiting for your answer'
    }
    if (event.event === 'clarification_answered') {
      const item = questions.value.find(item => item.key === `${epoch}:${String(event.request_id)}`)
      if (item) {
        item.state = 'Answered'
        messages.value.push({ role: 'You', text: item.answer })
      }
      status.value = 'Working…'
    }
    if (event.event === 'input_rejected') {
      const question = questions.value.find(item => item.key === `${epoch}:${String(event.id)}`)
      if (question?.state === 'sending')
        question.state = 'pending'
      const item = approvals.value.find(item => item.key === `${epoch}:${String(event.id)}`)
      if (item?.state === 'sending')
        item.state = 'pending'
      error.value = `Your response was refused: ${String(event.reason)}`
    }
    if (event.event === 'assistant_text' && typeof event.text === 'string')
      messages.value.push({ role: 'Igor', text: event.text })
    if (event.event === 'turn_finished')
      finish(event.outcome)
  }
  async function prepare() {
    // Read after a Settings save even when an older background poll is still in flight.
    const latest = await store.refreshProviders()
    providers.value = latest.providers
    if (!latest.providers.some(provider => provider.id === providerId.value))
      providerId.value = latest.providers[0]?.id ?? ''
    const chosen = latest.providers.find(provider => provider.id === providerId.value)
    if (!chosen || chosen.unavailable)
      throw new Error('Provider unavailable')
    if (session && (session.info.provider_id !== providerId.value || sessionRevision !== chosen.revision)) {
      await session.close()
      session = null
      notice.value = 'Provider changed. A new model conversation starts here; earlier messages remain visible.'
    }
    if (!session) {
      epoch++
      lastSequence = -1
      session = await startSession(providerId.value, receive)
      sessionRevision = chosen.revision
    }
    return session
  }
  async function send() {
    const message = text.value.trim()
    if (!message || busy.value || !loaded.value || !selected.value || selected.value.unavailable)
      return
    busy.value = true
    toolsUnsupported.value = false
    error.value = ''
    stopRequested = false
    status.value = 'Working…'
    try {
      await prepare()
      if (stopRequested || disposed) {
        await session?.close()
        session = null
        status.value = 'Cancelled'
        return
      }
      const active = session
      if (!active)
        return
      messages.value.push({ role: 'You', text: message })
      text.value = ''
      const outcome = await active.send(message)
      finish(outcome)
    }
    catch {
      error.value = 'The turn could not finish. Check the provider and server settings before sending another message.'
      status.value = 'Turn failed — the answer may be incomplete'
      if (session) {
        try {
          await session.close()
        }
        catch { /* Keep diagnostics native. */ }
        session = null
      }
    }
    finally {
      busy.value = false
      hasSession.value = session !== null
      for (const item of [...questions.value, ...approvals.value]) {
        if (item.state === 'pending' || item.state === 'sending')
          item.state = 'Turn ended'
      }
    }
  }
  async function answer(item: (typeof questions.value)[number]) {
    if (!session || !busy.value || item.state !== 'pending' || !item.answer.trim())
      return
    item.state = 'sending'
    error.value = ''
    try {
      await session.answer(item.id, item.answer)
    }
    catch {
      item.state = 'pending'
      error.value = 'Could not submit your answer. Try again or stop the turn.'
    }
  }
  async function decide(item: (typeof approvals.value)[number], decision: 'approve' | 'deny') {
    if (!busy.value || item.state !== 'pending' || !session)
      return
    item.state = 'sending'
    error.value = ''
    try {
      await session.approve(item.request.approval_id, item.request.action_hash, decision)
    }
    catch {
      item.state = 'pending'
      error.value = 'Could not submit your decision. Nothing is assumed approved; try again or stop the turn.'
    }
  }
  async function exportActivity() {
    if (!session || busy.value || exporting.value)
      return
    exporting.value = true
    exportNotice.value = ''
    try {
      exportNotice.value = `Activity saved to ${await session.exportActivity()}`
    }
    catch {
      exportNotice.value = 'Could not export the complete activity. Try again.'
    }
    finally {
      exporting.value = false
    }
  }
  async function stop() {
    stopRequested = true
    try {
      await session?.cancel()
      status.value = 'Stopping…'
    }
    catch {
      error.value = 'Could not stop the turn. Try again.'
    }
  }
  async function reset() {
    if (busy.value)
      return
    try {
      await session?.close()
      session = null
      messages.value = []
      activity.value = {}
      connections.value = []
      approvals.value = []
      questions.value = []
      notice.value = ''
      error.value = ''
      status.value = 'Ready for a message'
      toolsUnsupported.value = false
      hasSession.value = false
      exportNotice.value = ''
    }
    catch {
      error.value = 'Could not close the conversation. Try again.'
    }
  }
  const unsubscribe = store.subscribe(() => { void refresh(true); })
  onMounted(() => { void refresh(); })
  onUnmounted(() => {
    unsubscribe()
    disposed = true
    void session?.close().catch(() => { })
  })
  return { providers, providerId, servers, loaded, busy, text, error, notice, status, toolsUnsupported, exportNotice, exporting, hasSession, questions, approvals, connections, activity, messages, selected, refresh, finish, receive, send, answer, decide, exportActivity, stop, reset }
}
