import { mount, flushPromises } from '@vue/test-utils'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import { afterEach, describe, expect, it } from 'vitest'
import App from './App.vue'

afterEach(clearMocks)

describe('desktop status bridge', () => {
  it('calls only the read-only status command and shows readiness in the footer', async () => {
    mockIPC((command, args) => {
      expect(command).toBe('desktop_status')
      expect(args).toEqual({ request: { scope: 'runtime' } })
      return { app_version: '0.1.0', core_event_schema: 1 }
    })
    const wrapper = mount(App)
    await flushPromises()
    expect(wrapper.get('footer [role="status"]').text()).toBe('Ready')
    expect(wrapper.get('footer').text()).toContain('v0.1.0')
    expect(wrapper.get('.readiness-dot').classes()).toContain('is-ready')
    expect(wrapper.find('button').exists()).toBe(false)
    expect(wrapper.text()).not.toContain('Check connection')
    wrapper.unmount()
  })

  it('shows a fixed error without leaking native diagnostics, and supports retry', async () => {
    let calls = 0
    mockIPC(() => {
      calls++
      if (calls === 1) throw new Error('private-config-path-and-secret')
      return { app_version: '0.1.0', core_event_schema: 1 }
    })
    const wrapper = mount(App)
    await flushPromises()
    expect(wrapper.get('[role="alert"]').text()).toContain('Desktop unavailable')
    expect(wrapper.text()).not.toContain('private-config-path-and-secret')
    await wrapper.get('button').trigger('click')
    await flushPromises()
    expect(wrapper.get('footer [role="status"]').text()).toBe('Ready')
    expect(calls).toBe(2)
    wrapper.unmount()
  })

  it('shows pending readiness without a green dot and renders native text literally', async () => {
    let finish: ((value: unknown) => void) | undefined
    mockIPC(() => new Promise((resolve) => { finish = resolve }))
    const wrapper = mount(App)
    await flushPromises()
    expect(wrapper.get('footer [role="status"]').text()).toBe('Starting…')
    expect(wrapper.get('.readiness-dot').classes()).not.toContain('is-ready')
    expect(wrapper.find('button').exists()).toBe(false)
    finish?.({ app_version: '<img src=x onerror=alert(1)>', core_event_schema: 1 })
    await flushPromises()
    expect(wrapper.find('img').exists()).toBe(false)
    expect(wrapper.text()).toContain('<img src=x onerror=alert(1)>')
    expect(wrapper.get('.readiness-dot').classes()).toContain('is-ready')
    wrapper.unmount()
  })
})
