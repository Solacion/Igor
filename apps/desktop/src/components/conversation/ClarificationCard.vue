<script setup lang="ts">
import type { QuestionItem } from '../../composables/useConversation'
defineProps<{
    item: QuestionItem
    busy: boolean
}>()
const emit = defineEmits<{
    answer: [
    ]
}>()
</script>
<template>
      <article class="activity-row">
        <h2>Igor needs your answer</h2><p class="literal">{{ item.question }}</p>
        <form v-if="busy && (item.state === 'pending' || item.state === 'sending')" class="composer" @submit.prevent="emit('answer')">
          <label :for="`answer-${item.key}`">Your answer</label>
          <textarea :id="`answer-${item.key}`" v-model="item.answer" :disabled="item.state !== 'pending'" maxlength="32768" rows="2" />
          <button type="submit" :disabled="item.state !== 'pending' || !item.answer.trim()">Send answer</button>
        </form>
        <p role="status">{{ item.state === 'sending' ? 'Answer sent; waiting for the runtime.' : item.state === 'pending' ? 'Waiting for your answer. You can stop the turn.' : item.state }}</p>
      </article>
</template>
