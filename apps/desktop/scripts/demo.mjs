import { spawn, spawnSync } from 'node:child_process'
import { mkdtemp, readFile, writeFile, rm, access, realpath } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, isAbsolute, join, resolve, relative, sep } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

export const repository = resolve(dirname(fileURLToPath(import.meta.url)), '../../..')

/** Rewrite only the known fixture program/arguments. Keep provider and permission settings
 * verbatim; fail rather than silently running a changed example program from a temp folder. */
export function absoluteConfig(source, root, executable) {
  if (!isAbsolute(executable)) throw new Error('Cargo did not report an absolute fixture executable path.')
  let programs = 0
  let fixtures = 0
  const text = source.split('\n').map(line => {
    const command = line.match(/^command\s*=\s*(".*")\s*$/)
    if (command) {
      if (JSON.parse(command[1]) !== '../target/debug/igor-fixture-server') throw new Error('The example server program changed; update the desktop demo launcher.')
      programs++
      return `command = ${JSON.stringify(executable)}`
    }
    const args = line.match(/^args\s*=\s*(\[.*\])\s*$/)
    if (args) {
      const values = JSON.parse(args[1])
      if (!Array.isArray(values) || values.length !== 2 || values[0] !== '--fixture' || typeof values[1] !== 'string') throw new Error('The example fixture arguments changed; update the desktop demo launcher.')
      fixtures++
      return `args = ${JSON.stringify(['--fixture', resolve(root, 'examples', values[1])])}`
    }
    return line
  }).join('\n')
  if (programs !== 2 || fixtures !== 2) throw new Error('Expected the sales and supplier fixture servers in examples/igor.toml.')
  return text
}

export function buildFixture(root, run = spawnSync) {
  const result = run('cargo', ['build', '--locked', '-p', 'igor-fixture-server', '--message-format=json'], {
    cwd: root, encoding: 'utf8', stdio: ['inherit', 'pipe', 'inherit'], maxBuffer: 16 * 1024 * 1024,
  })
  if (result.error) throw new Error('Could not start Cargo. Install the pinned Rust toolchain and ensure cargo is on PATH.')
  if (result.status !== 0) throw new Error('The fixture server build failed. See Cargo output above.')
  const artifacts = String(result.stdout).split('\n').flatMap(line => {
    try { return [JSON.parse(line)] } catch { return [] }
  })
  const executable = artifacts.find(item => item.reason === 'compiler-artifact' && item.target?.name === 'igor-fixture-server' && typeof item.executable === 'string')?.executable
  if (!executable || !isAbsolute(executable)) throw new Error('Cargo did not report the fixture server executable.')
  return executable
}

export async function prepareDemo(root = repository, executable = buildFixture(root), temporaryRoot = tmpdir()) {
  const source = await readFile(join(root, 'examples/igor.toml'), 'utf8')
  const content = absoluteConfig(source, root, executable)
  await access(executable)
  for (const fixture of ['sales', 'supplier']) await access(join(root, `crates/igor-fixture-server/fixtures/${fixture}.json`))
  const location = relative(await realpath(root), await realpath(temporaryRoot))
  if (location === '' || (location !== '..' && !location.startsWith(`..${sep}`) && !isAbsolute(location))) {
    throw new Error('The demo temporary directory must be outside the repository.')
  }
  const directory = await mkdtemp(join(temporaryRoot, 'igor-desktop-demo-'))
  const config = join(directory, 'igor.toml')
  try { await writeFile(config, content, { mode: 0o600, flag: 'wx' }) }
  catch (error) { await rm(directory, { recursive: true, force: true }); throw error }
  return { config, directory, cleanup: () => rm(directory, { recursive: true, force: true }) }
}

export async function launchDemo({ root = repository, demo, run = spawn, env = process.env }) {
  const child = run('pnpm', ['--filter', '@igor/desktop', 'tauri', 'dev'], {
    cwd: root, env: { ...env, IGOR_CONFIG: demo.config }, stdio: 'inherit',
  })
  const interrupt = () => { child.kill('SIGINT') }
  const terminate = () => { child.kill('SIGTERM') }
  process.on('SIGINT', interrupt)
  process.on('SIGTERM', terminate)
  try {
    return await new Promise((done, reject) => {
      child.once('error', () => reject(new Error('Could not start pnpm. Install the documented desktop prerequisites.')))
      child.once('exit', (code, signal) => done(code ?? (signal === 'SIGINT' ? 130 : 1)))
    })
  } finally {
    process.off('SIGINT', interrupt); process.off('SIGTERM', terminate)
    await demo.cleanup()
  }
}

async function main() {
  if (process.argv.slice(2).some(arg => arg !== '--prepare-only')) throw new Error('Usage: desktop:demo [--prepare-only]')
  const demo = await prepareDemo()
  console.log(`Disposable demo config: ${demo.config}`)
  console.log('Choose lm_studio (qwen/qwen3-vl-8b loaded in LM Studio), or claude with ANTHROPIC_API_KEY set before launch.')
  if (process.argv.includes('--prepare-only')) {
    console.log('Preparation only: no app or provider was started. Remove this temporary directory when finished.')
    return
  }
  console.log('Close Igor’s window or choose Quit for awaited server cleanup. The temporary config is removed after the launcher exits.')
  process.exitCode = await launchDemo({ demo })
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  main().catch(() => { console.error('Desktop demo failed. Check the preceding build output and the documented prerequisites.'); process.exitCode = 1 })
}
