<script setup lang="ts">
import type { ApprovalItem } from '../../composables/useConversation'
defineProps<{
    item: ApprovalItem
    busy: boolean
}>()
const emit = defineEmits<{
    decide: [
        decision: 'approve' | 'deny'
    ]
}>()
</script>
<template>
      <article class="approval-card">
        <h2>Review write action</h2>
        <dl><dt>Server</dt><dd>{{ item.request.tool.server }}</dd><dt>Tool</dt><dd>{{ item.request.tool.name }}</dd></dl>
        <h3>Exact arguments</h3><pre>{{ item.request.canonical_arguments }}</pre>
        <h3>Action hash</h3><p class="literal">{{ item.request.action_hash }}</p>
        <p role="status">{{ item.state === 'pending' ? 'Waiting for your decision. Nothing runs until you approve.' : item.state === 'sending' ? 'Decision sent; waiting for the runtime.' : item.state }}</p>
        <div v-if="busy && (item.state === 'pending' || item.state === 'sending')" class="composer-actions">
          <button type="button" :disabled="item.state !== 'pending'" @click="emit('decide', 'approve')">Approve</button>
          <button type="button" class="secondary" :disabled="item.state !== 'pending'" @click="emit('decide', 'deny')">Deny</button>
        </div>
      </article>
</template>
