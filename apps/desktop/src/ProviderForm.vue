<script setup lang="ts">
import { reactive, ref } from 'vue'
import { isCommandError } from './bridge'
import { saveProvider, providersChanged, type ProviderEntry, type ProviderSettingsView } from './providers'

const props = defineProps<{ original: ProviderEntry | null }>()
const emit = defineEmits<{ saved: [view: ProviderSettingsView]; cancel: [] }>()
const form = reactive({
  id: props.original?.id ?? '', kind: props.original?.kind ?? 'openai_compatible',
  model: props.original?.model ?? '', baseUrl: props.original?.base_url ?? '',
  keyName: props.original?.api_key_env ?? '',
})
const errors = ref<Record<string, string>>({})
const busy = ref(false)
function preset() {
  form.id = 'lm_studio'; form.kind = 'openai_compatible'; form.model = 'qwen/qwen3-vl-8b'
  form.baseUrl = 'http://127.0.0.1:1234/v1'; form.keyName = ''; errors.value = {}
}
async function save() {
  if (busy.value) return
  errors.value = {}
  const id = form.id.trim(), model = form.model.trim(), keyName = form.keyName.trim()
  const baseUrl = form.baseUrl.trim()
  if (!/^[a-z0-9_-]+$/.test(id)) errors.value.id = 'Use lowercase letters, numbers, underscores or hyphens.'
  if (!model) errors.value.model = 'Enter the model identifier.'
  if (keyName && !/^[A-Z_][A-Z0-9_]*$/.test(keyName)) errors.value.keyName = 'Enter an environment variable NAME, never an API key value.'
  if (form.kind === 'anthropic' && !keyName) errors.value.keyName = 'Anthropic needs an API key environment variable name.'
  if (!baseUrl && form.kind === 'openai_compatible') errors.value.baseUrl = 'Enter the provider base URL.'
  if (baseUrl) {
    try {
      const url = new URL(baseUrl)
      if (url.username || url.password || !['http:', 'https:'].includes(url.protocol) ||
          (url.protocol === 'http:' && !['127.0.0.1', 'localhost', '[::1]'].includes(url.hostname))) throw new Error()
    } catch { errors.value.baseUrl = 'Use HTTPS, or HTTP for a local server, without embedded credentials.' }
  }
  if (Object.keys(errors.value).length) return
  const entry: ProviderEntry = { id, kind: form.kind, model, base_url: baseUrl || null, api_key_env: keyName || null }
  busy.value = true
  try {
    const view = await saveProvider(props.original?.id ?? null, entry)
    providersChanged(); emit('saved', view)
  } catch (failure) {
    const fields: Record<string, string> = { id: 'id', kind: 'kind', model: 'model', base_url: 'baseUrl', api_key: 'keyName', api_key_env: 'keyName' }
    errors.value = isCommandError(failure)
      ? { [fields[failure.field ?? ''] ?? 'form']: failure.message }
      : { form: 'Could not save this provider. Try again.' }
  } finally { busy.value = false }
}
</script>
<template>
  <form class="provider-form server-form" @submit.prevent="save">
    <h3>{{ original ? 'Edit model provider' : 'Add model provider' }}</h3>
    <button type="button" class="secondary" :disabled="busy" @click="preset">LM Studio (local)</button>
    <label>Provider id<input v-model="form.id" name="provider-id" :aria-invalid="!!errors.id" :disabled="busy" autocomplete="off" /></label>
    <p v-if="errors.id" class="field-error" role="alert">{{ errors.id }}</p>
    <label>Kind<select v-model="form.kind" name="provider-kind" :disabled="busy"><option value="openai_compatible">openai_compatible</option><option value="anthropic">anthropic</option></select></label>
    <label>Model<input v-model="form.model" name="provider-model" :aria-invalid="!!errors.model" :disabled="busy" autocomplete="off" /></label>
    <p v-if="errors.model" class="field-error" role="alert">{{ errors.model }}</p>
    <label>Base URL<input v-model="form.baseUrl" name="provider-url" :aria-invalid="!!errors.baseUrl" :disabled="busy" autocomplete="off" placeholder="http://127.0.0.1:1234/v1" /></label>
    <p v-if="errors.baseUrl" class="field-error" role="alert">{{ errors.baseUrl }}</p>
    <label>API key environment variable name<input v-model="form.keyName" name="provider-key-name" :aria-invalid="!!errors.keyName" :disabled="busy" autocomplete="off" autocapitalize="off" :spellcheck="false" placeholder="IGOR_ANTHROPIC_KEY" /></label>
    <p class="hint">Only the variable name is saved, never the value</p>
    <p v-if="errors.keyName" class="field-error" role="alert">{{ errors.keyName }}</p>
    <p v-if="errors.form" class="field-error" role="alert">{{ errors.form }}</p>
    <div class="form-actions"><button type="submit" :disabled="busy">{{ busy ? 'Saving…' : 'Save provider' }}</button><button type="button" class="secondary" :disabled="busy" @click="emit('cancel')">Cancel</button></div>
  </form>
</template>
