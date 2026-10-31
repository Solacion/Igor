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
  expect(wrapper.text()).toContain('environment variable name'); expect(wrapper.text()).toContain('Use HTTPS')
  expect(calls).toHaveLength(0); wrapper.unmount()
})
it.each(['sk-ant-private-value', '{ env = "IGOR_KEY" }', 'IGOR KEY', 'IGOR_KEY=value'])('rejects a pasted value/expression without crossing IPC: %s', async (key) => {
  const calls = backend(); const wrapper = mount(ProviderForm, { props: { original: claude } })
  await wrapper.get('[name=provider-key-name]').setValue(key)
  await wrapper.get('form').trigger('submit'); await flushPromises()
  expect(wrapper.text()).toContain('environment variable NAME, never an API key value')
  expect(calls).toHaveLength(0); wrapper.unmount()
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
  expect(wrapper.text()).toContain('Model providers'); expect(wrapper.text()).toContain('anthropic'); expect(wrapper.text()).toContain('Key variable IGOR_CLAUDE_KEY not set')
  await wrapper.findAll('button').find(button => button.text() === 'Remove')!.trigger('click'); await flushPromises()
  expect(calls.some(call => call.command === 'settings_remove_provider')).toBe(false)
  await wrapper.findAll('button').find(button => button.text() === 'Confirm removal')!.trigger('click'); await flushPromises()
  expect(calls.find(call => call.command === 'settings_remove_provider')?.args).toEqual({ request: { id: 'claude' } }); wrapper.unmount()
})
