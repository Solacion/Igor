import { inject, onScopeDispose, provide, ref, type InjectionKey } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { readSettings, type SettingsView } from '../bridge'
import { readProviders, type ProviderView } from '../session'
import { readProviderSettings, type ProviderSettingsView } from '../providers'

export function createSettingsStore() {
  const settings = ref<SettingsView | null>(null)
  const providers = ref<ProviderView[]>([])
  const providerSettings = ref<ProviderSettingsView | null>(null)
  let users = 0
  let unlisten: UnlistenFn | undefined
  let generation = 0
  const listeners = new Set<() => void>()
  function subscribe(callback: () => void) {
    listeners.add(callback)
    return () => listeners.delete(callback)
  }
  function changed() {
    for (const callback of listeners) callback()
  }
  function connect() {
    users++
    if (users !== 1) return
    const current = ++generation
    void listen('settings-changed', changed).then(stop => {
      if (generation !== current || !users) stop()
      else unlisten = stop
    }).catch(() => { /* Native-only event bridge. Reads still report their own failures. */ })
  }
  function disconnect() {
    users--
    if (users) return
    generation++
    unlisten?.()
    unlisten = undefined
  }
  async function refreshSettings() {
    const next = await readSettings()
    settings.value = next
    return next
  }
  async function refreshProviders() {
    const next = await readProviders()
    providers.value = next.providers
    return next
  }
  async function refreshProviderSettings() {
    const next = await readProviderSettings()
    providerSettings.value = next
    return next
  }
  return { settings, providers, providerSettings, subscribe, connect, disconnect, refreshSettings, refreshProviders, refreshProviderSettings }
}
export type SettingsStore = ReturnType<typeof createSettingsStore>
const settingsKey: InjectionKey<SettingsStore> = Symbol('settings')
export function provideSettingsStore() {
  const store = createSettingsStore()
  provide(settingsKey, store)
  return store
}
export function useSettingsStore() {
  const store = inject(settingsKey, undefined) ?? createSettingsStore()
  store.connect()
  onScopeDispose(store.disconnect)
  return store
}
