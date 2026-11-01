import { invoke } from '@tauri-apps/api/core'

export interface DesktopStatus {
  app_version: string
  core_event_schema: number
}

/** Read-only status of the embedded core. */
export async function readDesktopStatus(): Promise<DesktopStatus> {
  return invoke<DesktopStatus>('desktop_status', { request: { scope: 'runtime' } })
}

/** An MCP server entry as written in the configuration file. `env` maps each variable the
 * server receives to the name of the environment variable holding its value, never a value. */
/** Where a server secret is read from: an environment variable or a Keychain item. */
export type SecretEntry = { env: string } | { keychain: string }

export interface ServerEntry {
  id: string
  /** False when the operator turned the server off. Absent means on. */
  enabled?: boolean
  /** `stdio` for a local process, `http` for a remote server, `sse` for a remote server on the
   * legacy HTTP+SSE transport. Absent means `stdio`. */
  transport?: 'stdio' | 'http' | 'sse'
  /** Remote endpoint, for `http`. */
  url?: string | null
  /** Sent as `Authorization: Bearer <token>`, for `http`. */
  bearer_token?: SecretEntry | null
  /** Headers sent with every request, for `http`. */
  headers?: Record<string, SecretEntry>
  /** `oauth` when the remote server needs sign-in. */
  auth?: 'oauth' | null
  command: string
  args: string[]
  cwd: string | null
  env_allowlist: string[]
  env: Record<string, string>
  startup_timeout_ms: number | null
  call_timeout_ms: number | null
}

export type ServerStatus =
  | { state: 'not_connected' }
  | { state: 'starting' }
  | { state: 'connected' }
  | { state: 'disabled' }
  | { state: 'sign_in_needed' }
  | { state: 'failed'; detail: string }

/** A tool reported by a server. Every text field is untrusted server output. */
export interface ToolView {
  name: string
  description: string | null
  read_only_hint: boolean | null
  destructive_hint: boolean | null
}

export interface ServerView {
  entry: ServerEntry
  status: ServerStatus
  /** Core-redacted server output; render only as plain text. */
  recent_stderr?: string[]
  tools: ToolView[]
  permissions: Record<string, string>
}

export interface SettingsView {
  config_path: string
  servers: ServerView[]
}

/** `allow` is Always allow; `read` still asks when the server says a tool changes data. */
export type PermissionClass = 'read' | 'allow' | 'write' | 'denied'

/** A rejected settings command. `field` names the entry field at fault, such as `command`. */
export interface CommandError {
  field: string | null
  message: string
}

export function isCommandError(value: unknown): value is CommandError {
  return typeof value === 'object' && value !== null && typeof (value as CommandError).message === 'string'
}

export function readSettings(): Promise<SettingsView> {
  return invoke<SettingsView>('settings_view')
}

/** `secret` is a key typed for a remote server; the backend stores it in the Keychain. */
export function saveServer(replacing: string | null, entry: ServerEntry, secret: string | null = null): Promise<SettingsView> {
  return invoke<SettingsView>('settings_save_server', { request: { replacing, entry, secret } })
}

export function testServer(replacing: string | null, entry: ServerEntry, secret: string | null = null): Promise<ToolView[]> {
  return invoke<ToolView[]>('settings_test_server', { request: { replacing, entry, secret } })
}

export function removeServer(id: string): Promise<SettingsView> {
  return invoke<SettingsView>('settings_remove_server', { request: { id } })
}

export function reconnectServer(id: string): Promise<SettingsView> {
  return invoke<SettingsView>('settings_reconnect_server', { request: { id } })
}

/** Turns a server on (connecting it) or off, keeping its settings and permissions. */
export function setServerEnabled(id: string, enabled: boolean): Promise<SettingsView> {
  return invoke<SettingsView>('settings_set_server_enabled', { request: { id, enabled } })
}

/** Opens the server's sign-in page in the browser and waits for it to finish. */
export function signInServer(id: string): Promise<SettingsView> {
  return invoke<SettingsView>('settings_sign_in_server', { request: { id } })
}

export function signOutServer(id: string): Promise<SettingsView> {
  return invoke<SettingsView>('settings_sign_out_server', { request: { id } })
}

export function setPermission(server: string, tool: string, cls: PermissionClass | null): Promise<SettingsView> {
  return invoke<SettingsView>('settings_set_permission', { request: { server, tool, class: cls } })
}

/** A local Settings destination; never a URL or arbitrary DOM selector. */
export type SettingsSection = 'mcp-servers' | 'model-providers' | 'voice'
