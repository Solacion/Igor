/** Downsample mono samples with a weighted box filter; output is finite, bounded f32 PCM. */
export function resample16k(input: Float32Array, sampleRate: number): Float32Array {
  if (!Number.isFinite(sampleRate) || sampleRate <= 0)
    throw new Error('Invalid sample rate')
  const ratio = sampleRate / 16000
  const output = new Float32Array(Math.floor(input.length / ratio))
  for (let index = 0; index < output.length; index++) {
    const start = index * ratio
    const end = Math.min(input.length, (index + 1) * ratio)
    let sum = 0
    for (let offset = Math.floor(start); offset < Math.ceil(end); offset++) {
      const weight = Math.min(end, offset + 1) - Math.max(start, offset)
      const sample = input[offset] ?? 0
      sum += (Number.isFinite(sample) ? Math.max(-1, Math.min(1, sample)) : 0) * weight
    }
    output[index] = sum / (end - start)
  }
  return output
}
export function monoFrame(buffer: AudioBuffer): Float32Array {
  const mono = new Float32Array(buffer.length)
  for (let channel = 0; channel < buffer.numberOfChannels; channel++) {
    const data = buffer.getChannelData(channel)
    for (let index = 0; index < mono.length; index++) {
      const sample = data[index] ?? 0
      mono[index] = (mono[index] ?? 0) + (Number.isFinite(sample) ? sample / buffer.numberOfChannels : 0)
    }
  }
  return mono
}
