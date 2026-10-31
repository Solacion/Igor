import { watch } from 'vue'
import type { useConversation } from './useConversation'
import type { VoiceOutput } from '../voice/output'
import { approvalSummary } from '../conversationDisplay'
export function useConversationSpeech(conversation: ReturnType<typeof useConversation>, voice: VoiceOutput) {
  let firstMessage = 0
  let completed = false
  function flushReplies() {
    if (!completed || suppressed)
      return
    const messages = conversation.messages.value
    for (const message of messages.slice(firstMessage)) {
      if (message.role === 'Igor')
        voice.speak(message.text)
    }
    firstMessage = messages.length
  }
  let suppressed = false
  watch(conversation.busy, (busy, previous) => {
    if (busy) {
      voice.stop()
      completed = false
      firstMessage = conversation.messages.value.length
      suppressed = false
    }
    else if (previous) {
      completed = true
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
