<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { readSettings, type ServerView } from './bridge'
import { readProviders, startSession, type CoreEvent, type ProviderView, type TurnOutcome } from './session'

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
const messages = ref<Array<{ role: string; text: string }>>([])
const selected = computed(() => providers.value.find(p => p.id === providerId.value))
let session: Awaited<ReturnType<typeof startSession>> | null = null
let lastSequence = -1
let timer: ReturnType<typeof setInterval> | undefined
let disposed = false
let refreshing = false
let stopRequested = false

async function refresh() {
  if (refreshing) return
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
  } finally { refreshing = false }
}
function finish(outcome: TurnOutcome) {
  switch (outcome.outcome) {
    case 'completed': status.value = 'Completed'; break
    case 'cancelled': status.value = 'Cancelled — the answer may be incomplete'; break
    case 'limit_reached': status.value = `Stopped at limit: ${outcome.limit}. The answer may be incomplete.`; break
    case 'failed': status.value = `Turn failed: ${outcome.message}`; break
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
  if (event.event === 'assistant_text' && typeof event.text === 'string') messages.value.push({ role: 'Igor', text: event.text })
  if (event.event === 'turn_finished') finish(event.outcome as TurnOutcome)
}
async function prepare() {
  if (session && session.info.provider_id !== providerId.value) {
    await session.close(); session = null
    notice.value = 'Provider changed. A new model conversation starts here; earlier messages remain visible.'
  }
  if (!session) { lastSequence = -1; session = await startSession(providerId.value, receive) }
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
  } finally { busy.value = false }
}
async function stop() {
  stopRequested = true
  try { await session?.cancel(); status.value = 'Stopping…' }
  catch { error.value = 'Could not stop the turn. Try again.' }
}
async function reset() {
  if (busy.value) return
  try {
    await session?.close(); session = null; messages.value = []; notice.value = ''; error.value = ''; status.value = 'Ready for a message'
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
    <p role="status" class="turn-status">{{ status }}</p>
    <p v-if="error" role="alert">{{ error }} <button v-if="!loaded" @click="refresh">Try again</button></p>
    <form class="composer" @submit.prevent="send">
      <label for="message">Message Igor</label>
      <textarea id="message" v-model="text" :disabled="busy" maxlength="32768" rows="3" placeholder="What would you like to work on?" />
      <div class="composer-actions"><button type="submit" :disabled="busy || !loaded || !text.trim() || !selected || !!selected.unavailable">Send message</button><button v-if="busy" type="button" class="secondary" @click="stop">Stop turn</button></div>
    </form>
  </section>
</template>
