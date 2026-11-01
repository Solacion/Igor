<script setup lang="ts">
import { inputStatus, inputPreparing, inputProgress, inputError, refreshInput, downloadInput, capitaliseLanguage } from './voice/transcription'
import { computed, onMounted } from 'vue'
import { igorVoicePrefix, useVoiceOutput } from './voice/output'
onMounted(refreshInput)
const voice = useVoiceOutput()
const { preferences, voices, igor, download, error } = voice
const megabytes = (bytes: number) => Math.round(bytes / 1_000_000)
const percent = computed(() => download.value?.total ? Math.floor(download.value.received / download.value.total * 100) : 0)
</script>
<template>
  <section id="voice" aria-labelledby="voice-title">
    <h2 id="voice-title">Voice</h2>
    <p class="hint">Voice is independent of your AI provider. These preferences stay on this device and are never saved in igor.toml.</p>
    <label for="speech-language">Speech language</label>
    <select id="speech-language" v-model="preferences.speechLanguage">
      <option :value="null">Automatic</option>
      <option v-for="language in voice.speechLanguages.value" :key="language.code" :value="language.code">{{ capitaliseLanguage(language.name) }}</option>
    </select>
    <p v-if="voice.languageError.value" role="status">{{ voice.languageError.value }} <button type="button" @click="voice.refreshLanguages">Try again</button></p>
    <label><input v-model="preferences.enabled" type="checkbox" :disabled="!voice.supported.value" /> Speak Igor's replies</label>
    <p class="hint">Click Talk to speak. Your message sends automatically when you finish speaking.</p>
    <h3>Speech input</h3>
    <p class="hint">Whisper transcribes on this Mac. Audio stays on your device. Download its 574 MB model once to enable voice input.</p>
    <template v-if="inputProgress">
      <label for="input-download-progress">Downloading speech input: {{ Math.floor(inputProgress.received / inputProgress.total * 100) }}%</label>
      <progress id="input-download-progress" :value="inputProgress.received" :max="inputProgress.total" />
    </template>
    <button v-else-if="!inputStatus?.installed" type="button" @click="downloadInput">Download speech input (574 MB)</button>
    <p v-if="inputPreparing" role="status">Preparing speech…</p>
    <p v-else-if="inputStatus?.installed && !inputError" role="status">Speech input is ready.</p>
    <p v-if="inputError" role="alert">{{ inputError }} <button type="button" @click="refreshInput">Retry preparing speech</button></p>
    <div v-if="igor && !igor.installed" class="igor-voice-download">
      <p class="hint">Igor has its own natural voice. It runs on this Mac with no extra service, after a one-time download of {{ megabytes(igor.download_bytes) }} MB. Until then, Igor uses the basic macOS voices.</p>
      <template v-if="download">
        <label for="igor-voice-progress">Downloading Igor's voice: {{ percent }}%</label>
        <progress id="igor-voice-progress" :value="download.received" :max="download.total || undefined" />
      </template>
      <button v-else type="button" @click="voice.downloadIgor">Download Igor's voice ({{ megabytes(igor.download_bytes) }} MB)</button>
    </div>
    <label for="voice-picker">Voice</label>
    <select id="voice-picker" v-model="preferences.voiceURI" :disabled="!voice.supported.value">
      <option value="">Automatic ({{ igor?.installed ? 'Igor\'s voice' : 'best available English voice' }})</option>
      <optgroup v-if="igor?.installed" label="Igor's voices">
        <option v-for="option in igor.voices" :key="option.id" :value="igorVoicePrefix + option.id">{{ option.name }} — {{ option.accent }}</option>
      </optgroup>
      <template v-if="voices.length">
        <option disabled value="separator">──────────</option>
        <optgroup label="macOS voices (basic)">
          <option v-for="option in voices" :key="option.voiceURI" :value="option.voiceURI">{{ option.name }} · {{ option.lang }}</option>
        </optgroup>
      </template>
    </select>
    <label for="voice-rate">Speaking rate: {{ preferences.rate }}</label>
    <input id="voice-rate" v-model.number="preferences.rate" type="range" min="0.5" max="2" step="0.1" :disabled="!voice.supported.value" />
    <button type="button" :disabled="!voice.supported.value" @click="voice.speak('Hello. I am Igor. You remain in control.', { interrupt: true, test: true })">Test voice</button>
    <button v-if="voice.speaking.value" type="button" class="secondary" @click="voice.stop">Stop speaking</button>
    <p class="hint">Approval requests are read aloud, but approval is accepted only on screen. Spoken approval is never accepted.</p>
    <p v-if="!voice.supported.value" role="status">Speech synthesis is unavailable in this window.</p>
    <p v-if="error" role="alert">{{ error }}</p>
  </section>
</template>
