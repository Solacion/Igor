<script setup lang="ts">
import { nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import ServerForm from './ServerForm.vue'
import ProviderSettings from './ProviderSettings.vue'
import { isCommandError, readSettings, reconnectServer, removeServer, setPermission } from './bridge'
import type { PermissionClass, ServerEntry, ServerView, SettingsView, ToolView, SettingsSection } from './bridge'

const props = withDefaults(defineProps<{ initialSection?: SettingsSection }>(), { initialSection: 'mcp-servers' })
const section = ref<SettingsSection>(props.initialSection)
async function focusSection(next: SettingsSection) {
  section.value = next
  await nextTick()
  const target = document.getElementById(next)
  target?.setAttribute('tabindex', '-1')
  target?.scrollIntoView?.({ block: 'start' })
  target?.focus({ preventScroll: true })
}
watch(() => props.initialSection, next => { void focusSection(next) })

const emit = defineEmits<{ close: [] }>()

/** How often the server list refreshes connection status while Settings is open. */
const REFRESH_MS = 3000

const view = ref<SettingsView | null>(null)
const loadError = ref<string | null>(null)
const actionError = ref<string | null>(null)
/** `new` for the add form, or the id of the server being edited. */
const editing = ref<string | null>(null)
const confirmingRemove = ref<string | null>(null)
const busy = ref<string | null>(null)
let timer: ReturnType<typeof setInterval> | undefined

async function refresh() {
  try {
    view.value = await readSettings()
    loadError.value = null
  } catch (error) {
    loadError.value = isCommandError(error) ? error.message : 'Settings are only available in the desktop window.'
  }
}

async function act(key: string, action: () => Promise<SettingsView>) {
  if (busy.value) return
  busy.value = key
  actionError.value = null
  try {
    view.value = await action()
  } catch (error) {
    actionError.value = isCommandError(error) ? error.message : 'Igor could not complete this. Try again.'
  } finally {
    busy.value = null
  }
}

function saved(next: SettingsView) {
  view.value = next
  editing.value = null
}

function remove(id: string) {
  confirmingRemove.value = null
  void act(`remove:${id}`, () => removeServer(id))
}

function permission(server: string, tool: string, value: string) {
  const cls = value === '' ? null : (value as PermissionClass)
  void act(`permission:${server}:${tool}`, () => setPermission(server, tool, cls))
}

/** Live tools, plus tools that have a permission but are not currently reported. */
function tools(server: ServerView): ToolView[] {
  const live = new Set(server.tools.map((tool) => tool.name))
  const configuredOnly = Object.keys(server.permissions)
    .filter((name) => !live.has(name))
    .map((name) => ({ name, description: null, read_only_hint: null, destructive_hint: null }))
  return [...server.tools, ...configuredOnly]
}

function statusText(server: ServerView): string {
  switch (server.status.state) {
    case 'connected': return 'Connected'
    case 'starting': return 'Starting…'
    case 'failed': return 'Not connected'
    default: return 'Not connected yet'
  }
}

function commandLine(entry: ServerEntry): string {
  return [entry.command, ...entry.args].join(' ')
}

function editingEntry(): ServerEntry | null {
  return view.value?.servers.find((server) => server.entry.id === editing.value)?.entry ?? null
}

onMounted(() => {
  void focusSection(props.initialSection)
  void refresh()
  timer = setInterval(() => { if (!busy.value) void refresh() }, REFRESH_MS)
})
onUnmounted(() => clearInterval(timer))
</script>

<template>
  <section class="settings" aria-labelledby="settings-title">
    <header class="settings-header">
      <h1 id="settings-title">Settings</h1>
      <button type="button" class="secondary" @click="emit('close')">Done</button>
    </header>

    <div class="settings-layout">
      <nav class="settings-nav" aria-label="Settings sections">
        <a href="#mcp-servers" :class="{ 'is-active': section === 'mcp-servers' }" :aria-current="section === 'mcp-servers' ? 'location' : undefined" @click.prevent="focusSection('mcp-servers')">MCP servers</a>
        <a href="#model-providers" :class="{ 'is-active': section === 'model-providers' }" :aria-current="section === 'model-providers' ? 'location' : undefined" @click.prevent="focusSection('model-providers')">Model providers</a>
      </nav>

      <div class="settings-body">
        <section id="mcp-servers" aria-labelledby="servers-title">
          <div class="section-head">
            <div>
              <h2 id="servers-title">MCP servers</h2>
              <p class="hint">Igor uses the tools these servers provide. A new tool is blocked until you allow it.</p>
            </div>
            <button v-if="view && editing === null" type="button" @click="editing = 'new'">Add server</button>
          </div>

          <p v-if="loadError" class="field-error" role="alert">{{ loadError }}</p>
          <p v-if="actionError" class="field-error" role="alert">{{ actionError }}</p>

          <ServerForm v-if="editing === 'new'" :original="null" @saved="saved" @cancel="editing = null" />

          <p v-if="view && !view.servers.length && editing !== 'new'" class="empty">No servers yet. Add one to give Igor tools to work with.</p>

          <ul v-if="view" class="server-list">
            <li v-for="server in view.servers" :key="server.entry.id" class="server-card">
              <ServerForm v-if="editing === server.entry.id" :original="editingEntry()" @saved="saved" @cancel="editing = null" />
              <template v-else>
                <div class="server-head">
                  <div>
                    <h3>{{ server.entry.id }}</h3>
                    <code class="command-line">{{ commandLine(server.entry) }}</code>
                  </div>
                  <span class="status" :data-state="server.status.state">
                    <span class="readiness-dot" :class="{ 'is-ready': server.status.state === 'connected', 'is-unavailable': server.status.state === 'failed' }" aria-hidden="true"></span>
                    {{ statusText(server) }}
                  </span>
                </div>
                <p v-if="server.status.state === 'failed'" class="status-detail">{{ server.status.detail }}</p>

                <details v-if="server.recent_stderr?.length" class="server-output">
                  <summary>Server output (last lines)</summary>
                  <pre>{{ server.recent_stderr.join('\n') }}</pre>
                </details>

                <details v-if="tools(server).length" class="tools">
                  <summary>Tools and permissions ({{ tools(server).length }})</summary>
                  <table>
                    <thead><tr><th scope="col">Tool</th><th scope="col">Igor may</th></tr></thead>
                    <tbody>
                      <tr v-for="tool in tools(server)" :key="tool.name">
                        <td>
                          <strong>{{ tool.name }}</strong>
                          <p v-if="tool.description" class="hint">{{ tool.description }}</p>
                          <p class="hint">
                            <span v-if="tool.read_only_hint">Server says: read-only. </span>
                            <span v-if="tool.destructive_hint">Server says: may change or delete data. </span>
                            <span v-if="!server.tools.some((live) => live.name === tool.name)">Not reported by the server right now.</span>
                          </p>
                        </td>
                        <td>
                          <select :aria-label="`Permission for ${tool.name}`" :value="server.permissions[tool.name] ?? ''" :disabled="!!busy" @change="permission(server.entry.id, tool.name, ($event.target as HTMLSelectElement).value)">
                            <option value="">Not allowed (not set)</option>
                            <option value="read">Use it freely (read)</option>
                            <option value="write">Ask me each time (write)</option>
                            <option value="denied">Never (denied)</option>
                          </select>
                        </td>
                      </tr>
                    </tbody>
                  </table>
                  <p class="hint">Server descriptions are hints only. Use "Ask me each time" for anything that changes data.</p>
                </details>

                <div v-if="confirmingRemove === server.entry.id" class="confirm" role="alertdialog" :aria-label="`Remove ${server.entry.id}`">
                  <p>Remove <strong>{{ server.entry.id }}</strong> and its tool permissions?</p>
                  <div class="actions">
                    <button type="button" @click="remove(server.entry.id)">Remove</button>
                    <button type="button" class="secondary" @click="confirmingRemove = null">Keep</button>
                  </div>
                </div>
                <div v-else class="actions">
                  <button type="button" class="secondary" :disabled="!!busy || editing !== null" @click="editing = server.entry.id">Edit</button>
                  <button type="button" class="secondary" :disabled="!!busy" @click="act(`reconnect:${server.entry.id}`, () => reconnectServer(server.entry.id))">{{ busy === `reconnect:${server.entry.id}` ? 'Connecting…' : 'Reconnect' }}</button>
                  <button type="button" class="link danger" :disabled="!!busy" @click="confirmingRemove = server.entry.id">Remove</button>
                </div>
              </template>
            </li>
          </ul>

          <p v-if="view" class="hint config-path">Saved in {{ view.config_path }}</p>
        </section>
        <ProviderSettings />
      </div>
    </div>
  </section>
</template>

<style scoped>
.server-output pre { white-space: pre-wrap; overflow-wrap: anywhere; }
</style>
