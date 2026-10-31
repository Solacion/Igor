<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue'
import { useSettingsStore } from './stores/settings'
import ProviderForm from './ProviderForm.vue'
import { removeProvider, type ProviderSettingsView } from './providers'
const store = useSettingsStore()
const view = store.providerSettings
const unsubscribe = store.subscribe(() => { void refresh(); })
const editing = ref<string | null>(null)
const removing = ref<string | null>(null)
const busy = ref(false)
const error = ref('')
async function refresh() {
  try {
    view.value = await store.refreshProviderSettings()
    error.value = ''
  }
  catch {
    error.value = 'Could not load model providers. Try again.'
  }
}
function saved(next: ProviderSettingsView) { view.value = next; editing.value = null; }
async function remove(id: string) {
  if (busy.value)
    return
  busy.value = true
  error.value = ''
  try {
    view.value = await removeProvider(id)
    removing.value = null
  }
  catch {
    error.value = 'Could not remove this provider. Try again.'
  }
  finally {
    busy.value = false
  }
}
onMounted(refresh)
onUnmounted(unsubscribe)
</script>
<template>
  <section id="model-providers" aria-labelledby="providers-title">
    <div class="section-head"><div><h2 id="providers-title">Model providers</h2><p class="hint">Choose the model Igor uses for conversations.</p></div><button v-if="view && editing === null" @click="editing = 'new'">Add provider</button></div>
    <p v-if="error" role="alert" class="field-error">{{ error }} <button class="secondary" @click="refresh">Retry</button></p>
    <ProviderForm v-if="editing === 'new'" :original="null" @saved="saved" @cancel="editing = null" />
    <p v-if="view && !view.providers.length && editing !== 'new'" class="empty">No model providers configured.</p>
    <ul v-if="view" class="server-list">
      <li v-for="provider in view.providers" :key="provider.entry.id" class="server-card provider-card">
        <ProviderForm v-if="editing === provider.entry.id" :original="provider.entry" @saved="saved" @cancel="editing = null" />
        <template v-else>
          <h3>{{ provider.entry.id }}</h3>
          <p>{{ provider.entry.kind }} · {{ provider.entry.model }}</p><p class="literal">{{ provider.entry.base_url ?? 'default' }}</p>
          <p role="status">{{ provider.key_missing ? `Key variable ${provider.key_missing} is not set` : provider.entry.api_key_env ? 'Key variable is set' : 'No key required' }}</p>
          <p v-if="provider.entry.api_key_env" class="hint">Variable name: {{ provider.entry.api_key_env }}. Only the variable name is saved, never the value</p>
          <div class="form-actions"><button class="secondary" :disabled="busy || editing !== null" @click="editing = provider.entry.id">Edit</button><button class="secondary danger" :disabled="busy || editing !== null" @click="removing = provider.entry.id">Remove</button></div>
          <div v-if="removing === provider.entry.id" role="group" :aria-label="`Remove provider ${provider.entry.id}`"><p>Remove {{ provider.entry.id }}? The next message will use another selected provider.</p><button class="danger" :disabled="busy" @click="remove(provider.entry.id)">Confirm removal</button> <button class="secondary" :disabled="busy" @click="removing = null">Cancel</button></div>
        </template>
      </li>
    </ul>
  </section>
</template>
