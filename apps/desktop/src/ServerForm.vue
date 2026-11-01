<script setup lang="ts">
import { computed, reactive, ref } from 'vue'
import { isCommandError, saveServer, testServer } from './bridge'
import type { SecretEntry, ServerEntry, SettingsView, ToolView } from './bridge'

const props = defineProps<{ original: ServerEntry | null }>()
const emit = defineEmits<{ saved: [view: SettingsView]; cancel: [] }>()

const lines = (text: string) => text.split('\n').map((line) => line.trim()).filter(Boolean)

/** The Keychain item that holds the key typed here for server `id`. */
const keyItem = (id: string) => `mcp-${id}`
const inKeychain = (secret: SecretEntry | null | undefined, item: string) => !!secret && 'keychain' in secret && secret.keychain === item
const originalItem = keyItem(props.original?.id ?? '')
/** The header that carries the key typed here, when the original sends it as a header. */
const originalHeader = Object.entries(props.original?.headers ?? {}).find(([, secret]) => inKeychain(secret, originalItem))?.[0]
/** True when a key typed here earlier is in the Keychain, so the field may stay empty. */
const keySaved = inKeychain(props.original?.bearer_token, originalItem) || !!originalHeader

const form = reactive({
  id: props.original?.id ?? '',
  transport: ((props.original?.transport ?? 'stdio') === 'stdio' ? 'stdio' : 'http') as 'stdio' | 'http',
  protocol: (props.original?.transport === 'sse' ? 'sse' : 'http') as 'http' | 'sse',
  url: props.original?.url ?? '',
  auth: (props.original?.auth === 'oauth' ? 'oauth' : inKeychain(props.original?.bearer_token, originalItem) ? 'bearer' : originalHeader ? 'header' : 'none') as 'none' | 'bearer' | 'header' | 'oauth',
  header: originalHeader ?? 'X-API-Key',
  key: '',
  command: props.original?.command ?? '',
  args: (props.original?.args ?? []).join('\n'),
  cwd: props.original?.cwd ?? '',
  envAllowlist: (props.original?.env_allowlist ?? []).join('\n'),
  env: Object.entries(props.original?.env ?? {}).map(([name, from]) => `${name}=${from}`).join('\n'),
  startupTimeout: props.original?.startup_timeout_ms?.toString() ?? '',
  callTimeout: props.original?.call_timeout_ms?.toString() ?? '',
})

/** Error message per form field, plus `form` for errors not tied to a field. */
const errors = ref<Record<string, string>>({})
const busy = ref<'test' | 'save' | null>(null)
const confirming = ref(false)
const tested = ref<ToolView[] | null>(null)

/** Maps an entry field path such as `env.TOKEN` or `args[2]` to its form field. */
function formField(field: string | null): string {
  const key = (field ?? '').split(/[.[]/)[0] ?? ''
  const known: Record<string, string> = {
    id: 'id', command: 'command', args: 'args', cwd: 'cwd', env_allowlist: 'envAllowlist', env: 'env',
    startup_timeout_ms: 'startupTimeout', call_timeout_ms: 'callTimeout',
    url: 'url', headers: 'header', bearer_token: 'key', secret: 'key',
  }
  return known[key] ?? 'form'
}

function timeout(text: string, field: string): number | null {
  if (!text.trim()) return null
  const value = Number(text)
  if (!Number.isInteger(value) || value < 0) {
    errors.value[field] = 'Enter a whole number of milliseconds.'
    return null
  }
  return value
}

/** The entry described by the form, or null when the form has errors of its own. */
function entry(): ServerEntry | null {
  errors.value = {}
  const env: Record<string, string> = {}
  for (const line of lines(form.env)) {
    const [name, from, extra] = line.split('=').map((part) => part.trim())
    if (!name || !from || extra !== undefined) {
      errors.value.env = 'Write one NAME=IGOR_VARIABLE per line.'
      break
    }
    env[name] = from
  }
  const common = {
    id: form.id.trim(),
    // Editing keeps the server on or off as it was.
    enabled: props.original?.enabled ?? true,
    startup_timeout_ms: timeout(form.startupTimeout, 'startupTimeout'),
    call_timeout_ms: timeout(form.callTimeout, 'callTimeout'),
  }
  let result: ServerEntry
  if (form.transport === 'http') {
    const item = keyItem(common.id)
    // Hand-written headers stay; the one that carried the typed key is replaced.
    const headers = Object.fromEntries(Object.entries(props.original?.headers ?? {}).filter(([name]) => name !== originalHeader))
    if (form.auth === 'header') {
      if (!form.header.trim()) errors.value.header = 'Enter the header name, such as X-API-Key.'
      headers[form.header.trim()] = { keychain: item }
    }
    if ((form.auth === 'bearer' || form.auth === 'header') && !form.key && !(keySaved && common.id === props.original?.id))
      errors.value.key = 'Enter the key. It is saved in Keychain, never in the configuration file.'
    result = {
      ...common, transport: form.protocol, url: form.url.trim(), headers, auth: form.auth === 'oauth' && form.protocol === 'http' ? 'oauth' : null,
      bearer_token: form.auth === 'bearer' ? { keychain: item } : props.original?.bearer_token && !inKeychain(props.original.bearer_token, originalItem) ? props.original.bearer_token : null,
      command: '', args: [], cwd: null, env_allowlist: [], env: {},
    }
  } else {
    result = {
      ...common, transport: 'stdio',
      command: form.command.trim(),
      args: lines(form.args),
      cwd: form.cwd.trim() || null,
      env_allowlist: lines(form.envAllowlist),
      env,
    }
  }
  return Object.keys(errors.value).length ? null : result
}

function showError(error: unknown) {
  if (isCommandError(error)) errors.value = { [formField(error.field)]: error.message }
  else errors.value = { form: 'Igor could not complete this. Try again in the desktop window.' }
}

/** The program and arguments that will run, or the URL Igor connects to, as one line. */
const commandLine = computed(() => form.transport === 'http' ? form.url.trim() : [form.command.trim(), ...lines(form.args)].join(' '))
const runsSomethingNew = computed(() =>
  !props.original ||
  (props.original.transport ?? 'stdio') !== (form.transport === 'http' ? form.protocol : 'stdio') ||
  (form.transport === 'http'
    ? (props.original.url ?? '') !== form.url.trim()
    : props.original.command !== form.command.trim() ||
      props.original.args.join('\n') !== lines(form.args).join('\n') ||
      (props.original.cwd ?? '') !== form.cwd.trim()))
/** The typed key, sent only for a remote server that uses one. */
const typedKey = () => form.transport === 'http' && (form.auth === 'bearer' || form.auth === 'header') && form.key ? form.key : null

async function test() {
  const value = entry()
  if (!value || busy.value) return
  busy.value = 'test'
  tested.value = null
  try {
    tested.value = await testServer(props.original?.id ?? null, value, typedKey())
  } catch (error) {
    showError(error)
  } finally {
    busy.value = null
  }
}

function requestSave() {
  if (!entry()) return
  if (runsSomethingNew.value) confirming.value = true
  else void save()
}

async function save() {
  const value = entry()
  confirming.value = false
  if (!value || busy.value) return
  busy.value = 'save'
  try {
    emit('saved', await saveServer(props.original?.id ?? null, value, typedKey()))
    form.key = ''
  } catch (error) {
    showError(error)
  } finally {
    busy.value = null
  }
}
</script>

<template>
  <form class="server-form" novalidate @submit.prevent="requestSave">
    <h3>{{ original ? `Edit ${original.id}` : 'Add an MCP server' }}</h3>
    <div class="segmented" role="radiogroup" aria-label="Kind of server">
      <label :class="{ 'is-on': form.transport === 'stdio' }"><input v-model="form.transport" type="radio" name="transport" value="stdio" />Local program</label>
      <label :class="{ 'is-on': form.transport === 'http' }"><input v-model="form.transport" type="radio" name="transport" value="http" />Remote server</label>
    </div>
    <p class="hint">{{ form.transport === 'http' ? 'Igor connects to the server over the internet (Streamable HTTP). Headers are sent only to this URL.' : 'Igor starts the server as a program on this computer and talks to it over standard input and output.' }}</p>

    <label>Name
      <input v-model="form.id" name="id" autocomplete="off" spellcheck="false" placeholder="pos" :aria-invalid="!!errors.id" />
      <small class="hint">Lowercase letters, digits, - or _. Tools appear to the model as name__tool.</small>
      <small v-if="errors.id" class="field-error" role="alert">{{ errors.id }}</small>
    </label>
    <template v-if="form.transport === 'http'">
      <label>URL
        <input v-model="form.url" name="url" type="url" autocomplete="off" spellcheck="false" placeholder="https://mcp.example.com/mcp" :aria-invalid="!!errors.url" />
        <small class="hint">https:// is required, except for this computer (localhost).</small>
        <small v-if="errors.url" class="field-error" role="alert">{{ errors.url }}</small>
      </label>
      <label>Protocol
        <select v-model="form.protocol" name="protocol">
          <option value="http">Streamable HTTP</option>
          <option value="sse">HTTP+SSE (older servers)</option>
        </select>
        <small v-if="form.protocol === 'sse'" class="hint">For servers built for the 2024-11-05 version of MCP. The URL is the server's event stream, often ending in /sse.</small>
      </label>
      <label>Authentication
        <select v-model="form.auth" name="auth">
          <option value="none">None</option>
          <option value="bearer">Bearer token</option>
          <option value="header">API key header</option>
          <option v-if="form.protocol === 'http'" value="oauth">Sign in (OAuth)</option>
        </select>
        <small v-if="form.auth === 'oauth'" class="hint">After saving, choose Sign in on the server. Igor opens the sign-in page in your browser and keeps the tokens in Keychain.</small>
      </label>
      <label v-if="form.auth === 'header'">Header name
        <input v-model="form.header" name="header" autocomplete="off" spellcheck="false" :aria-invalid="!!errors.header" />
        <small v-if="errors.header" class="field-error" role="alert">{{ errors.header }}</small>
      </label>
      <label v-if="form.auth === 'bearer' || form.auth === 'header'">{{ form.auth === 'bearer' ? 'Token' : 'Key' }}
        <input v-model="form.key" name="key" type="password" autocomplete="off" spellcheck="false" :placeholder="keySaved ? 'Saved in Keychain — type to replace' : ''" :aria-invalid="!!errors.key" />
        <small class="hint">Saved in your Keychain, never in the configuration file.</small>
        <small v-if="errors.key" class="field-error" role="alert">{{ errors.key }}</small>
      </label>
    </template>
    <template v-else>
    <label>Command
      <input v-model="form.command" name="command" autocomplete="off" spellcheck="false" placeholder="/usr/local/bin/pos-mcp" :aria-invalid="!!errors.command" />
      <small v-if="errors.command" class="field-error" role="alert">{{ errors.command }}</small>
    </label>
    <label>Arguments <span class="optional">one per line</span>
      <textarea v-model="form.args" name="args" rows="2" spellcheck="false" :aria-invalid="!!errors.args"></textarea>
      <small v-if="errors.args" class="field-error" role="alert">{{ errors.args }}</small>
    </label>
    </template>

    <details class="advanced">
      <summary>Advanced</summary>
      <template v-if="form.transport === 'stdio'">
      <label>Working directory <span class="optional">optional</span>
        <input v-model="form.cwd" name="cwd" autocomplete="off" spellcheck="false" :aria-invalid="!!errors.cwd" />
        <small v-if="errors.cwd" class="field-error" role="alert">{{ errors.cwd }}</small>
      </label>
      <label>Inherited environment variables <span class="optional">one per line, such as PATH</span>
        <textarea v-model="form.envAllowlist" name="envAllowlist" rows="2" spellcheck="false" :aria-invalid="!!errors.envAllowlist"></textarea>
        <small class="hint">The server inherits nothing else from Igor's environment.</small>
        <small v-if="errors.envAllowlist" class="field-error" role="alert">{{ errors.envAllowlist }}</small>
      </label>
      <label>Secrets <span class="optional">one NAME=IGOR_VARIABLE per line</span>
        <textarea v-model="form.env" name="env" rows="2" spellcheck="false" :aria-invalid="!!errors.env"></textarea>
        <small class="hint">Sets NAME in the server from Igor's environment variable IGOR_VARIABLE. Only the variable name is saved, never the value.</small>
        <small v-if="errors.env" class="field-error" role="alert">{{ errors.env }}</small>
      </label>
      </template>
      <div class="field-row">
        <label>Startup timeout (ms)
          <input v-model="form.startupTimeout" name="startupTimeout" inputmode="numeric" placeholder="10000" :aria-invalid="!!errors.startupTimeout" />
          <small v-if="errors.startupTimeout" class="field-error" role="alert">{{ errors.startupTimeout }}</small>
        </label>
        <label>Tool call timeout (ms)
          <input v-model="form.callTimeout" name="callTimeout" inputmode="numeric" placeholder="default" :aria-invalid="!!errors.callTimeout" />
          <small v-if="errors.callTimeout" class="field-error" role="alert">{{ errors.callTimeout }}</small>
        </label>
      </div>
    </details>

    <p v-if="errors.form" class="field-error" role="alert">{{ errors.form }}</p>

    <div v-if="tested" class="tested" role="status">
      <strong>Connected. {{ tested.length }} {{ tested.length === 1 ? 'tool' : 'tools' }} found.</strong>
      <ul><li v-for="tool in tested" :key="tool.name">{{ tool.name }}</li></ul>
      <small class="hint">New tools stay blocked until you allow them after saving.</small>
    </div>

    <div v-if="confirming" class="confirm" role="alertdialog" aria-labelledby="confirm-title">
      <p id="confirm-title"><strong>{{ form.transport === 'http' ? 'Connect to this server?' : 'Run this program on your computer?' }}</strong></p>
      <code class="command-line">{{ commandLine }}</code>
      <p class="hint">Only add servers you trust. Igor {{ form.transport === 'http' ? 'connects' : 'starts this program' }} now and whenever it opens.</p>
      <div class="actions">
        <button type="button" @click="save">{{ form.transport === 'http' ? 'Connect and save' : 'Run and save' }}</button>
        <button type="button" class="secondary" @click="confirming = false">Back</button>
      </div>
    </div>
    <div v-else class="actions">
      <button type="submit" :disabled="!!busy">{{ busy === 'save' ? 'Saving…' : 'Save' }}</button>
      <button type="button" class="secondary" :disabled="!!busy" @click="test">{{ busy === 'test' ? 'Testing…' : 'Test connection' }}</button>
      <button type="button" class="link" :disabled="!!busy" @click="emit('cancel')">Cancel</button>
    </div>
  </form>
</template>
