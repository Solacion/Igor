import { test } from 'node:test'
import assert from 'node:assert/strict'
import { EventEmitter } from 'node:events'
import { mkdtemp, readFile, writeFile, mkdir, rm, access } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { absoluteConfig, buildFixture, prepareDemo, launchDemo, repository } from './demo.mjs'

const sample = await readFile(join(repository, 'examples/igor.toml'), 'utf8')
test('copies the real two-server example with absolute paths, preserving providers/permissions and source', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'igor-launcher-test-'))
  try {
    const root = join(dir, 'repo with spaces and "quotes"')
    await mkdir(join(root, 'examples'), { recursive: true })
    await mkdir(join(root, 'crates/igor-fixture-server/fixtures'), { recursive: true })
    const executable = join(root, 'fixture executable')
    await writeFile(executable, '')
    await writeFile(join(root, 'examples/igor.toml'), sample)
    for (const name of ['sales', 'supplier']) await writeFile(join(root, `crates/igor-fixture-server/fixtures/${name}.json`), '{}')
    await assert.rejects(prepareDemo(root, executable, root), /outside the repository/)
    const demo = await prepareDemo(root, executable, dir)
    assert.ok(!demo.config.startsWith(root))
    const copied = await readFile(demo.config, 'utf8')
    const programs = copied.match(/^command = .+$/gm)
    assert.equal(programs.length, 2)
    assert.ok(programs.every(line => JSON.parse(line.slice('command = '.length)) === executable))
    for (const line of copied.match(/^args = .+$/gm)) assert.ok(JSON.parse(line.slice('args = '.length))[1].startsWith(root))
    assert.ok(copied.includes('[permissions.supplier]'))
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
