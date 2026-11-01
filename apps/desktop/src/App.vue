<script setup lang="ts">
import { provideVoiceOutput } from './voice/output'
import { provideSettingsStore } from './stores/settings'
import { onMounted, ref } from 'vue'
import SettingsView from './SettingsView.vue'
import ConversationView from './ConversationView.vue'
import { readDesktopStatus } from './bridge'
import type { DesktopStatus, SettingsSection } from './bridge'
provideSettingsStore()
provideVoiceOutput()
const status = ref<DesktopStatus | null>(null)
const loading = ref(false)
const error = ref(false)
const page = ref<'home' | 'settings'>('home')
const settingsSection = ref<SettingsSection>('mcp-servers')
function openSettings(section: SettingsSection = 'mcp-servers') {
  settingsSection.value = section
  page.value = 'settings'
}
async function checkReadiness() {
  if (loading.value)
    return
  loading.value = true
  error.value = false
  status.value = null
  try {
    status.value = await readDesktopStatus()
  }
  catch {
    // Native errors can contain local paths or other diagnostics. Keep the UI message fixed.
    error.value = true
  }
  finally {
    loading.value = false
  }
}
onMounted(checkReadiness)
</script>

<template>
  <div class="app-shell">
    <main v-if="page === 'settings'" id="settings-main" tabindex="-1" class="app-page">
      <SettingsView :initial-section="settingsSection" @close="page = 'home'" />
    </main>
    <main v-show="page === 'home'" id="main" tabindex="-1" class="app-page">
      <ConversationView :active="page === 'home'" @settings="openSettings" />
    </main>
    <footer class="app-footer">
      <span class="readiness" role="status" aria-live="polite" aria-atomic="true">
        <span class="readiness-dot" :class="{ 'is-ready': status, 'is-unavailable': error }" aria-hidden="true"></span>
        {{ loading ? 'Starting…' : status ? 'Ready' : 'Unavailable' }}
      </span>
      <span v-if="status" class="version">v{{ status.app_version }}</span>
      <span v-if="error" class="footer-error" role="alert">Desktop unavailable. Open Igor in its native window, then try again.</span>
      <button v-if="error" type="button" class="link" @click="checkReadiness">Try again</button>
      <span class="footer-spacer"></span>
      <span>Igor by Solacion · Open source · GPLv3</span>
    </footer>
  </div>
</template>
