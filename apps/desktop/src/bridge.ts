import { invoke } from '@tauri-apps/api/core'

export interface DesktopStatus {
  app_version: string
  core_event_schema: number
}

/** Read-only, capability-restricted bridge. No configuration or credentials cross IPC. */
export async function readDesktopStatus(): Promise<DesktopStatus> {
  return invoke<DesktopStatus>('desktop_status', { request: { scope: 'runtime' } })
}
