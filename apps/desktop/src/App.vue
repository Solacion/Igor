<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { readDesktopStatus } from './bridge'
import type { DesktopStatus } from './bridge'

const status = ref<DesktopStatus | null>(null)
const loading = ref(false)
const error = ref(false)

async function checkConnection() {
  if (loading.value) return
  loading.value = true
  error.value = false
  status.value = null
  try {
    status.value = await readDesktopStatus()
  } catch {
    // Native errors can contain local paths or other diagnostics. Keep the UI message fixed.
    error.value = true
  } finally {
    loading.value = false
  }
}

onMounted(checkConnection)
</script>

<template>
  <div class="app-shell">
    <header class="topbar">
      <a class="brand" href="#main" aria-label="Igor home">
        <span class="brand-mark" aria-hidden="true">i</span>
        <span>Igor <span class="byline">by Solacion</span></span>
      </a>
      <span class="edition">Desktop preview</span>
    </header>

    <main id="main" tabindex="-1">
      <section class="intro" aria-labelledby="welcome-title">
        <p class="eyebrow">Your models. Your tools. Your control.</p>
        <h1 id="welcome-title">A place for your<br />next good idea.</h1>
        <p class="lede">A local home for working with AI. Connect the models and tools you choose, with you in control of every action.</p>
      </section>

      <section class="workspace" aria-labelledby="workspace-title">
        <div class="workspace-heading">
          <div>
            <p class="eyebrow">Your workspace</p>
            <h2 id="workspace-title">The foundation is here.</h2>
          </div>
          <span class="phase">First build</span>
        </div>
        <p class="scope-note">This preview checks the desktop connection. Conversations, model selection and tool connections are coming next.</p>
        <div class="status-grid">
          <div class="status-item">
            <span class="status-label">Desktop</span>
            <strong v-if="loading">Checking…</strong>
            <strong v-else-if="status" class="ready">Connected</strong>
            <strong v-else>Not connected</strong>
          </div>
          <div class="status-item">
            <span class="status-label">Models</span>
            <strong>Not connected yet</strong>
          </div>
          <div class="status-item">
            <span class="status-label">Tools</span>
            <strong>Not connected yet</strong>
          </div>
        </div>
        <div class="workspace-footer">
          <div class="feedback" aria-live="polite" aria-atomic="true">
            <p v-if="error" role="alert">Couldn’t reach the desktop app. Open Igor in its native window, then try again.</p>
            <p v-else-if="status">Desktop ready <span aria-hidden="true">·</span> Version {{ status.app_version }}</p>
            <p v-else>Checking the desktop connection…</p>
          </div>
          <button type="button" :disabled="loading" @click="checkConnection">
            {{ loading ? 'Checking…' : error ? 'Try again' : 'Check connection' }}
            <span aria-hidden="true">↗</span>
          </button>
        </div>
      </section>
      <p class="privacy-note">This check stays on your machine. It makes no model calls and changes no files.</p>
    </main>
    <footer class="app-footer"><span>Built to work locally.</span><span>Open source · GPLv3</span></footer>
  </div>
</template>
