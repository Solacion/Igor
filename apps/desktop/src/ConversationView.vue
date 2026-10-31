<script setup lang="ts">
import { useVoiceOutput } from './voice/output'
import { useConversationSpeech } from './composables/useConversationSpeech'
import { ref } from 'vue'
import { useConversation } from './composables/useConversation'
import type { SettingsSection } from './bridge'
import MessageList from './components/conversation/MessageList.vue'
import Composer from './components/conversation/Composer.vue'
const emit = defineEmits<{
  settings: [
    section?: SettingsSection
  ]
}>()
const confirmingReset = ref(false)
const voice = useVoiceOutput()
const conversation = useConversation()
const speech = useConversationSpeech(conversation, voice)
const { providers, providerId, servers, loaded, busy, text, error, notice, status, toolsUnsupported, exportNotice, exporting, hasSession, timeline, selected, refresh, send, answer, decide, exportActivity, stop, reset } = conversation
function newConversation() {
  speech.stopSpeaking()
  if (timeline.value.length)
    confirmingReset.value = true
  else
    void reset()
}
async function confirmReset() {
  speech.stopSpeaking()
  confirmingReset.value = false
  await reset()
}
async function stopTurn() {
  speech.stopSpeaking()
  await stop()
}
</script>
<template>
  <section class="conversation" aria-labelledby="conversation-title">
    <div class="conversation-heading">
      <div><p class="eyebrow">Your models. Your tools. Your control.</p><h1 id="conversation-title">Conversation</h1></div>
      <div><button class="secondary" :disabled="busy || exporting || !hasSession" @click="exportActivity">Export activity</button> <button class="secondary" :disabled="busy || exporting" @click="newConversation">New conversation</button></div>
    </div>
    <p v-if="exportNotice" role="status">{{ exportNotice }}</p>
    <div class="conversation-toolbar">
    <label for="provider">Model provider</label>
    <select id="provider" v-model="providerId" :disabled="busy || !loaded">
      <option v-for="provider in providers" :key="provider.id" :value="provider.id">{{ provider.id }} · {{ provider.model }}</option>
    </select>
    <p v-if="loaded && !providers.length" class="provider-setup">No model providers configured. Add one to start a conversation. <button class="secondary" @click="emit('settings', 'model-providers')">Open Model providers</button></p>
    <p v-if="selected?.unavailable" role="alert">{{ selected.unavailable }}</p>
    <div class="conversation-servers" aria-label="Server connections">
      <span v-for="server in servers" :key="server.entry.id"><span class="readiness-dot" :class="{ 'is-ready': server.status.state === 'connected', 'is-unavailable': server.status.state === 'failed' }" aria-hidden="true"></span>{{ server.entry.id }}: {{ server.status.state.replaceAll('_', ' ') }}</span>
      <p v-if="loaded && !servers.length">No MCP servers configured. <button class="secondary" @click="emit('settings', 'mcp-servers')">Open Settings</button></p>
    </div>
    </div>
    <p v-if="notice" role="status">{{ notice }}</p>
    <MessageList :entries="timeline" :busy="busy" @decide="decide" @answer="answer" />
    <div v-if="confirmingReset" role="alertdialog" aria-labelledby="reset-title" class="reset-confirmation">
      <h2 id="reset-title">Start a new conversation?</h2><p>This clears the visible history. Export activity first if you need it.</p>
      <button type="button" @click="confirmReset">Start new conversation</button><button type="button" class="secondary" @click="confirmingReset = false">Keep conversation</button>
    </div>
    <p v-if="busy" class="thinking" role="status"><span class="thinking-dot" aria-hidden="true"></span>Igor is thinking…</p>
    <p :role="toolsUnsupported ? 'alert' : 'status'" class="turn-status" :data-state="toolsUnsupported ? 'tools_unsupported' : undefined">{{ status }}<template v-if="toolsUnsupported"> Choose a model with tool calling in <a href="#model-providers" @click.prevent="emit('settings', 'model-providers')">Settings → Model providers</a>.</template></p>
    <p v-if="error" role="alert">{{ error }} <button v-if="!loaded" @click="refresh()">Try again</button></p>
    <button v-if="voice.speaking.value" type="button" class="secondary" @click="speech.stopSpeaking">Stop speaking</button>
    <p v-if="voice.error.value" role="alert">{{ voice.error.value }}</p>
    <Composer v-model:text="text" :busy="busy" :loaded="loaded" :selected="selected" @send="send" @stop="stopTurn" />
  </section>
</template>
