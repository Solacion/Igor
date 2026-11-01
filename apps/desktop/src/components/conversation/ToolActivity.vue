<script setup lang="ts">
import { computed } from 'vue'
import type { Activity } from '../../activity'
import type { Connection } from '../../conversationDisplay'
import { toolLabel } from '../../conversationDisplay'
const props = defineProps<{ call?: Activity; connection?: Connection }>()
/** Icon for the line: working, done or a problem. */
const icon = computed(() => {
  const state = props.call?.state ?? props.connection?.state
  if (state === 'Completed' || state === 'connected' || state === 'tools_changed')
    return 'done'
  if (['Failed', 'Refused', 'Server reported an error', 'failed', 'disconnected'].includes(state ?? ''))
    return 'problem'
  return 'working'
})
</script>
<template>
  <section class="tool-activity" :data-icon="icon">
    <ol v-if="connection" class="connection-events"><li class="activity-row activity-line">
      <span class="activity-icon" aria-hidden="true"></span>
      <Transition name="activity-fade" mode="out-in"><span :key="connection.state" class="activity-text">
        <template v-if="connection.state === 'tools_changed'">{{ connection.server }} changed its tools; Igor refreshed them before continuing</template>
        <template v-else>{{ connection.server }}: {{ connection.state === 'failed' ? 'Server stopped; later calls are not executed.' : connection.state === 'disconnected' ? 'Server disconnected.' : connection.state === 'connecting' ? 'Server connecting…' : connection.state === 'connected' ? 'Server connected.' : connection.state }}</template>
      </span></Transition>
      <span v-if="connection.detail" class="literal activity-detail">{{ connection.detail }}</span>
    </li></ol>
    <details v-if="call" class="activity-row activity-line compact-tool">
      <summary>
        <span class="activity-icon" aria-hidden="true"></span>
        <Transition name="activity-fade" mode="out-in"><span :key="call.state" class="activity-text">{{ toolLabel(call) }} <span class="activity-server">· {{ call.server }} · {{ call.state }}</span></span></Transition>
      </summary>
      <div class="activity-more">
        <p v-if="call.detail" class="literal">{{ call.detail }}</p>
        <div v-if="call.arguments !== undefined"><h3>Proposed arguments</h3><pre>{{ JSON.stringify(call.arguments, null, 2) }}</pre></div>
      </div>
    </details>
  </section>
</template>
