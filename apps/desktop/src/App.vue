<script setup lang="ts">
import { onMounted, ref } from 'vue'
import SettingsView from './SettingsView.vue'
import ConversationView from './ConversationView.vue'
import { readDesktopStatus } from './bridge'
import type { DesktopStatus } from './bridge'

const status = ref<DesktopStatus | null>(null)
const loading = ref(false)
const error = ref(false)
const page = ref<'home' | 'settings'>('home')

async function checkReadiness() {
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

onMounted(checkReadiness)
</script>

<template>
  <div class="app-shell">
    <header class="topbar">
      <a class="brand" href="#main" aria-label="Igor home" @click="page = 'home'">
        <span class="brand-mark" aria-hidden="true">i</span>
        <span>Igor <span class="byline">by Solacion</span></span>
      </a>
      <div class="topbar-actions">
        <span class="edition">Desktop preview</span>
        <button type="button" class="secondary" :aria-pressed="page === 'settings'" @click="page = page === 'settings' ? 'home' : 'settings'">Settings</button>
      </div>
    </header>

    <main v-if="page === 'settings'" id="settings-main" tabindex="-1" class="wide">
      <SettingsView @close="page = 'home'" />
    </main>
    <main v-show="page === 'home'" id="main" tabindex="-1" class="wide">
      <ConversationView @settings="page = 'settings'" />
    </main>
    <footer class="app-footer">
      <div class="footer-readiness">
        <span class="readiness" role="status" aria-live="polite" aria-atomic="true">
          <span class="readiness-dot" :class="{ 'is-ready': status, 'is-unavailable': error }" aria-hidden="true"></span>
          {{ loading ? 'Starting…' : status ? 'Ready' : 'Unavailable' }}
        </span>
        <span v-if="status" class="version">v{{ status.app_version }}</span>
        <button v-if="error" type="button" @click="checkReadiness">Try again</button>
      </div>
      <span>Open source · GPLv3</span>
    </footer>
    <p v-if="error" class="footer-error" role="alert">Desktop unavailable. Open Igor in its native window, then try again.</p>
  </div>
</template>
