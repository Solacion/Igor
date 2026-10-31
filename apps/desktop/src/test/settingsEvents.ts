export const settingsListeners = new Set<() => void>()
export function emitSettingsChanged() {
  for (const listener of settingsListeners) listener()
}
