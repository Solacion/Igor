<script setup lang="ts">
import { nextTick, ref, watch } from 'vue'
import type { ProviderView } from '../../session'
const text = defineModel<string>('text', { required: true })
const props = defineProps<{
  inputBusy?: boolean
  listening?: boolean
  busy: boolean
  loaded: boolean
  selected?: ProviderView
}>()
const emit = defineEmits<{
  send: [
  ]
  stop: [
  ]
}>()
const field = ref<HTMLTextAreaElement>()
/** Grows the field with its text, up to the CSS max-height, then it scrolls. */
function fit() {
  const element = field.value
  if (!element)
    return
  element.style.height = 'auto'
  element.style.height = `${element.scrollHeight}px`
}
watch(text, () => nextTick(fit))
function keydown(event: KeyboardEvent) {
  if (event.key !== 'Enter' || event.shiftKey || event.isComposing)
    return
  event.preventDefault()
  if (!props.busy && !props.inputBusy && props.loaded && text.value.trim() && props.selected && !props.selected.unavailable)
    emit('send')
}
</script>
<template>
    <form class="composer" :class="{ 'is-listening': listening }" @submit.prevent="emit('send')">
      <label for="message" class="sr-only">Message Igor</label>
      <textarea id="message" ref="field" v-model="text" @keydown="keydown" :disabled="busy || inputBusy" maxlength="32768" rows="1" placeholder="Ask Igor anything" />
      <div class="composer-actions">
        <button v-if="busy" type="button" class="icon-button round stop-button" title="Stop turn" @click="emit('stop')"><svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><rect x="2" y="2" width="8" height="8" rx="1.5" fill="currentColor"/></svg><span class="sr-only">Stop turn</span></button>
        <button v-else type="submit" class="icon-button round send-button" title="Send message" :disabled="busy || inputBusy || !loaded || !text.trim() || !selected || !!selected.unavailable"><svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M8 13V3M4 7l4-4 4 4"/></svg><span class="sr-only">Send message</span></button>
        <!-- After Send and Stop in the DOM; CSS shows the microphone first. -->
        <slot name="voice" />
      </div>
    </form>
</template>
