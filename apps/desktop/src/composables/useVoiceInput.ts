import { onMounted, onScopeDispose, ref, watch } from 'vue'
import { monoFrame, resample16k } from '../voice/audio'
import { transcription, type Transcriber } from '../voice/transcription'
interface Capture {
  stream: MediaStream
  context: AudioContext
  source: MediaStreamAudioSourceNode
  processor: ScriptProcessorNode
  mute: GainNode
  chunks: Float32Array[]
  count: number
  peak: number
}
export function useVoiceInput(options: {
  active: () => boolean
  busy: () => boolean
  onStart: () => void
  onText: (text: string) => void | Promise<void>
  onError: (message: string) => void
}, transcriber: Transcriber = transcription) {
  const state = ref<'Idle' | 'Listening' | 'Transcribing'>('Idle')
  const level = ref(0)
  const error = ref('')
  let pressed = false
  let generation = 0
  let capture: Capture | undefined
  let pendingStream: MediaStream | undefined
  let pendingContext: AudioContext | undefined
  let limit: ReturnType<typeof setTimeout> | undefined
  let spaceHeld = false
  function report(message: string) {
    error.value = message
    options.onError(message)
  }
  function disposeCapture(recording: Capture | undefined) {
    clearTimeout(limit)
    level.value = 0
    if (pendingStream) {
      pendingStream.getTracks().forEach(track => track.stop())
      pendingStream = undefined
    }
    if (pendingContext) {
      void pendingContext.close().catch(() => { })
      pendingContext = undefined
    }
    if (!recording)
      return
    recording.processor.onaudioprocess = null
    recording.stream.getTracks().forEach(track => track.stop())
    for (const node of [recording.source, recording.processor, recording.mute]) {
      try {
        node.disconnect()
      }
      catch { /* Already disconnected after a device error. */ }
    }
    void recording.context.close().catch(() => { })
  }
  function cancel() {
    generation++
    pressed = false
    spaceHeld = false
    const recording = capture
    capture = undefined
    disposeCapture(recording)
    state.value = 'Idle'
  }
  async function start() {
    if (pressed || state.value !== 'Idle' || options.busy() || !options.active() || !transcriber.available)
      return
    pressed = true
    const token = ++generation
    error.value = ''
    state.value = 'Listening'
    options.onStart()
    let stream: MediaStream | undefined
    let context: AudioContext | undefined
    try {
      if (!navigator.mediaDevices?.getUserMedia || !globalThis.AudioContext)
        throw new Error('unsupported')
      stream = await navigator.mediaDevices.getUserMedia({ audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true }, video: false })
      if (!pressed || token !== generation) {
        stream.getTracks().forEach(track => track.stop())
        return
      }
      pendingStream = stream
      context = new AudioContext()
      pendingContext = context
      await context.resume()
      if (!pressed || token !== generation) {
        stream.getTracks().forEach(track => track.stop())
        await context.close()
        return
      }
      const source = context.createMediaStreamSource(stream)
      const processor = context.createScriptProcessor(4096, 1, 1)
      const mute = context.createGain()
      mute.gain.value = 0
      const recording: Capture = { stream, context, source, processor, mute, chunks: [], count: 0, peak: 0 }
      capture = recording
      pendingStream = undefined
      pendingContext = undefined
      processor.onaudioprocess = event => {
        if (!pressed || capture !== recording || generation !== token)
          return
        const frame = monoFrame(event.inputBuffer)
        const remaining = Math.max(0, Math.floor(recording.context.sampleRate * 60) - recording.count)
        const chunk = frame.slice(0, remaining)
        recording.chunks.push(chunk)
        recording.count += chunk.length
        let power = 0
        for (const sample of chunk)
          power += sample * sample
        const rms = Math.sqrt(power / Math.max(1, chunk.length))
        recording.peak = Math.max(recording.peak, rms)
        level.value = Math.min(1, rms * 4)
        if (!remaining || chunk.length < frame.length)
          { cancel(); report('Recording reached the one-minute limit. Try a shorter message.') }
      }
      source.connect(processor)
      processor.connect(mute)
      mute.connect(context.destination)
      limit = setTimeout(() => { cancel(); report('Recording reached the one-minute limit. Try a shorter message.') }, 60000)
    }
    catch (cause) {
      stream?.getTracks().forEach(track => track.stop())
      if (context)
        void context.close().catch(() => { })
      if (token !== generation)
        return
      cancel()
      const denied = cause instanceof DOMException && ['NotAllowedError', 'PermissionDeniedError'].includes(cause.name)
      report(denied ? 'Microphone permission was denied. Allow access in macOS settings and try again.' : 'Could not start the microphone. Check its availability and try again.')
    }
  }
  async function release() {
    if (!pressed)
      return
    pressed = false
    const token = generation
    const recording = capture
    capture = undefined
    disposeCapture(recording)
    if (!recording) {
      generation++
      state.value = 'Idle'
      return
    }
    if (recording.count < recording.context.sampleRate * .15 || recording.peak < .005) {
      state.value = 'Idle'
      report('No speech heard. Hold the talk button while speaking and try again.')
      return
    }
    const combined = new Float32Array(recording.count)
    let offset = 0
    for (const chunk of recording.chunks) {
      combined.set(chunk, offset)
      offset += chunk.length
    }
    recording.chunks.length = 0
    state.value = 'Transcribing'
    try {
      const samples = resample16k(combined, recording.context.sampleRate)
      const text = (await transcriber.transcribe({ samples, sampleRate: 16000, channels: 1 })).trim()
      if (token !== generation || !options.active())
        return
      state.value = 'Idle'
      if (!text) {
        report('No speech heard. Hold the talk button while speaking and try again.')
        return
      }
      await options.onText(text)
    }
    catch {
      if (token !== generation)
        return
      state.value = 'Idle'
      report('Could not transcribe speech. Try again or type your message.')
    }
  }
  function interactive(target: EventTarget | null) {
    return target instanceof Element && !!target.closest('input, textarea, select, button, a, summary, [contenteditable="true"], [role="button"]')
  }
  function keydown(event: KeyboardEvent) {
    if (event.code !== 'Space' || interactive(event.target) || !options.active() || options.busy() || !transcriber.available)
      return
    event.preventDefault()
    if (event.repeat)
      return
    spaceHeld = true
    void start()
  }
  function keyup(event: KeyboardEvent) {
    if (event.code !== 'Space' || !spaceHeld)
      return
    event.preventDefault()
    spaceHeld = false
    void release()
  }
  onMounted(() => {
    window.addEventListener('keydown', keydown)
    window.addEventListener('keyup', keyup)
    window.addEventListener('blur', cancel)
  })
  watch(options.active, active => { if (!active)
    cancel(); })
  onScopeDispose(() => {
    window.removeEventListener('keydown', keydown)
    window.removeEventListener('keyup', keyup)
    window.removeEventListener('blur', cancel)
    cancel()
  })
  return { state, level, error, available: transcriber.available, start, release, cancel }
}
