import { invoke } from '@tauri-apps/api/core'

export interface ProviderEntry {
  id: string
  kind: 'anthropic' | 'openai_compatible'
  model: string
  base_url: string | null
  api_key_env: string | null
}
export interface ProviderSettingsView {
  providers: Array<{ entry: ProviderEntry; key_missing: string | null }>
}
export const readProviderSettings = () => invoke<ProviderSettingsView>('settings_providers')
export const saveProvider = (replacing: string | null, entry: ProviderEntry) =>
  invoke<ProviderSettingsView>('settings_save_provider', { request: { replacing, entry } })
export const removeProvider = (id: string) =>
  invoke<ProviderSettingsView>('settings_remove_provider', { request: { id } })

/** Both mounted pages use this signal; the native configuration remains the source of truth. */
export function providersChanged() { window.dispatchEvent(new Event('igor:providers-changed')) }
