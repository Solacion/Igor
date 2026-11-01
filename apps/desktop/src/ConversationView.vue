<script setup lang="ts">
import { capitaliseLanguage } from './voice/transcription'
import { useVoiceOutput } from './voice/output'
import { useConversationSpeech } from './composables/useConversationSpeech'
import { useVoiceInput } from './composables/useVoiceInput'
import { computed, ref } from 'vue'
import { setServerEnabled } from './bridge'
import { useConversation } from './composables/useConversation'
import type { SettingsSection } from './bridge'
import MessageList from './components/conversation/MessageList.vue'
import Composer from './components/conversation/Composer.vue'
const emit = defineEmits<{
  settings: [
    section?: SettingsSection
  ]
}>()
const props = withDefaults(defineProps<{
  active?: boolean
}>(), { active: true })
const confirmingReset = ref(false)
const voice = useVoiceOutput()
const conversation = useConversation({ spoken: () => voice.preferences.value.enabled })
const speech = useConversationSpeech(conversation, voice)
const { providers, providerId, servers, loaded, busy, text, error, notice, status, toolsUnsupported, exportNotice, exporting, hasSession, timeline, selected, refresh, send, answer, decide, exportActivity, stop, reset } = conversation
const voiceInput = useVoiceInput({
  active: () => props.active,
  language: () => voice.preferences.value.speechLanguage,
  busy: () => busy.value || confirmingReset.value || !loaded.value || !selected.value || !!selected.value.unavailable,
  onStart: speech.stopSpeaking,
  onText: async (transcript) => {
    text.value = transcript
    await send()
  },
  onError: message => voice.speak(message, { interrupt: true }),
})
const heardLanguage = computed(() => {
  const code = voiceInput.heardLanguage.value
  if (!code || code === 'en') return ''
  return capitaliseLanguage(voice.speechLanguages.value.find(language => language.code === code)?.name ?? code)
})
/** Servers the operator has turned on; the others keep their settings but are not used. */
const enabledServers = computed(() => servers.value.filter(server => server.entry.enabled !== false))
const connected = computed(() => enabledServers.value.filter(server => server.status.state === 'connected').length)
const connectionState = computed(() => !enabledServers.value.length ? 'none' : connected.value === enabledServers.value.length ? 'ready' : enabledServers.value.some(server => server.status.state === 'failed') ? 'problem' : 'pending')
const switching = ref<string | null>(null)
async function toggleServer(id: string, enabled: boolean) {
  if (switching.value)
    return
  switching.value = id
  try {
    await setServerEnabled(id, enabled)
  } catch {
    notice.value = `Could not turn ${id} ${enabled ? 'on' : 'off'}. Try again in Settings.`
  } finally {
    switching.value = null
  }
}
const voiceState = computed(() => voiceInput.state.value !== 'Idle' ? voiceInput.state.value : voice.speaking.value ? 'Speaking' : busy.value ? 'Thinking' : 'Idle')
function newConversation() {
  voiceInput.cancel()
  speech.stopSpeaking()
  if (timeline.value.length)
    confirmingReset.value = true
  else
    void reset()
}
async function confirmReset() {
  voiceInput.cancel()
  speech.stopSpeaking()
  confirmingReset.value = false
  await reset()
}
async function stopTurn() {
  voiceInput.cancel()
  speech.stopSpeaking()
  await stop()
}
</script>
<template>
  <section class="conversation" aria-labelledby="conversation-title">
    <header class="toolbar">
      <h1 id="conversation-title" class="toolbar-title">Igor</h1>
      <div class="conversation-toolbar">
        <label for="provider" class="sr-only">Model provider</label>
        <select id="provider" v-model="providerId" class="toolbar-select" :disabled="busy || !loaded">
          <option v-for="provider in providers" :key="provider.id" :value="provider.id">{{ provider.id }} · {{ provider.model }}</option>
        </select>
        <details class="connections" :data-state="connectionState">
          <summary :aria-label="`Server connections: ${connected} of ${enabledServers.length} connected`"><span class="readiness-dot" :class="{ 'is-ready': connectionState === 'ready', 'is-unavailable': connectionState === 'problem' }" aria-hidden="true"></span>{{ enabledServers.length === 1 ? '1 server' : `${enabledServers.length} servers` }}</summary>
          <div class="conversation-servers popover" aria-label="Server connections">
            <span v-for="server in servers" :key="server.entry.id" class="connection-row" :data-enabled="server.entry.enabled !== false"><span class="readiness-dot" :class="{ 'is-ready': server.status.state === 'connected', 'is-unavailable': server.status.state === 'failed', 'is-off': server.status.state === 'disabled' }" aria-hidden="true"></span><span class="connection-name">{{ server.entry.id }}</span><span class="connection-state">{{ server.status.state === 'disabled' ? 'off' : server.status.state.replaceAll('_', ' ') }}</span><input type="checkbox" role="switch" class="switch small" :checked="server.entry.enabled !== false" :disabled="!!switching || busy" :aria-label="`Use ${server.entry.id}`" @change="toggleServer(server.entry.id, ($event.target as HTMLInputElement).checked)" /></span>
            <p v-if="loaded && !servers.length">No MCP servers configured. <button class="link" @click="emit('settings', 'mcp-servers')">Open Settings</button></p>
            <button v-else type="button" class="link" @click="emit('settings', 'mcp-servers')">Manage servers…</button>
          </div>
        </details>
      </div>
      <span class="toolbar-spacer"></span>
      <button type="button" class="icon-button" :disabled="busy || exporting" title="New conversation" @click="newConversation"><svg width="17" height="17" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" aria-hidden="true"><path d="M9 3.5H5.5a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h9a2 2 0 0 0 2-2V11"/><path d="M14.5 3.2l2.3 2.3-6.6 6.6-3 .7.7-3z"/></svg><span class="sr-only">New conversation</span></button>
      <button type="button" class="icon-button" :disabled="busy || exporting || !hasSession" title="Export activity" @click="exportActivity"><svg width="17" height="17" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M10 12.5V3M6.5 6.5L10 3l3.5 3.5"/><path d="M4 11v4.5a1.5 1.5 0 0 0 1.5 1.5h9a1.5 1.5 0 0 0 1.5-1.5V11"/></svg><span class="sr-only">Export activity</span></button>
      <button type="button" class="icon-button" title="Settings" @click="emit('settings')"><svg width="17" height="17" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" aria-hidden="true"><circle cx="10" cy="10" r="2.6"/><path d="M10 2.5v2M10 15.5v2M2.5 10h2M15.5 10h2M4.7 4.7l1.4 1.4M13.9 13.9l1.4 1.4M4.7 15.3l1.4-1.4M13.9 6.1l1.4-1.4"/></svg><span class="sr-only">Settings</span></button>
    </header>

    <div class="notices">
      <p v-if="exportNotice" role="status">{{ exportNotice }}</p>
      <p v-if="loaded && !providers.length" class="provider-setup">No model providers configured. Add one to start a conversation. <button class="link" @click="emit('settings', 'model-providers')">Open Model providers</button></p>
      <p v-if="selected?.unavailable" role="alert">{{ selected.unavailable }}</p>
      <p v-if="notice" role="status">{{ notice }}</p>
      <p v-if="error" role="alert">{{ error }} <button v-if="!loaded" class="link" @click="refresh()">Try again</button></p>
      <p v-if="voice.error.value" role="alert">{{ voice.error.value }}</p>
    </div>

    <MessageList :entries="timeline" :busy="busy" @decide="decide" @answer="answer" />

    <footer class="composer-area">
      <div v-if="confirmingReset" role="alertdialog" aria-labelledby="reset-title" class="reset-confirmation">
        <h2 id="reset-title">Start a new conversation?</h2><p>This clears the visible history. Export activity first if you need it.</p>
        <div class="card-actions"><button type="button" class="start-new" @click="confirmReset">Start new conversation</button><button type="button" class="secondary" @click="confirmingReset = false">Keep conversation</button></div>
      </div>
      <p v-if="busy" class="thinking" role="status"><span class="thinking-dot" aria-hidden="true"></span>Igor is thinking…</p>
      <p :role="toolsUnsupported ? 'alert' : 'status'" class="turn-status" :data-state="toolsUnsupported ? 'tools_unsupported' : undefined">{{ status }}<template v-if="toolsUnsupported"> Choose a model with tool calling in <a href="#model-providers" @click.prevent="emit('settings', 'model-providers')">Settings → Model providers</a>.</template></p>
      <Composer :input-busy="voiceInput.state.value !== 'Idle' || confirmingReset" :listening="voiceInput.state.value === 'Listening'" v-model:text="text" :busy="busy" :loaded="loaded" :selected="selected" @send="send" @stop="stopTurn">
        <template #voice>
          <meter v-if="voiceInput.state.value === 'Listening'" class="mic-level" :value="voiceInput.level.value" min="0" max="1" aria-label="Microphone level" />
          <button type="button" class="icon-button round mic-button" :disabled="!voiceInput.available || busy || !loaded || !selected || !!selected.unavailable || confirmingReset || voiceInput.state.value === 'Transcribing'" :aria-pressed="voiceInput.state.value === 'Listening'" :title="voiceInput.state.value === 'Listening' ? 'Stop listening' : 'Talk'" @click="voiceInput.toggle">
            <svg v-if="voiceInput.state.value !== 'Listening'" width="15" height="15" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" aria-hidden="true"><rect x="5.5" y="1.5" width="5" height="8.5" rx="2.5"/><path d="M3 7.5a5 5 0 0 0 10 0M8 12.5v2"/></svg>
            <svg v-else width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><rect x="2" y="2" width="8" height="8" rx="1.5" fill="currentColor"/></svg>
            <span class="sr-only">{{ voiceInput.state.value === 'Listening' ? 'Stop listening' : 'Talk' }}</span>
          </button>
        </template>
      </Composer>
      <section class="voice-input" aria-label="Voice input">
        <span role="status" aria-live="polite" class="voice-state" :data-state="voiceState">{{ voiceState }}</span>
        <span v-if="voiceInput.preparing.value" role="status">Preparing speech recognition…</span>
        <span v-else-if="!voiceInput.available" class="hint">Download speech input in Settings → Voice, or <button type="button" class="link" @click="voiceInput.prepare">try again</button>.</span>
        <span v-else class="hint">Press Space or the microphone to talk; it sends when you pause.</span>
        <span v-if="heardLanguage" class="hint">Heard in {{ heardLanguage }}</span>
        <button v-if="voice.speaking.value" type="button" class="link" @click="speech.stopSpeaking">Stop speaking</button>
        <span v-if="voiceInput.error.value" class="voice-error" role="alert">{{ voiceInput.error.value }}</span>
      </section>
    </footer>
  </section>
</template>
