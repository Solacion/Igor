import { computed, watch } from 'vue'
import type { useConversation } from './useConversation'
import type { VoiceOutput } from '../voice/output'
import { approvalSummary } from '../conversationDisplay'
/** Said when Igor starts using tools in a turn, so the operator is not left in silence. */
export const lookingPhrases = ['Let me have a look for you.', 'One moment, I\'ll check.', 'Let me find out.', 'Give me a second to look that up.']
/** Said when a tool fails; the model usually tries another way. */
export const retryPhrases = ['Hmm, that didn\'t work. Let me try something else.', 'That didn\'t go through. Let me try another way.']
const failedStates = new Set(['Failed', 'Server reported an error'])
export function useConversationSpeech(conversation: ReturnType<typeof useConversation>, voice: VoiceOutput) {
  let firstMessage = 0
  /** True once Igor has said something of its own this turn, such as "Let me check the weather". */
  let saidSomething = false
  /** Speaks Igor's new messages as they arrive, so a sentence before a tool call is heard while
   * the tool runs rather than after the turn. */
  function flushReplies() {
    if (suppressed)
      return
    const messages = conversation.messages.value
    for (const message of messages.slice(firstMessage)) {
      if (message.role === 'Igor' && message.text.trim()) {
        voice.speak(message.text)
        saidSomething = true
      }
    }
    firstMessage = messages.length
  }
  let suppressed = false
  // Tool progress counts per turn: calls started and failed, against the turn's starting counts.
  const progress = computed(() => {
    const rows = Object.values(conversation.activity.value)
    return { started: rows.filter(row => row.state !== 'Proposed' && row.state !== 'Refused').length, failed: rows.filter(row => failedStates.has(row.state)).length }
  })
  let baseline = { started: 0, failed: 0 }
  let approvalsAtStart = 0
  let looked = false
  let retried = 0
  let phrase = 0
  const pick = (phrases: string[]) => phrases[phrase++ % phrases.length]!
  watch(progress, now => {
    if (!conversation.busy.value || suppressed)
      return
    const failed = now.failed - baseline.failed
    if (failed > retried) {
      retried = failed
      // Once per turn is enough; repeated failures read as nagging.
      if (retried === 1)
        voice.speak(pick(retryPhrases))
    }
    // Stay quiet after an on-screen approval, or when Igor already said what it is doing.
    else if (!looked && !saidSomething && now.started > baseline.started && conversation.approvals.value.length === approvalsAtStart) {
      looked = true
      voice.speak(pick(lookingPhrases))
    }
  }, { flush: 'sync' })
  watch(conversation.busy, (busy, previous) => {
    if (busy) {
      voice.stop()
      saidSomething = false
      firstMessage = conversation.messages.value.length
      suppressed = false
      baseline = progress.value
      approvalsAtStart = conversation.approvals.value.length
      looked = false
      retried = 0
    }
    else if (previous) {
      flushReplies()
    }
  }, { flush: 'sync' })
  // Native channel events and the command reply may arrive on different schedules.
  watch(() => conversation.messages.value.length, flushReplies, { flush: 'sync' })
  watch(() => conversation.approvals.value.length, (length, previous) => {
    for (const item of conversation.approvals.value.slice(previous, length)) {
      if (item.state === 'pending')
        voice.speak(`${approvalSummary(item.request).replace(/[.?]$/, '')}. Approve on screen?`, { interrupt: true })
    }
  }, { flush: 'sync' })
  function stopSpeaking() {
    suppressed = true
    voice.stop()
  }
  return { stopSpeaking }
}
