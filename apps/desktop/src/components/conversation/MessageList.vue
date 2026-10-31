<script setup lang="ts">
import { nextTick, ref, watch } from 'vue'
import type { ApprovalDecision } from '../../session'
import type { TimelineEntry } from '../../conversationDisplay'
import { approvalSummary } from '../../conversationDisplay'
import ToolActivity from './ToolActivity.vue'
import ApprovalCard from './ApprovalCard.vue'
import ClarificationCard from './ClarificationCard.vue'
import type { ApprovalItem, QuestionItem } from '../../composables/useConversation'
const props = defineProps<{ entries: TimelineEntry[]; busy: boolean }>()
const emit = defineEmits<{ decide: [item: ApprovalItem, decision: ApprovalDecision]; answer: [item: QuestionItem] }>()
const container = ref<HTMLOListElement>()
const following = ref(true)
const announcement = ref('')
const announced = new Set<string>()
function scrolled() {
  const element = container.value
  if (element) following.value = element.scrollHeight - element.scrollTop - element.clientHeight < 64
}
function scrollToLatest() {
  const element = container.value
  if (element) element.scrollTop = element.scrollHeight
  following.value = true
}
watch(() => props.entries, async entries => {
  await nextTick()
  let action: TimelineEntry | undefined
  for (const entry of entries) {
    if ((entry.kind === 'approval' || entry.kind === 'question') && entry.item.state === 'pending' && !announced.has(`${entry.kind}:${entry.key}`)) {
      announced.add(`${entry.kind}:${entry.key}`)
      action = entry
    }
  }
  if (action && (action.kind === 'approval' || action.kind === 'question')) {
    announcement.value = action.kind === 'approval' ? `${approvalSummary(action.item.request)} Review and decide on screen.` : action.item.question
    const row = Array.from(container.value?.children ?? []).find(child => child.getAttribute('data-entry-key') === `${action.kind}:${action.key}`)
    row?.scrollIntoView?.({ block: 'nearest', behavior: 'instant' })
    row?.querySelector<HTMLElement>('button:not(:disabled), textarea:not(:disabled)')?.focus({ preventScroll: true })
  } else if (following.value) scrollToLatest()
}, { deep: true, flush: 'post' })
</script>
<template>
  <p class="sr-only" role="status" aria-live="assertive" aria-atomic="true">{{ announcement }}</p>
  <ol ref="container" class="messages timeline" aria-label="Conversation timeline" aria-live="polite" aria-relevant="additions" @scroll="scrolled">
    <li v-for="entry in entries" :key="`${entry.kind}:${entry.key}`" :data-entry-key="`${entry.kind}:${entry.key}`" :data-kind="entry.kind" :class="{ approvals: entry.kind === 'approval', clarifications: entry.kind === 'question' }">
      <template v-if="entry.kind === 'message'"><strong>{{ entry.message.role }}</strong><p class="literal">{{ entry.message.text }}</p></template>
      <ToolActivity v-else-if="entry.kind === 'tool'" :call="entry.call" />
      <ToolActivity v-else-if="entry.kind === 'connection'" :connection="entry.connection" />
      <p v-else-if="entry.kind === 'session-grant'" class="literal">{{ entry.tool.server }} · {{ entry.tool.name }}: Ran without asking — allowed for this conversation</p>
      <ApprovalCard v-else-if="entry.kind === 'approval'" :item="entry.item" :busy="busy" @decide="emit('decide', entry.item, $event)" />
      <ClarificationCard v-else-if="entry.kind === 'question'" :item="entry.item" :busy="busy" @answer="emit('answer', entry.item)" />
    </li>
  </ol>
  <button v-if="!following" type="button" class="secondary jump-latest" @click="scrollToLatest">Jump to latest</button>
</template>
