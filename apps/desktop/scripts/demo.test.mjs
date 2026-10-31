import { test } from 'node:test'
import assert from 'node:assert/strict'
import { EventEmitter } from 'node:events'
import { mkdtemp, readFile, writeFile, mkdir, rm, access } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { absoluteConfig, buildFixture, prepareDemo, launchDemo, releaseExecutable, repository } from './demo.mjs'

const sample = await readFile(join(repository, 'examples/igor.toml'), 'utf8')
test('prepares provider-only setup plus three absolute server entries without modifying the source', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'igor-launcher-test-'))
  try {
    const root = join(dir, 'repo with spaces and "quotes"')
    await mkdir(join(root, 'examples'), { recursive: true })
    await mkdir(join(root, 'crates/igor-fixture-server/fixtures'), { recursive: true })
    await mkdir(join(root, 'apps/desktop/fixtures'), { recursive: true })
    await writeFile(join(root, 'apps/desktop/fixtures/notes.json'), await readFile(join(repository, 'apps/desktop/fixtures/notes.json')))
    const executable = join(root, 'fixture executable')
    await writeFile(executable, '')
    await writeFile(join(root, 'examples/igor.toml'), sample)
    for (const name of ['sales', 'supplier']) await writeFile(join(root, `crates/igor-fixture-server/fixtures/${name}.json`), '{}')
    await assert.rejects(prepareDemo(root, executable, root), /outside the repository/)
    const demo = await prepareDemo(root, executable, dir)
    assert.ok(!demo.config.startsWith(root))
    const copied = await readFile(demo.config, 'utf8')
    assert.ok(!copied.includes('[[servers]]'))
    assert.ok(!copied.includes('[permissions.'))
    const entries = JSON.parse(await readFile(demo.setup, 'utf8'))
    assert.deepEqual(entries.map(entry => entry.id), ['notes', 'sales', 'supplier'])
    assert.ok(entries.every(entry => entry.command === executable && entry.args.length === 2 && entry.args[0] === '--fixture' && entry.args[1].startsWith(root)))
    assert.equal(entries[0].args[1], join(root, 'apps/desktop/fixtures/notes.json'))
    assert.ok(copied.includes('model = "qwen/qwen3-vl-8b"'))
    assert.ok(copied.includes('api_key = { env = "ANTHROPIC_API_KEY" }'))
    // Simulate Settings editing the copy; the repository source stays byte-for-byte unchanged.
    await writeFile(demo.config, 'edited')
    assert.equal(await readFile(join(root, 'examples/igor.toml'), 'utf8'), sample)
    await demo.cleanup(); await assert.rejects(access(demo.directory))
  } finally { await rm(dir, { recursive: true, force: true }) }
})
test('honors Cargo artifact path and fails without a successful build', () => {
  const executable = '/custom target/debug/igor-fixture-server'
  assert.equal(buildFixture('/repo', (program, args, options) => {
    assert.equal(program, 'cargo'); assert.deepEqual(args, ['build', '--locked', '-p', 'igor-fixture-server', '--message-format=json']); assert.equal(options.cwd, '/repo')
    return { status: 0, stdout: JSON.stringify({ reason: 'compiler-artifact', target: { name: 'igor-fixture-server' }, executable }) }
  }), executable)
  assert.throws(() => buildFixture('/repo', () => ({ status: 1 })), /build failed/)
  assert.throws(() => buildFixture('/repo', () => ({ status: 0, stdout: '' })), /did not report/)
  assert.throws(() => absoluteConfig(sample.replaceAll('../target/debug/igor-fixture-server', '/changed-program'), '/repo', executable), /program changed/)
})
test('launches with the disposable config, forwards exit status, and cleans up on launch failure', async () => {
  for (const fail of [false, true]) {
    let cleaned = false
    const child = new EventEmitter(); child.kill = () => {}
    const task = launchDemo({ root: '/repo', demo: { config: '/temp/demo.toml', cleanup: async () => { cleaned = true } }, env: { PATH: '/bin', IGOR_CONFIG: '/operator/config.toml' }, run(program, args, options) {
      assert.equal(program, 'pnpm'); assert.deepEqual(args, ['--filter', '@igor/desktop', 'tauri', 'dev']); assert.equal(options.cwd, '/repo')
      assert.equal(options.env.IGOR_CONFIG, '/temp/demo.toml'); assert.equal(options.env.PATH, '/bin')
      return child
    } })
    if (fail) { child.emit('error', new Error('spawn failed')); await assert.rejects(task, /Could not start pnpm/) }
    else { child.emit('exit', 7, null); assert.equal(await task, 7) }
    assert.ok(cleaned)
  }
})

test('notes supplies the documented fixture dates and restock policy without writes', async () => {
  const fixture = JSON.parse(await readFile(join(repository, 'apps/desktop/fixtures/notes.json'), 'utf8'))
  assert.deepEqual(fixture.tools.map(tool => tool.name), ['list_notes'])
  assert.equal(fixture.tools[0].annotations.readOnlyHint, true)
  const call = fixture.calls[0]
  assert.deepEqual(call.arguments, {})
  assert.deepEqual(JSON.parse(call.result.content[0].text), call.result.structuredContent)
  const text = JSON.stringify(call.result.structuredContent)
  for (const fact of ['supplier-x', '2026-07-13', '2026-07-06', '2026-07-12', '2026-07-15', 'previous seven days', 'rounding up', 'Draft only']) assert.ok(text.includes(fact))
})

test('locates the release bundle using Cargo target metadata, including paths with spaces', () => {
  const env = { CARGO_TARGET_DIR: '/build cache' }
  const result = releaseExecutable('/repo', env, (program, args, options) => {
    assert.equal(program, 'cargo'); assert.ok(args.includes('/repo/apps/desktop/src-tauri/Cargo.toml'))
    assert.equal(options.env, env)
    return { status: 0, stdout: JSON.stringify({ target_directory: '/build cache' }) }
  })
  assert.equal(result, '/build cache/release/bundle/macos/Igor.app/Contents/MacOS/igor-desktop')
})

test('release demo starts the bundled executable with disposable config, never a dev server', async () => {
  let cleaned = false
  let child
  const started = new Promise(resolve => { child = new EventEmitter(); child.kill = () => {}; resolve() })
  const task = launchDemo({ release: true, root: '/repo', demo: { config: '/temp/release.toml', cleanup: async () => { cleaned = true } },
    locate: () => '/release bundle/Igor.app/Contents/MacOS/igor-desktop', check: async () => {}, env: { PATH: '/bin' },
    run(program, args, options) {
      assert.equal(program, '/release bundle/Igor.app/Contents/MacOS/igor-desktop')
      assert.deepEqual(args, []); assert.equal(options.env.IGOR_CONFIG, '/temp/release.toml')
      assert.equal(options.env.PATH, '/bin'); return child
    },
  })
  await started; await new Promise(resolve => setImmediate(resolve))
  assert.equal(cleaned, false)
  child.emit('exit', 0, null); assert.equal(await task, 0); assert.equal(cleaned, true)
})

test('missing release bundle gives build guidance and cleans the disposable config', async () => {
  let cleaned = false
  await assert.rejects(launchDemo({ release: true, demo: { config: '/temp/demo.toml', cleanup: async () => { cleaned = true } },
    locate: () => '/missing/Igor.app', check: async () => { throw new Error('missing') },
    run() { assert.fail('must not launch pnpm or fall back to development') },
  }), /Run pnpm run desktop:release first/)
  assert.equal(cleaned, true)
})
