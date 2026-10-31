<script setup lang="ts">
import type { Activity } from '../../activity'
defineProps<{
    activity: Record<string, Activity>
    connections: Array<{
        key: string
        server: string
        state: string
        detail: string
    }>
}>()
</script>
<template>
    <section v-if="Object.keys(activity).length || connections.length" class="tool-activity" aria-labelledby="activity-title">
      <h2 id="activity-title">Tool activity</h2>
      <ol class="connection-events"><li v-for="connection in connections" :key="connection.key" class="activity-row">
        <strong v-if="connection.state !== 'tools_changed'">{{ connection.server }}</strong>
        <p v-if="connection.state === 'tools_changed'">{{ connection.server }} changed its tools; Igor refreshed them before continuing</p>
        <p v-else>{{ connection.state === 'failed' ? 'Server stopped; later calls are not executed.' : connection.state === 'disconnected' ? 'Server disconnected.' : connection.state === 'connecting' ? 'Server connecting…' : connection.state === 'connected' ? 'Server connected.' : connection.state }}</p>
        <p v-if="connection.detail" class="literal">{{ connection.detail }}</p>
      </li></ol>
      <ol><li v-for="(call, key) in activity" :key="key" class="activity-row">
        <strong>{{ call.server }} · {{ call.tool }}</strong><p>{{ call.state }}</p>
        <p v-if="call.detail" class="literal">{{ call.detail }}</p>
        <details v-if="call.arguments !== undefined"><summary>Proposed arguments</summary><pre>{{ JSON.stringify(call.arguments, null, 2) }}</pre></details>
      </li></ol>
    </section>

</template>
