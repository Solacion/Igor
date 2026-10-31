<script setup lang="ts">
import { updateActivity, type Activity } from './activity'
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { readSettings, type ServerView } from './bridge'
import { readProviders, startSession, type ApprovalPreview, type CoreEvent, type ProviderView, type TurnOutcome } from './session'

const emit = defineEmits<{ settings: [] }>()
const providers = ref<ProviderView[]>([])
const providerId = ref('')
const servers = ref<ServerView[]>([])
const loaded = ref(false)
const busy = ref(false)
const text = ref('')
const error = ref('')
const notice = ref('')
const status = ref('Ready for a message')
const questions = ref<Array<{ key: string; id: string; question: string; answer: string; state: string }>>([])
const approvals = ref<Array<{ key: string; request: ApprovalPreview; state: string }>>([])
const connections = ref<Array<{ key: string; server: string; state: string; detail: string }>>([])
const activity = ref<Record<string, Activity>>({})
let epoch = 0
const messages = ref<Array<{ role: string; text: string }>>([])
const selected = computed(() => providers.value.find(p => p.id === providerId.value))
let session: Awaited<ReturnType<typeof startSession>> | null = null
let lastSequence = -1
let timer: ReturnType<typeof setInterval> | undefined
let disposed = false
let refreshing = false
let refreshAgain = false
let stopRequested = false

async function refresh(afterEvent = false) {
  if (refreshing) { refreshAgain ||= afterEvent; return }
  refreshing = true
  try {
    const [options, settings] = await Promise.all([readProviders(), readSettings()])
    if (disposed) return
    providers.value = options.providers
    servers.value = settings.servers
    if (!providerId.value) providerId.value = options.providers[0]?.id ?? ''
    loaded.value = true
  } catch {
    if (!disposed) { loaded.value = false; error.value = 'Could not load conversation settings. Check your configuration and try again.' }
  } finally {
    refreshing = false
    if (refreshAgain && !disposed) { refreshAgain = false; void refresh() }
  }
}
function finish(value: unknown) {
  const outcome = value && typeof value === 'object' ? value as Record<string, unknown> : {}
  for (const item of questions.value) if (item.state === 'pending' || item.state === 'sending') item.state = 'Turn ended'
  for (const item of approvals.value) if (item.state === 'pending' || item.state === 'sending') item.state = 'Turn ended'
  switch (outcome.outcome) {
    case 'completed': status.value = 'Completed'; break
    case 'completed_with_gaps':
      status.value = typeof outcome.unsuccessful_calls === 'number' && Number.isSafeInteger(outcome.unsuccessful_calls) && outcome.unsuccessful_calls > 0
        ? `The answer may be incomplete: ${outcome.unsuccessful_calls} tool calls did not succeed.`
        : 'The answer may be incomplete: some tool calls did not succeed.'
      break
    case 'cancelled': status.value = 'Cancelled — the answer may be incomplete'; break
    case 'limit_reached': status.value = `Stopped at limit: ${outcome.limit}. The answer may be incomplete.`; break
    case 'failed': status.value = `Turn failed: ${outcome.message}`; break
    default: status.value = 'Not a full answer — the runtime returned an unrecognized outcome.'; break
  }
}
function receive(envelope: CoreEvent) {
  if (disposed || envelope.sequence <= lastSequence) return
  if (envelope.schema_version !== 1 || envelope.sequence !== lastSequence + 1) {
    error.value = 'The event stream was interrupted. Stop this turn and start a new conversation.'
    void session?.cancel().catch(() => {})
    return
  }
  lastSequence = envelope.sequence
  const event = envelope.event
  updateActivity(activity.value, event, epoch)
  if (event.event === 'connection_changed' && typeof event.server === 'string' && typeof event.state === 'string') {
    connections.value.push({ key: `${epoch}:${envelope.sequence}`, server: event.server, state: event.state, detail: typeof event.detail === 'string' ? event.detail : '' })
    void refresh(true)
  }
  if (event.event === 'approval_requested') {
    const request = event.request as unknown as ApprovalPreview
    approvals.value.push({ key: `${epoch}:${request.approval_id}`, request, state: 'pending' })
    status.value = 'Waiting for your approval'
  }
  if (event.event === 'approval_resolved') {
    const item = approvals.value.find(item => item.key === `${epoch}:${String(event.approval_id)}`)
    if (item) item.state = String(event.outcome)
    status.value = 'Working…'
  }
  if (event.event === 'clarification_requested' && typeof event.question === 'string' && typeof event.request_id === 'string') {
    questions.value.push({ key: `${epoch}:${event.request_id}`, id: event.request_id, question: event.question, answer: '', state: 'pending' })
    status.value = 'Waiting for your answer'
  }
  if (event.event === 'clarification_answered') {
    const item = questions.value.find(item => item.key === `${epoch}:${String(event.request_id)}`)
    if (item) { item.state = 'Answered'; messages.value.push({ role: 'You', text: item.answer }) }
    status.value = 'Working…'
  }
  if (event.event === 'input_rejected') {
    const question = questions.value.find(item => item.key === `${epoch}:${String(event.id)}`)
    if (question?.state === 'sending') question.state = 'pending'
    const item = approvals.value.find(item => item.key === `${epoch}:${String(event.id)}`)
    if (item?.state === 'sending') item.state = 'pending'
    error.value = `Your response was refused: ${String(event.reason)}`
  }
  if (event.event === 'assistant_text' && typeof event.text === 'string') messages.value.push({ role: 'Igor', text: event.text })
  if (event.event === 'turn_finished') finish(event.outcome as TurnOutcome)
}
async function prepare() {
  if (session && session.info.provider_id !== providerId.value) {
    await session.close(); session = null
    notice.value = 'Provider changed. A new model conversation starts here; earlier messages remain visible.'
  }
  if (!session) { epoch++; lastSequence = -1; session = await startSession(providerId.value, receive) }
  return session
}
async function send() {
  const message = text.value.trim()
  if (!message || busy.value || !loaded.value || !selected.value || selected.value.unavailable) return
  busy.value = true; error.value = ''; stopRequested = false; status.value = 'Working…'
  try {
    await prepare()
    if (stopRequested || disposed) { await session?.close(); session = null; status.value = 'Cancelled'; return }
    const active = session
    if (!active) return
    messages.value.push({ role: 'You', text: message }); text.value = ''
    let outcome: TurnOutcome
    try { outcome = await active.send(message) }
    catch (failure) {
      if ((failure as { code?: string })?.code !== 'settings_changed') throw failure
      await active.close(); session = null
      notice.value = 'Settings changed. A new model conversation starts here; earlier messages remain visible.'
      await refresh(); const replacement = await prepare()
      if (!replacement || stopRequested || disposed) { status.value = 'Cancelled'; return }
      outcome = await replacement.send(message)
    }
    finish(outcome)
  } catch {
    error.value = 'The turn could not finish. Check the provider and server settings before sending another message.'
    status.value = 'Turn failed — the answer may be incomplete'
    if (session) { try { await session.close() } catch { /* Keep diagnostics native. */ } session = null }
  } finally {
    busy.value = false
    for (const item of [...questions.value, ...approvals.value]) {
      if (item.state === 'pending' || item.state === 'sending') item.state = 'Turn ended'
    }
  }
}
async function answer(item: (typeof questions.value)[number]) {
  if (!session || !busy.value || item.state !== 'pending' || !item.answer.trim()) return
  item.state = 'sending'; error.value = ''
  try { await session.answer(item.id, item.answer) }
  catch { item.state = 'pending'; error.value = 'Could not submit your answer. Try again or stop the turn.' }
}
async function decide(item: (typeof approvals.value)[number], decision: 'approve' | 'deny') {
  if (!busy.value || item.state !== 'pending' || !session) return
  item.state = 'sending'; error.value = ''
  try { await session.approve(item.request.approval_id, item.request.action_hash, decision) }
  catch { item.state = 'pending'; error.value = 'Could not submit your decision. Nothing is assumed approved; try again or stop the turn.' }
}
async function stop() {
  stopRequested = true
  try { await session?.cancel(); status.value = 'Stopping…' }
  catch { error.value = 'Could not stop the turn. Try again.' }
}
async function reset() {
  if (busy.value) return
  try {
    await session?.close(); session = null; messages.value = []; activity.value = {}; connections.value = []; approvals.value = []; questions.value = []; notice.value = ''; error.value = ''; status.value = 'Ready for a message'
  } catch { error.value = 'Could not close the conversation. Try again.' }
}
onMounted(() => { void refresh(); timer = setInterval(() => { void refresh() }, 3000) })
onUnmounted(() => { disposed = true; clearInterval(timer); void session?.close().catch(() => {}) })
</script>

<template>
  <section class="conversation" aria-labelledby="conversation-title">
    <div class="conversation-heading">
      <div><p class="eyebrow">Your models. Your tools. Your control.</p><h1 id="conversation-title">Conversation</h1></div>
      <button class="secondary" :disabled="busy" @click="reset">New conversation</button>
    </div>
    <label for="provider">Model provider</label>
    <select id="provider" v-model="providerId" :disabled="busy || !loaded">
      <option v-for="provider in providers" :key="provider.id" :value="provider.id">{{ provider.id }} · {{ provider.model }}</option>
    </select>
    <p v-if="loaded && !providers.length">No providers configured. Add a provider to your Igor configuration, then restart Igor.</p>
    <p v-if="selected?.unavailable" role="alert">{{ selected.unavailable }}</p>
    <div class="conversation-servers" aria-label="Server connections">
      <span v-for="server in servers" :key="server.entry.id">{{ server.entry.id }}: {{ server.status.state.replaceAll('_', ' ') }}</span>
      <p v-if="loaded && !servers.length">No MCP servers configured. <button class="secondary" @click="emit('settings')">Open Settings</button></p>
    </div>
    <p v-if="notice" role="status">{{ notice }}</p>
    <ol class="messages" aria-label="Conversation messages" aria-live="polite" aria-relevant="additions">
      <li v-for="(message, index) in messages" :key="index"><strong>{{ message.role }}</strong><p class="literal">{{ message.text }}</p></li>
    </ol>
    <section v-if="Object.keys(activity).length || connections.length" class="tool-activity" aria-labelledby="activity-title">
      <h2 id="activity-title">Tool activity</h2>
      <ol class="connection-events"><li v-for="connection in connections" :key="connection.key" class="activity-row">
        <strong>{{ connection.server }}</strong>
        <p>{{ connection.state === 'failed' ? 'Server stopped; later calls are not executed.' : connection.state === 'disconnected' ? 'Server disconnected.' : connection.state === 'connecting' ? 'Server connecting…' : connection.state === 'connected' ? 'Server connected.' : connection.state }}</p>
        <p v-if="connection.detail" class="literal">{{ connection.detail }}</p>
      </li></ol>
      <ol><li v-for="(call, key) in activity" :key="key" class="activity-row">
        <strong>{{ call.server }} · {{ call.tool }}</strong><p>{{ call.state }}</p>
        <p v-if="call.detail" class="literal">{{ call.detail }}</p>
        <details v-if="call.arguments !== undefined"><summary>Proposed arguments</summary><pre>{{ JSON.stringify(call.arguments, null, 2) }}</pre></details>
      </li></ol>
    </section>
    <section v-if="approvals.length" class="approvals" aria-label="Write action approvals">
      <article v-for="item in approvals" :key="item.key" class="approval-card">
        <h2>Review write action</h2>
        <dl><dt>Server</dt><dd>{{ item.request.tool.server }}</dd><dt>Tool</dt><dd>{{ item.request.tool.name }}</dd></dl>
        <h3>Exact arguments</h3><pre>{{ item.request.canonical_arguments }}</pre>
        <h3>Action hash</h3><p class="literal">{{ item.request.action_hash }}</p>
        <p role="status">{{ item.state === 'pending' ? 'Waiting for your decision. Nothing runs until you approve.' : item.state === 'sending' ? 'Decision sent; waiting for the runtime.' : item.state }}</p>
        <div v-if="busy && (item.state === 'pending' || item.state === 'sending')" class="composer-actions">
          <button type="button" :disabled="item.state !== 'pending'" @click="decide(item, 'approve')">Approve</button>
          <button type="button" class="secondary" :disabled="item.state !== 'pending'" @click="decide(item, 'deny')">Deny</button>
        </div>
      </article>
    </section>
    <section v-if="questions.length" class="clarifications" aria-label="Questions from Igor">
      <article v-for="item in questions" :key="item.key" class="activity-row">
        <h2>Igor needs your answer</h2><p class="literal">{{ item.question }}</p>
        <form v-if="busy && (item.state === 'pending' || item.state === 'sending')" class="composer" @submit.prevent="answer(item)">
          <label :for="`answer-${item.key}`">Your answer</label>
          <textarea :id="`answer-${item.key}`" v-model="item.answer" :disabled="item.state !== 'pending'" maxlength="32768" rows="2" />
          <button type="submit" :disabled="item.state !== 'pending' || !item.answer.trim()">Send answer</button>
        </form>
        <p role="status">{{ item.state === 'sending' ? 'Answer sent; waiting for the runtime.' : item.state === 'pending' ? 'Waiting for your answer. You can stop the turn.' : item.state }}</p>
      </article>
    </section>
    <p role="status" class="turn-status">{{ status }}</p>
    <p v-if="error" role="alert">{{ error }} <button v-if="!loaded" @click="refresh()">Try again</button></p>
    <form class="composer" @submit.prevent="send">
      <label for="message">Message Igor</label>
      <textarea id="message" v-model="text" :disabled="busy" maxlength="32768" rows="3" placeholder="What would you like to work on?" />
      <div class="composer-actions"><button type="submit" :disabled="busy || !loaded || !text.trim() || !selected || !!selected.unavailable">Send message</button><button v-if="busy" type="button" class="secondary" @click="stop">Stop turn</button></div>
    </form>
  </section>
</template>
