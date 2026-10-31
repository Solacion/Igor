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
export interface ServerEntry {
  id: string
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
  tools: ToolView[]
  permissions: Record<string, string>
}

export interface SettingsView {
  config_path: string
  servers: ServerView[]
}

export type PermissionClass = 'read' | 'write' | 'denied'

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

export function saveServer(replacing: string | null, entry: ServerEntry): Promise<SettingsView> {
  return invoke<SettingsView>('settings_save_server', { request: { replacing, entry } })
}

export function testServer(replacing: string | null, entry: ServerEntry): Promise<ToolView[]> {
  return invoke<ToolView[]>('settings_test_server', { request: { replacing, entry } })
}

export function removeServer(id: string): Promise<SettingsView> {
  return invoke<SettingsView>('settings_remove_server', { request: { id } })
}

export function reconnectServer(id: string): Promise<SettingsView> {
  return invoke<SettingsView>('settings_reconnect_server', { request: { id } })
}

export function setPermission(server: string, tool: string, cls: PermissionClass | null): Promise<SettingsView> {
  return invoke<SettingsView>('settings_set_permission', { request: { server, tool, class: cls } })
}

/** A local Settings destination; never a URL or arbitrary DOM selector. */
export type SettingsSection = 'mcp-servers' | 'model-providers'
