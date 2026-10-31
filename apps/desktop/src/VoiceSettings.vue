<script setup lang="ts">
import { useVoiceOutput } from './voice/output'
const voice = useVoiceOutput()
const { preferences, voices, error } = voice
</script>
<template>
  <section id="voice" aria-labelledby="voice-title">
    <h2 id="voice-title">Voice</h2>
    <p class="hint">Voice is independent of your AI provider. These preferences stay on this device and are never saved in igor.toml.</p>
    <label><input v-model="preferences.enabled" type="checkbox" :disabled="!voice.supported" /> Speak Igor's replies</label>
    <label for="voice-picker">Voice</label>
    <select id="voice-picker" v-model="preferences.voiceURI" :disabled="!voice.supported">
      <option value="">Automatic (best available English voice)</option>
      <option v-for="option in voices" :key="option.voiceURI" :value="option.voiceURI">{{ option.name }} · {{ option.lang }}</option>
    </select>
    <label for="voice-rate">Speaking rate: {{ preferences.rate }}</label>
    <input id="voice-rate" v-model.number="preferences.rate" type="range" min="0.5" max="2" step="0.1" :disabled="!voice.supported" />
    <button type="button" :disabled="!voice.supported" @click="voice.speak('Hello. I am Igor. You remain in control.', { interrupt: true, test: true })">Test voice</button>
    <button v-if="voice.speaking.value" type="button" class="secondary" @click="voice.stop">Stop speaking</button>
    <p class="hint">Approval requests are read aloud, but approval is accepted only on screen. Spoken approval is never accepted.</p>
    <p v-if="!voice.supported" role="status">Speech synthesis is unavailable in this window.</p>
    <p v-if="error" role="alert">{{ error }}</p>
  </section>
</template>
