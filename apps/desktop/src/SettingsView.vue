<script setup lang="ts">
import { nextTick, onMounted, onUnmounted, reactive, ref, watch } from 'vue'
import { useSettingsStore } from './stores/settings'
import VoiceSettings from './VoiceSettings.vue'
import ServerForm from './ServerForm.vue'
import ProviderSettings from './ProviderSettings.vue'
import { isCommandError, reconnectServer, removeServer, setPermission, setServerEnabled, signInServer, signOutServer } from './bridge'
import type { PermissionClass, ServerEntry, ServerView, SettingsView, ToolView, SettingsSection } from './bridge'
const props = withDefaults(defineProps<{
  initialSection?: SettingsSection
}>(), { initialSection: 'mcp-servers' })
const section = ref<SettingsSection>(props.initialSection)
async function focusSection(next: SettingsSection) {
  section.value = next
  await nextTick()
  const target = document.getElementById(next)
  target?.setAttribute('tabindex', '-1')
  target?.scrollIntoView?.({ block: 'start' })
  target?.focus({ preventScroll: true })
}
watch(() => props.initialSection, next => { void focusSection(next); })
const emit = defineEmits<{
  close: [
  ]
}>()
/** How often the server list refreshes connection status while Settings is open. */
const store = useSettingsStore()
const view = store.settings
const loadError = ref<string | null>(null)
const actionError = ref<string | null>(null)
/** `new` for the add form, or the id of the server being edited. */
const editing = ref<string | null>(null)
const confirmingRemove = ref<string | null>(null)
const busy = ref<string | null>(null)
const unsubscribe = store.subscribe(() => { void refresh(); })
async function refresh() {
  try {
    view.value = await store.refreshSettings()
    loadError.value = null
  }
  catch (error) {
    loadError.value = isCommandError(error) ? error.message : 'Settings are only available in the desktop window.'
  }
}
async function act(key: string, action: () => Promise<SettingsView>) {
  if (busy.value)
    return
  busy.value = key
  actionError.value = null
  try {
    view.value = await action()
  }
  catch (error) {
    actionError.value = isCommandError(error) ? error.message : 'Igor could not complete this. Try again.'
  }
  finally {
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
type Choice = 'always' | 'ask' | 'never'
/** Permission choices as the operator sees them. */
const permissionOptions: { value: Choice, label: string }[] = [
  { value: 'always', label: 'Always allow' },
  { value: 'ask', label: 'Ask first' },
  { value: 'never', label: 'Never' },
]
/** True when the server says the tool may change data, so a `read` tool still asks first. */
function mayModify(tool: ToolView): boolean {
  return tool.read_only_hint === false || tool.destructive_hint === true
}
/** What the tool will actually do: a `read` tool the server says changes data asks first. */
function choiceOf(server: ServerView, tool: ToolView): Choice | null {
  switch (server.permissions[tool.name]) {
    case 'allow': return 'always'
    case 'read': return mayModify(tool) ? 'ask' : 'always'
    case 'write': return 'ask'
    case 'denied': return 'never'
    default: return null
  }
}
function choose(server: ServerView, tool: ToolView, choice: Choice) {
  // `allow` only where it differs from `read`: it overrides the server's own warning.
  const cls: PermissionClass = choice === 'always' ? (mayModify(tool) ? 'allow' : 'read') : choice === 'ask' ? 'write' : 'denied'
  permission(server.entry.id, tool.name, cls)
}
function permission(server: string, tool: string, value: string) {
  const cls = value === '' ? null : (value as PermissionClass)
  void act(`permission:${server}:${tool}`, () => setPermission(server, tool, cls))
}
/** Live tools, plus tools that have a permission but are not currently reported. */
/** Search text per server for its tool list. */
const toolSearch = reactive<Record<string, string>>({})
/** The server's tools whose name or description contains every word of its search. */
function shownTools(server: ServerView): ToolView[] {
  const words = (toolSearch[server.entry.id] ?? '').toLowerCase().split(/\s+/).filter(Boolean)
  return tools(server).filter(tool => words.every(word => `${tool.name} ${tool.description ?? ''}`.toLowerCase().includes(word)))
}
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
    case 'disabled': return 'Off'
    case 'sign_in_needed': return 'Sign-in needed'
    case 'failed': return 'Not connected'
    default: return 'Not connected yet'
  }
}
function commandLine(entry: ServerEntry): string {
  if (entry.transport === 'http' || entry.transport === 'sse')
    return entry.url ?? ''
  return [entry.command, ...entry.args].join(' ')
}
function editingEntry(): ServerEntry | null {
  return view.value?.servers.find((server) => server.entry.id === editing.value)?.entry ?? null
}
onMounted(() => {
  void focusSection(props.initialSection)
  void refresh()
})
onUnmounted(unsubscribe)
</script>

<template>
  <section class="settings" aria-labelledby="settings-title">
    <div class="settings-layout">
      <nav class="settings-nav" aria-label="Settings sections">
        <button type="button" class="settings-back" @click="emit('close')"><svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M7.5 2.5L4 6l3.5 3.5"/></svg>Conversation</button>
        <h1 id="settings-title">Settings</h1>
        <a href="#mcp-servers" :class="{ 'is-active': section === 'mcp-servers' }" :aria-current="section === 'mcp-servers' ? 'location' : undefined" @click.prevent="focusSection('mcp-servers')"><span class="nav-icon" data-icon="servers" aria-hidden="true">S</span>MCP servers</a>
        <a href="#model-providers" :class="{ 'is-active': section === 'model-providers' }" :aria-current="section === 'model-providers' ? 'location' : undefined" @click.prevent="focusSection('model-providers')"><span class="nav-icon" data-icon="models" aria-hidden="true">M</span>Model providers</a>
        <a href="#voice" :class="{ 'is-active': section === 'voice' }" :aria-current="section === 'voice' ? 'location' : undefined" @click.prevent="focusSection('voice')"><span class="nav-icon" data-icon="voice" aria-hidden="true">V</span>Voice</a>
      </nav>

      <div class="settings-body">
        <section v-show="section === 'mcp-servers'" id="mcp-servers" aria-labelledby="servers-title">
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
                  <div class="server-head-controls">
                    <span class="status" :data-state="server.status.state">
                      <span class="readiness-dot" :class="{ 'is-ready': server.status.state === 'connected', 'is-unavailable': server.status.state === 'failed', 'is-off': server.status.state === 'disabled' }" aria-hidden="true"></span>
                      {{ statusText(server) }}
                    </span>
                    <input type="checkbox" role="switch" class="switch" :checked="server.entry.enabled !== false" :disabled="!!busy" :aria-label="`Use ${server.entry.id}`" :title="server.entry.enabled !== false ? 'Turn off: keeps its settings, stops offering its tools' : 'Turn on'" @change="act(`enabled:${server.entry.id}`, () => setServerEnabled(server.entry.id, ($event.target as HTMLInputElement).checked))" />
                  </div>
                </div>
                <p v-if="server.status.state === 'failed'" class="status-detail">{{ server.status.detail }}</p>

                <details v-if="server.recent_stderr?.length" class="server-output">
                  <summary>Server output (last lines)</summary>
                  <pre>{{ server.recent_stderr.join('\n') }}</pre>
                </details>

                <details v-if="tools(server).length" class="tools">
                  <summary>Tools and permissions ({{ tools(server).length }})</summary>
                  <div v-if="tools(server).length > 5" class="tool-search">
                    <label :for="`tool-search-${server.entry.id}`" class="sr-only">Search tools of {{ server.entry.id }}</label>
                    <input :id="`tool-search-${server.entry.id}`" v-model="toolSearch[server.entry.id]" type="search" placeholder="Search tools" autocomplete="off" spellcheck="false" />
                    <span class="hint" role="status">{{ shownTools(server).length }} of {{ tools(server).length }}</span>
                  </div>
                  <p v-if="!shownTools(server).length" class="hint">No tools match.</p>
                  <ul v-else class="tool-rows">
                    <li v-for="tool in shownTools(server)" :key="tool.name" class="tool-row">
                      <div class="tool-info">
                        <strong>{{ tool.name }}</strong>
                        <p v-if="tool.description" class="hint">{{ tool.description }}</p>
                        <p class="hint">
                          <span v-if="tool.read_only_hint">Server says: read-only. </span>
                          <span v-if="tool.destructive_hint">Server says: may change or delete data. </span>
                          <span v-if="!server.tools.some((live) => live.name === tool.name)">Not reported by the server right now.</span>
                          <span v-if="!server.permissions[tool.name]">Not allowed yet. </span>
                          <span v-if="server.permissions[tool.name] === 'read' && mayModify(tool)">Asks first because the server says it changes data; choose Always allow to stop asking. </span>
                          <span v-if="server.permissions[tool.name] === 'allow' && mayModify(tool)">Runs without asking although the server says it changes data. </span>
                        </p>
                      </div>
                      <div class="segmented" role="radiogroup" :aria-label="`Permission for ${tool.name}`">
                        <label v-for="option in permissionOptions" :key="option.value" :class="{ 'is-on': choiceOf(server, tool) === option.value }">
                          <input type="radio" :name="`permission-${server.entry.id}-${tool.name}`" :value="option.value" :checked="choiceOf(server, tool) === option.value" :disabled="!!busy" @change="choose(server, tool, option.value)" />{{ option.label }}
                        </label>
                      </div>
                    </li>
                  </ul>
                  <p class="hint">Server descriptions are hints only. Use Ask first for anything that changes data.</p>
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
                  <button v-if="server.entry.auth === 'oauth' && server.status.state !== 'connected'" type="button" :disabled="!!busy" @click="act(`sign-in:${server.entry.id}`, () => signInServer(server.entry.id))">{{ busy === `sign-in:${server.entry.id}` ? 'Waiting for the browser…' : 'Sign in' }}</button>
                  <button v-if="server.entry.auth === 'oauth' && server.status.state === 'connected'" type="button" class="secondary" :disabled="!!busy" @click="act(`sign-out:${server.entry.id}`, () => signOutServer(server.entry.id))">Sign out</button>
                  <button type="button" class="secondary" :disabled="!!busy" @click="act(`reconnect:${server.entry.id}`, () => reconnectServer(server.entry.id))">{{ busy === `reconnect:${server.entry.id}` ? 'Connecting…' : 'Reconnect' }}</button>
                  <button type="button" class="link danger" :disabled="!!busy" @click="confirmingRemove = server.entry.id">Remove</button>
                </div>
              </template>
            </li>
          </ul>

          <p v-if="view" class="hint config-path">Saved in {{ view.config_path }}</p>
        </section>
        <ProviderSettings v-show="section === 'model-providers'" />
        <VoiceSettings v-show="section === 'voice'" />
      </div>
    </div>
  </section>
</template>

<style scoped>
.server-output pre { white-space: pre-wrap; overflow-wrap: anywhere; }
</style>
