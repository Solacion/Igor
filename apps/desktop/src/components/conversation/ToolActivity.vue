<script setup lang="ts">
import type { Activity } from '../../activity'
import type { Connection } from '../../conversationDisplay'
import { toolSummary } from '../../conversationDisplay'
defineProps<{ call?: Activity; connection?: Connection }>()
</script>
<template>
  <section class="tool-activity">
    <ol v-if="connection" class="connection-events"><li class="activity-row">
      <strong v-if="connection.state !== 'tools_changed'">{{ connection.server }}</strong>
      <p v-if="connection.state === 'tools_changed'">{{ connection.server }} changed its tools; Igor refreshed them before continuing</p>
      <p v-else>{{ connection.state === 'failed' ? 'Server stopped; later calls are not executed.' : connection.state === 'disconnected' ? 'Server disconnected.' : connection.state === 'connecting' ? 'Server connecting…' : connection.state === 'connected' ? 'Server connected.' : connection.state }}</p>
      <p v-if="connection.detail" class="literal">{{ connection.detail }}</p>
    </li></ol>
    <div v-if="call" class="activity-row compact-tool">
      <p>{{ toolSummary(call) }}</p>
      <details><summary>{{ call.tool }} · {{ call.state }}</summary>
        <p v-if="call.detail" class="literal">{{ call.detail }}</p>
        <div v-if="call.arguments !== undefined"><h3>Proposed arguments</h3><pre>{{ JSON.stringify(call.arguments, null, 2) }}</pre></div>
      </details>
    </div>
  </section>
</template>
