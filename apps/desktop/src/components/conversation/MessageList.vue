<script setup lang="ts">
defineProps<{
    messages: Array<{
        role: string
        text: string
    }>
}>()
</script>
<template>
    <ol class="messages" aria-label="Conversation messages" aria-live="polite" aria-relevant="additions">
      <li v-for="(message, index) in messages" :key="index"><strong>{{ message.role }}</strong><p class="literal">{{ message.text }}</p></li>
    </ol>

</template>
