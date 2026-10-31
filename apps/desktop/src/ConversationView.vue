<script setup lang="ts">
import { useVoiceOutput } from './voice/output'
import { useConversationSpeech } from './composables/useConversationSpeech'
import { useVoiceInput } from './composables/useVoiceInput'
import { computed, nextTick, ref } from 'vue'
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
const conversation = useConversation()
const speech = useConversationSpeech(conversation, voice)
const { providers, providerId, servers, loaded, busy, text, error, notice, status, toolsUnsupported, exportNotice, exporting, hasSession, timeline, selected, refresh, send, answer, decide, exportActivity, stop, reset } = conversation
const voiceInput = useVoiceInput({
  active: () => props.active,
  busy: () => busy.value || confirmingReset.value || !loaded.value || !selected.value || !!selected.value.unavailable,
  onStart: speech.stopSpeaking,
  onText: async (transcript) => {
    text.value = transcript
    if (!voice.preferences.value.reviewBeforeSending)
      await send()
    else {
      await nextTick()
      document.getElementById('message')?.focus()
    }
  },
  onError: message => voice.speak(message, { interrupt: true }),
})
const voiceState = computed(() => voiceInput.state.value !== 'Idle' ? voiceInput.state.value : busy.value ? 'Thinking' : voice.speaking.value ? 'Speaking' : 'Idle')
function hold(event: PointerEvent) {
  const button = event.currentTarget
  if (button instanceof HTMLElement)
    button.setPointerCapture?.(event.pointerId)
  void voiceInput.start()
}
function lostCapture() {
  if (voiceInput.state.value === 'Listening')
    voiceInput.cancel()
}
function keyHold(event: KeyboardEvent) {
  if (!event.repeat)
    void voiceInput.start()
}
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
    <section class="voice-input" aria-label="Voice input">
      <p role="status" aria-live="polite">{{ voiceState }}</p>
      <meter v-if="voiceInput.state.value === 'Listening'" :value="voiceInput.level.value" min="0" max="1" aria-label="Microphone level" />
      <button type="button" class="secondary mic-button" :disabled="!voiceInput.available || busy || !loaded || !selected || !!selected.unavailable || confirmingReset || voiceInput.state.value === 'Transcribing'" :aria-pressed="voiceInput.state.value === 'Listening'" @pointerdown.prevent="hold" @pointerup.prevent="voiceInput.release" @pointercancel="voiceInput.cancel" @lostpointercapture="lostCapture" @keydown.space.prevent="keyHold" @keyup.space.prevent="voiceInput.release">Hold to talk</button>
      <p v-if="!voiceInput.available" class="hint">Voice input is not available in this build. You can still type.</p>
      <p v-else class="hint">Hold the button, or hold Space outside text fields and controls. Release to transcribe.</p>
      <p v-if="voiceInput.error.value" role="alert">{{ voiceInput.error.value }}</p>
    </section>
    <Composer :input-busy="voiceInput.state.value !== 'Idle'" v-model:text="text" :busy="busy" :loaded="loaded" :selected="selected" @send="send" @stop="stopTurn" />
  </section>
</template>
