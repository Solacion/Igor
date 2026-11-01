<script setup lang="ts">
import { onScopeDispose, reactive, ref } from 'vue'
import { isCommandError } from './bridge'
import { saveProvider, saveProviderKey, type ProviderEntry, type ProviderSettingsView } from './providers'
const props = defineProps<{
  original: ProviderEntry | null
}>()
const emit = defineEmits<{
  saved: [
    view: ProviderSettingsView
  ]
  cancel: [
  ]
}>()
const form = reactive({
  id: props.original?.id ?? '', kind: props.original?.kind ?? 'openai_compatible',
  model: props.original?.model ?? '', baseUrl: props.original?.base_url ?? '',
  keyName: props.original?.api_key_env ?? '',
})
const keyInput = ref<HTMLInputElement | null>(null)
const useEnvironment = ref(!!props.original?.api_key_env)
onScopeDispose(() => { if (keyInput.value) keyInput.value.value = '' })
const errors = ref<Record<string, string>>({})
const busy = ref(false)
function preset() {
  form.id = 'lm_studio'
  form.kind = 'openai_compatible'
  form.model = 'qwen/qwen3-vl-8b'
  form.baseUrl = 'http://127.0.0.1:1234/v1'
  form.keyName = ''
  useEnvironment.value = false
  if (keyInput.value) keyInput.value.value = ''
  errors.value = {}
}
async function save() {
  if (busy.value)
    return
  errors.value = {}
  const id = form.id.trim(), model = form.model.trim(), keyName = form.keyName.trim()
  const baseUrl = form.baseUrl.trim()
  if (!/^[a-z0-9_-]+$/.test(id))
    errors.value.id = 'Use lowercase letters, numbers, underscores or hyphens.'
  if (!model)
    errors.value.model = 'Enter the model identifier.'
  if (form.kind === 'anthropic' && (useEnvironment.value ? !keyName : !keyInput.value?.value && !props.original?.api_key_keychain))
    errors.value.keyName = useEnvironment.value ? 'Anthropic needs an API key environment variable name.' : 'Enter an API key, or use an environment variable instead.'
  if (!baseUrl && form.kind === 'openai_compatible')
    errors.value.baseUrl = 'Enter the provider base URL.'
  if (baseUrl) {
    try {
      const url = new URL(baseUrl)
      if (url.username || url.password || !['http:', 'https:'].includes(url.protocol) ||
        (url.protocol === 'http:' && !['127.0.0.1', 'localhost', '[::1]'].includes(url.hostname)))
        throw new Error()
    }
    catch {
      errors.value.baseUrl = 'Use HTTPS, or HTTP for a local server, without embedded credentials.'
    }
  }
  if (Object.keys(errors.value).length)
    return
  const entry: ProviderEntry = { id, kind: form.kind, model, base_url: baseUrl || null, api_key_env: useEnvironment.value ? keyName || null : null,
    ...(!useEnvironment.value && props.original?.api_key_keychain ? { api_key_keychain: props.original.api_key_keychain } : {}) }
  busy.value = true
  try {
    // The password is never held in reactive state. Clear the element before awaiting IPC.
    const pending = !useEnvironment.value && keyInput.value?.value
      ? saveProviderKey(props.original?.id ?? null, entry, keyInput.value.value)
      : saveProvider(props.original?.id ?? null, entry)
    if (keyInput.value) keyInput.value.value = ''
    const view = await pending
    emit('saved', view)
  }
  catch (failure) {
    const fields: Record<string, string> = { id: 'id', kind: 'kind', model: 'model', base_url: 'baseUrl', api_key: 'keyName', api_key_env: 'keyName' }
    errors.value = isCommandError(failure)
      ? { [fields[failure.field ?? ''] ?? 'form']: failure.message }
      : { form: 'Could not save this provider. Try again.' }
  }
  finally {
    if (keyInput.value) keyInput.value.value = ''
    busy.value = false
  }
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
    <template v-if="!useEnvironment">
      <label>API key<input ref="keyInput" name="provider-api-key" type="password" :disabled="busy" autocomplete="new-password" autocapitalize="off" :spellcheck="false" /></label>
      <p class="hint">Saved only in macOS Keychain. {{ original?.api_key_keychain ? 'Leave empty to keep the saved key.' : form.kind === 'anthropic' ? 'Required for this provider.' : 'Optional for local providers.' }}</p>
    </template>
    <details><summary>Advanced</summary>
    <label><input v-model="useEnvironment" type="checkbox" name="use-environment" :disabled="busy" @change="() => { if (keyInput) keyInput.value = '' }" /> Use environment variable instead</label>
    <template v-if="useEnvironment">
    <label>API key environment variable name<input v-model="form.keyName" name="provider-key-name" :aria-invalid="!!errors.keyName" :disabled="busy" autocomplete="off" autocapitalize="off" :spellcheck="false" placeholder="IGOR_ANTHROPIC_KEY" /></label>
    <p class="hint">Only the variable name is saved, never the value</p>
    </template></details>
    <p v-if="errors.keyName" class="field-error" role="alert">{{ errors.keyName }}</p>
    <p v-if="errors.form" class="field-error" role="alert">{{ errors.form }}</p>
    <div class="form-actions"><button type="submit" :disabled="busy">{{ busy ? 'Saving…' : 'Save provider' }}</button><button type="button" class="secondary" :disabled="busy" @click="emit('cancel')">Cancel</button></div>
  </form>
</template>
