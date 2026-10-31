<script setup lang="ts">
import type { ProviderView } from '../../session'
const text = defineModel<string>('text', { required: true })
defineProps<{
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
</script>
<template>
    <form class="composer" @submit.prevent="emit('send')">
      <label for="message">Message Igor</label>
      <textarea id="message" v-model="text" :disabled="busy" maxlength="32768" rows="3" placeholder="What would you like to work on?" />
      <div class="composer-actions"><button type="submit" :disabled="busy || !loaded || !text.trim() || !selected || !!selected.unavailable">Send message</button><button v-if="busy" type="button" class="secondary" @click="emit('stop')">Stop turn</button></div>
    </form>
</template>
