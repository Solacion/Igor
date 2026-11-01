<script setup lang="ts">
import { computed } from 'vue'
import type { ApprovalDecision } from '../../session'
import type { ApprovalItem } from '../../composables/useConversation'
import { approvalSummary, displayValue, previewArguments } from '../../conversationDisplay'
import { isRecord } from '../../events'
const props = defineProps<{ item: ApprovalItem; busy: boolean }>()
const emit = defineEmits<{ decide: [decision: ApprovalDecision] }>()
const args = computed(() => previewArguments(props.item.request.canonical_arguments))
const lines = computed(() => {
  const value = args.value?.lines
  return props.item.request.tool.name === 'create_draft_order' && Array.isArray(value) && value.every(isRecord) ? value : []
})
</script>
<template>
  <article class="approval-card">
    <header class="approval-head">
      <span class="approval-icon" aria-hidden="true"><svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="7" width="10" height="7" rx="1.5"/><path d="M5.5 7V5a2.5 2.5 0 0 1 5 0v2"/></svg></span>
      <div><h2>Review write action</h2><p class="approval-tool">{{ item.request.tool.server }} · {{ item.request.tool.name }} · changes data</p></div>
    </header>
    <p class="approval-summary">{{ approvalSummary(item.request) }}</p>
    <table v-if="lines.length"><thead><tr><th scope="col">SKU</th><th scope="col">Quantity</th><th scope="col">Rationale</th></tr></thead>
      <tbody><tr v-for="(line, index) in lines" :key="index"><td>{{ displayValue(line.sku) }}</td><td>{{ displayValue(line.quantity) }}</td><td>{{ displayValue(line.rationale) }}</td></tr></tbody>
    </table>
    <dl v-else-if="args"><template v-for="(value, key) in args" :key="key"><dt>{{ key }}</dt><dd class="literal">{{ displayValue(value) }}</dd></template></dl>
    <p v-else>Review the exact arguments below before deciding.</p>
    <details><summary>Exact arguments and action hash</summary>
      <dl><dt>Server</dt><dd>{{ item.request.tool.server }}</dd><dt>Tool</dt><dd>{{ item.request.tool.name }}</dd></dl>
      <h3>Exact arguments</h3><pre>{{ item.request.canonical_arguments }}</pre>
      <h3>Action hash</h3><p class="literal">{{ item.request.action_hash }}</p>
    </details>
    <p role="status">{{ item.state === 'pending' ? 'Waiting for your decision. Nothing runs until you approve.' : item.state === 'sending' ? 'Decision sent; waiting for the runtime.' : item.state === 'approved_for_session' ? 'Allowed for this conversation' : item.state }}</p>
    <!-- DOM order keeps Approve first for focus; CSS order puts it on the right. -->
    <div v-if="busy && (item.state === 'pending' || item.state === 'sending')" class="card-actions approval-actions">
      <button type="button" class="approve" :disabled="item.state !== 'pending'" @click="emit('decide', 'approve')">Approve</button>
      <button type="button" class="secondary deny" :disabled="item.state !== 'pending'" @click="emit('decide', 'deny')">Deny</button>
      <button v-if="item.request.session_grant_allowed === true" type="button" class="link session-grant-button" :disabled="item.state !== 'pending'" @click="emit('decide', 'approve_for_session')">Allow for this conversation</button>
    </div>
  </article>
</template>
