<script setup lang="ts">
import { useConversation } from './composables/useConversation'
import type { SettingsSection } from './bridge'
import MessageList from './components/conversation/MessageList.vue'
import ToolActivity from './components/conversation/ToolActivity.vue'
import ApprovalCard from './components/conversation/ApprovalCard.vue'
import ClarificationCard from './components/conversation/ClarificationCard.vue'
import Composer from './components/conversation/Composer.vue'
const emit = defineEmits<{
  settings: [
    section?: SettingsSection
  ]
}>()
const { providers, providerId, servers, loaded, busy, text, error, notice, status, toolsUnsupported, exportNotice, exporting, hasSession, questions, approvals, connections, activity, messages, selected, refresh, send, answer, decide, exportActivity, stop, reset } = useConversation()
</script>
<template>
  <section class="conversation" aria-labelledby="conversation-title">
    <div class="conversation-heading">
      <div><p class="eyebrow">Your models. Your tools. Your control.</p><h1 id="conversation-title">Conversation</h1></div>
      <div><button class="secondary" :disabled="busy || exporting || !hasSession" @click="exportActivity">Export activity</button> <button class="secondary" :disabled="busy || exporting" @click="reset">New conversation</button></div>
    </div>
    <p v-if="exportNotice" role="status">{{ exportNotice }}</p>
    <label for="provider">Model provider</label>
    <select id="provider" v-model="providerId" :disabled="busy || !loaded">
      <option v-for="provider in providers" :key="provider.id" :value="provider.id">{{ provider.id }} · {{ provider.model }}</option>
    </select>
    <p v-if="loaded && !providers.length" class="provider-setup">No model providers configured. Add one to start a conversation. <button class="secondary" @click="emit('settings', 'model-providers')">Open Model providers</button></p>
    <p v-if="selected?.unavailable" role="alert">{{ selected.unavailable }}</p>
    <div class="conversation-servers" aria-label="Server connections">
      <span v-for="server in servers" :key="server.entry.id">{{ server.entry.id }}: {{ server.status.state.replaceAll('_', ' ') }}</span>
      <p v-if="loaded && !servers.length">No MCP servers configured. <button class="secondary" @click="emit('settings', 'mcp-servers')">Open Settings</button></p>
    </div>
    <p v-if="notice" role="status">{{ notice }}</p>
    <MessageList :messages="messages" />
    <ToolActivity :activity="activity" :connections="connections" />
    <section v-if="approvals.length" class="approvals" aria-label="Write action approvals">
      <ApprovalCard v-for="item in approvals" :key="item.key" :item="item" :busy="busy" @decide="decide(item, $event)" />
    </section>
    <section v-if="questions.length" class="clarifications" aria-label="Questions from Igor">
      <ClarificationCard v-for="item in questions" :key="item.key" :item="item" :busy="busy" @answer="answer(item)" />
    </section>
    <p :role="toolsUnsupported ? 'alert' : 'status'" class="turn-status" :data-state="toolsUnsupported ? 'tools_unsupported' : undefined">{{ status }}<template v-if="toolsUnsupported"> Choose a model with tool calling in <a href="#model-providers" @click.prevent="emit('settings', 'model-providers')">Settings → Model providers</a>.</template></p>
    <p v-if="error" role="alert">{{ error }} <button v-if="!loaded" @click="refresh()">Try again</button></p>
    <Composer v-model:text="text" :busy="busy" :loaded="loaded" :selected="selected" @send="send" @stop="stop" />
  </section>
</template>
