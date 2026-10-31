import { constants } from 'node:fs'
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
  const rewritten = absoluteConfig(source, root, executable)
  // Start the walkthrough with providers only; Settings performs every server add and grant.
  const firstServer = rewritten.indexOf('[[servers]]')
  const limits = rewritten.indexOf('[limits]', firstServer)
  const content = rewritten.slice(0, firstServer) + (limits < 0 ? '' : rewritten.slice(limits))
  const servers = ['notes', 'sales', 'supplier'].map(id => ({
    id, command: executable,
    args: ['--fixture', join(root, id === 'notes' ? 'apps/desktop/fixtures/notes.json' : `crates/igor-fixture-server/fixtures/${id}.json`)],
  }))
  await access(executable)
  for (const server of servers) await access(server.args[1])
  const location = relative(await realpath(root), await realpath(temporaryRoot))
  if (location === '' || (location !== '..' && !location.startsWith(`..${sep}`) && !isAbsolute(location))) {
    throw new Error('The demo temporary directory must be outside the repository.')
  }
  const directory = await mkdtemp(join(temporaryRoot, 'igor-desktop-demo-'))
  const config = join(directory, 'igor.toml')
  const setup = join(directory, 'servers.json')
  try {
    await writeFile(config, content, { mode: 0o600, flag: 'wx' })
    await writeFile(setup, JSON.stringify(servers, null, 2) + '\n', { mode: 0o600, flag: 'wx' })
  }
  catch (error) { await rm(directory, { recursive: true, force: true }); throw error }
  return { config, directory, servers, setup, cleanup: () => rm(directory, { recursive: true, force: true }) }
}

/** Cargo metadata honors CARGO_TARGET_DIR without assuming a checkout-local build cache. */
export function releaseExecutable(root = repository, env = process.env, run = spawnSync) {
  const result = run('cargo', ['metadata', '--no-deps', '--format-version', '1', '--manifest-path', join(root, 'apps/desktop/src-tauri/Cargo.toml')], {
    cwd: join(root, 'apps/desktop/src-tauri'), env, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'],
  })
  if (result.error || result.status !== 0) throw new Error('Could not locate the release app. Run pnpm run desktop:release first.')
  let target
  try { target = JSON.parse(result.stdout).target_directory } catch { /* Fixed diagnostic below. */ }
  if (typeof target !== 'string' || !isAbsolute(target)) throw new Error('Cargo did not report an absolute desktop build directory.')
  return join(target, 'release/bundle/macos/Igor.app/Contents/MacOS/igor-desktop')
}

export async function launchDemo({ root = repository, demo, run = spawn, env = process.env, release = false, locate = releaseExecutable, check = access }) {
  let child
  const interrupt = () => { child?.kill('SIGINT') }
  const terminate = () => { child?.kill('SIGTERM') }
  try {
    let program = 'pnpm'
    let args = ['--filter', '@igor/desktop', 'tauri', 'dev']
    if (release) {
      program = locate(root, env)
      try { await check(program, constants.X_OK) }
      catch { throw new Error('Release app not found or not executable. Run pnpm run desktop:release first.') }
      args = []
    }
    // Launch the bundle executable directly so it inherits IGOR_CONFIG and is awaited until quit.
    child = run(program, args, { cwd: root, env: { ...env, IGOR_CONFIG: demo.config }, stdio: 'inherit' })
    process.on('SIGINT', interrupt)
    process.on('SIGTERM', terminate)
    return await new Promise((done, reject) => {
      child.once('error', () => reject(new Error(release ? 'Could not start the release app. Run pnpm run desktop:release and check macOS permissions.' : 'Could not start pnpm. Install the documented desktop prerequisites.')))
      child.once('exit', (code, signal) => done(code ?? (signal === 'SIGINT' ? 130 : 1)))
    })
  } finally {
    process.off('SIGINT', interrupt); process.off('SIGTERM', terminate)
    await demo.cleanup()
  }
}

async function main() {
  if (process.argv.slice(2).some(arg => !['--prepare-only', '--release', '--'].includes(arg))) throw new Error('Usage: desktop:demo [--release] [--prepare-only]')
  const demo = await prepareDemo()
  console.log(`Disposable demo config: ${demo.config}`)
  console.log(`Server setup reference: ${demo.setup}`)
  console.log('In Settings, add these three servers. Each argument is a separate line; do not add shell quotes.')
  for (const server of demo.servers) {
    console.log(`Server id: ${server.id}\nProgram: ${server.command}\nArguments (one per line):\n${server.args.join('\n')}`)
  }
  console.log('Choose lm_studio (qwen/qwen3-vl-8b loaded in LM Studio), or claude with ANTHROPIC_API_KEY set before launch.')
  if (process.argv.includes('--prepare-only')) {
    console.log('Preparation only: no app or provider was started. Remove this temporary directory when finished.')
    return
  }
  console.log('Close Igor’s window or choose Quit for awaited server cleanup. The temporary config is removed after the launcher exits.')
  process.exitCode = await launchDemo({ demo, release: process.argv.includes('--release') })
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  main().catch(error => { console.error(error instanceof Error ? error.message : 'Desktop demo failed. Check the documented prerequisites.'); process.exitCode = 1 })
}
