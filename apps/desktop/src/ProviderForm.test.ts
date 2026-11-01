import { flushPromises, mount } from '@vue/test-utils'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import { afterEach, expect, it } from 'vitest'
import ProviderForm from './ProviderForm.vue'
import ProviderSettings from './ProviderSettings.vue'
import type { ProviderEntry } from './providers'

afterEach(clearMocks)
const claude: ProviderEntry = { id: 'claude', kind: 'anthropic', model: 'test-model', base_url: null, api_key_env: 'IGOR_CLAUDE_KEY' }
function backend() {
  const calls: Array<{ command: string; args: any }> = []
  mockIPC((command, args: any) => { calls.push({ command, args }); return { providers: [{ entry: args?.request?.entry ?? claude, key_missing: 'IGOR_CLAUDE_KEY' }] } })
  return calls
}
it('requires valid identifiers, a model, a local or HTTPS URL and a key name for Anthropic', async () => {
  const calls = backend(); const wrapper = mount(ProviderForm, { props: { original: null } })
  await wrapper.get('form').trigger('submit'); await flushPromises()
  expect(wrapper.text()).toContain('Use lowercase'); expect(wrapper.text()).toContain('model identifier'); expect(wrapper.text()).toContain('base URL')
  await wrapper.get('[name=provider-kind]').setValue('anthropic')
  await wrapper.get('[name=provider-id]').setValue('claude'); await wrapper.get('[name=provider-model]').setValue('model')
  await wrapper.get('[name=provider-url]').setValue('http://example.com/v1')
  await wrapper.get('form').trigger('submit'); await flushPromises()
  expect(wrapper.text()).toContain('Enter an API key'); expect(wrapper.text()).toContain('Use HTTPS')
  expect(calls).toHaveLength(0); wrapper.unmount()
})
it.each(['sk-ant-private-value', '{ env = "IGOR_KEY" }', 'IGOR KEY', 'IGOR_KEY=value'])('shows the backend key-field rejection: %s', async (key) => {
  const calls: string[] = []
  mockIPC((command) => { calls.push(command); throw { field: 'api_key', message: 'Backend rejected the key: enter a variable name, never a value.' } })
  const wrapper = mount(ProviderForm, { props: { original: claude } })
  await wrapper.get('[name=provider-key-name]').setValue(key)
  await wrapper.get('form').trigger('submit'); await flushPromises()
  expect(wrapper.text()).toContain('Backend rejected the key: enter a variable name, never a value.')
  expect(wrapper.get('[name=provider-key-name]').attributes('aria-invalid')).toBe('true')
  expect(calls).toEqual(['settings_save_provider']); expect(wrapper.emitted('saved')).toBeUndefined(); wrapper.unmount()
})

it('saves only the variable name and preserves the edited provider id', async () => {
  const calls = backend(); const wrapper = mount(ProviderForm, { props: { original: claude } })
  await wrapper.get('form').trigger('submit'); await flushPromises()
  expect(calls).toEqual([{ command: 'settings_save_provider', args: { request: { replacing: 'claude', entry: claude } } }])
  expect(wrapper.emitted('saved')).toHaveLength(1)
  expect(wrapper.text()).toContain('Only the variable name is saved, never the value'); wrapper.unmount()
})
it('LM Studio preset clears the key and supplies a loopback URL', async () => {
  const calls = backend(); const wrapper = mount(ProviderForm, { props: { original: claude } })
  await wrapper.findAll('button').find(button => button.text() === 'LM Studio (local)')!.trigger('click')
  await wrapper.get('form').trigger('submit'); await flushPromises()
  expect(calls[0]?.args.request.entry).toEqual({ id: 'lm_studio', kind: 'openai_compatible', model: 'qwen/qwen3-vl-8b', base_url: 'http://127.0.0.1:1234/v1', api_key_env: null })
  wrapper.unmount()
})
it('lists the missing variable by name and requires confirmation to remove a provider', async () => {
  const calls = backend(); const wrapper = mount(ProviderSettings); await flushPromises()
  expect(wrapper.text()).toContain('Model providers'); expect(wrapper.text()).toContain('anthropic'); expect(wrapper.text()).toContain('Key variable IGOR_CLAUDE_KEY is not set')
  await wrapper.findAll('button').find(button => button.text() === 'Remove')!.trigger('click'); await flushPromises()
  expect(calls.some(call => call.command === 'settings_remove_provider')).toBe(false)
  await wrapper.findAll('button').find(button => button.text() === 'Confirm removal')!.trigger('click'); await flushPromises()
  expect(calls.find(call => call.command === 'settings_remove_provider')?.args).toEqual({ request: { id: 'claude' } }); wrapper.unmount()
})

it('sends a pasted key once and clears the password before the save completes', async () => {
  let finish: ((value: unknown) => void) | undefined
  let payload: any
  mockIPC((command, args) => {
    expect(command).toBe('settings_save_provider_key')
    payload = args
    return new Promise(resolve => { finish = resolve })
  })
  const wrapper = mount(ProviderForm, { props: { original: { ...claude, api_key_env: null } } })
  const input = wrapper.get<HTMLInputElement>('[name=provider-api-key]')
  await input.setValue('sk-private-sentinel-20261101')
  expect(wrapper.text()).not.toContain('sk-private-sentinel-20261101')
  await wrapper.get('form').trigger('submit')
  expect(input.element.value).toBe('')
  expect(payload.request.key).toBe('sk-private-sentinel-20261101')
  expect(payload.request.entry.api_key_env).toBeNull()
  finish?.({ providers: [] })
  await flushPromises()
  expect(input.element.value).toBe('')
  expect(wrapper.emitted('saved')).toHaveLength(1)
  wrapper.unmount()
})
it('clears a pasted key even when Keychain rejects the save', async () => {
  mockIPC(() => { throw { message: 'Could not save the key in Keychain. Try again.' } })
  const wrapper = mount(ProviderForm, { props: { original: { ...claude, api_key_env: null } } })
  await wrapper.get('[name=provider-api-key]').setValue('sk-sentinel')
  await wrapper.get('form').trigger('submit')
  await flushPromises()
  expect(wrapper.get<HTMLInputElement>('[name=provider-api-key]').element.value).toBe('')
  expect(wrapper.text()).toContain('Could not save the key in Keychain')
  wrapper.unmount()
})
it('shows saved Keychain status with Replace and Remove key actions', async () => {
  const calls: string[] = []
  const entry = { ...claude, api_key_env: null, api_key_keychain: 'claude' }
  mockIPC(command => {
    calls.push(command)
    return { providers: [{ entry, key_missing: null, key_saved: command !== 'settings_remove_provider_key' }] }
  })
  const wrapper = mount(ProviderSettings)
  await flushPromises()
  expect(wrapper.text()).toContain('Key saved in Keychain')
  expect(wrapper.findAll('button').some(button => button.text() === 'Replace')).toBe(true)
  await wrapper.findAll('button').find(button => button.text() === 'Remove key')!.trigger('click')
  await flushPromises()
  expect(calls).toContain('settings_remove_provider_key')
  expect(wrapper.text()).toContain('No key saved in Keychain')
  wrapper.unmount()
})
