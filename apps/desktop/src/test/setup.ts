import { vi } from 'vitest'
vi.mock('@tauri-apps/api/event', async () => {
  const { settingsListeners } = await import('./settingsEvents')
  return {
    listen: async (name: string, callback: () => void) => {
      if (name !== 'settings-changed') throw new Error('Unexpected native event')
      settingsListeners.add(callback)
      return () => settingsListeners.delete(callback)
    },
  }
})
