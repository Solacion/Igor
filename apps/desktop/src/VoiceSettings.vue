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
    <div class="section-head"><div><h2 id="voice-title">Voice</h2><p class="hint">Voice runs on this Mac and is independent of your AI provider. These preferences stay on this device and are never saved in igor.toml.</p></div></div>

    <h3 class="group-title">Speaking</h3>
    <div class="group">
      <div class="row">
        <label for="speak-replies" class="row-label">Speak Igor's replies</label>
        <input id="speak-replies" v-model="preferences.enabled" type="checkbox" role="switch" class="switch" :disabled="!voice.supported.value" />
      </div>
      <div class="row">
        <label for="voice-picker" class="row-label">Voice</label>
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
      </div>
      <div class="row">
        <label for="voice-rate" class="row-label">Speaking rate <span class="row-value">{{ preferences.rate }}</span></label>
        <input id="voice-rate" v-model.number="preferences.rate" type="range" min="0.5" max="2" step="0.1" :disabled="!voice.supported.value" />
      </div>
      <div v-if="igor && !igor.installed" class="row igor-voice-download">
        <div class="row-label">Igor's natural voice<p class="hint">Runs on this Mac after a one-time download of {{ megabytes(igor.download_bytes) }} MB. Until then, Igor uses the basic macOS voices.</p></div>
        <template v-if="download">
          <label for="igor-voice-progress" class="sr-only">Downloading Igor's voice: {{ percent }}%</label>
          <progress id="igor-voice-progress" :value="download.received" :max="download.total || undefined" />
          <span class="row-value">{{ percent }}%</span>
        </template>
        <button v-else type="button" class="secondary" @click="voice.downloadIgor">Download Igor's voice ({{ megabytes(igor.download_bytes) }} MB)</button>
      </div>
      <div class="row">
        <span class="row-label">Try it</span>
        <span class="row-controls">
          <button v-if="voice.speaking.value" type="button" class="secondary" @click="voice.stop">Stop speaking</button>
          <button type="button" class="secondary" :disabled="!voice.supported.value" @click="voice.speak('Hello. I am Igor. You remain in control.', { interrupt: true, test: true })">Test voice</button>
        </span>
      </div>
    </div>
    <p class="hint">Approval requests are read aloud, but approval is accepted only on screen. Spoken approval is never accepted.</p>
    <p v-if="!voice.supported.value" role="status">Speech synthesis is unavailable in this window.</p>

    <h3 class="group-title">Listening</h3>
    <div class="group">
      <div class="row">
        <label for="speech-language" class="row-label">Speech language</label>
        <select id="speech-language" v-model="preferences.speechLanguage">
          <option :value="null">Automatic</option>
          <option v-for="language in voice.speechLanguages.value" :key="language.code" :value="language.code">{{ capitaliseLanguage(language.name) }}</option>
        </select>
      </div>
      <p v-if="voice.languageError.value" class="row" role="status">{{ voice.languageError.value }} <button type="button" class="link" @click="voice.refreshLanguages">Try again</button></p>
      <div class="row">
        <div class="row-label">Speech input<p class="hint">Whisper transcribes on this Mac. Audio stays on your device.</p></div>
        <template v-if="inputProgress">
          <label for="input-download-progress" class="sr-only">Downloading speech input: {{ Math.floor(inputProgress.received / inputProgress.total * 100) }}%</label>
          <progress id="input-download-progress" :value="inputProgress.received" :max="inputProgress.total" />
          <span class="row-value">{{ Math.floor(inputProgress.received / inputProgress.total * 100) }}%</span>
        </template>
        <button v-else-if="!inputStatus?.installed" type="button" class="secondary" @click="downloadInput">Download speech input (574 MB)</button>
        <span v-if="inputPreparing" class="row-value" role="status">Preparing speech…</span>
        <span v-else-if="inputStatus?.installed && !inputError" class="row-value is-ready" role="status">Speech input is ready.</span>
      </div>
      <p v-if="inputError" class="row" role="alert">{{ inputError }} <button type="button" class="link" @click="refreshInput">Retry preparing speech</button></p>
    </div>
    <p class="hint">Click the microphone or press Space to speak. Your message sends when you finish speaking.</p>
    <p v-if="error" role="alert">{{ error }}</p>
  </section>
</template>
