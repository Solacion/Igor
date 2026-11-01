import { invoke } from '@tauri-apps/api/core'

export interface ProviderEntry {
  id: string
  kind: 'anthropic' | 'openai_compatible'
  model: string
  base_url: string | null
  api_key_env: string | null
  api_key_keychain?: string | null
}
export interface ProviderSettingsView {
  providers: Array<{ entry: ProviderEntry; key_missing: string | null; key_saved?: boolean | null }>
}
export const readProviderSettings = () => invoke<ProviderSettingsView>('settings_providers')
export const saveProvider = (replacing: string | null, entry: ProviderEntry) =>
  invoke<ProviderSettingsView>('settings_save_provider', { request: { replacing, entry } })
export const removeProvider = (id: string) =>
  invoke<ProviderSettingsView>('settings_remove_provider', { request: { id } })


/** Write-only request: callers read straight from the password element and immediately clear it. */
export const saveProviderKey = (replacing: string | null, entry: ProviderEntry, key: string) =>
  invoke<ProviderSettingsView>('settings_save_provider_key', { request: { replacing, entry, key } })
export const removeProviderKey = (id: string) =>
  invoke<ProviderSettingsView>('settings_remove_provider_key', { request: { id } })
