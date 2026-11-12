import { expect, test, type Plugin } from 'claude-code/testing'
import type { On } from 'claude-code'

import { ASK_TIMEOUT_MS, OTHER_TIMEOUT_MS, resetForTests, VERSION_TIMEOUT_MS } from './jev'

const ok = <T>(value: T) => ({ value }) as never
/** A `jevPath` that does not exist, so the host itself fails to start it. */
const MISSING = 'C:/no/such/folder/jev-missing.exe'
const ANSWER = { q1: { type: 'noul', noul: 0.4 } }
const QUESTIONS = { q1: { type: 'noul', instructions: 'Is a.py a test?' } }

type Run = { argv: string[]; stdin?: string; timeoutMs?: number }
type Result = { exitCode: number; stdout: string }
/** `jev` answers each run from its arguments after the binary. */
type World = {
  jev?: (args: string[], stdin: string | undefined) => Result
  version?: string
  /** A command that the host gets with a binary that does not exist, so the run cannot start. */
  unstartable?: string
}

const line = (value: unknown, exitCode = 0): Result => ({ exitCode, stdout: `${JSON.stringify(value)}\n` })

// The test's own hooks sit beneath the plugin and play the engine and `jev`.
const engine = (on: On, world: World = {}) => {
  const runs: Run[] = []
  const toasts: string[] = []
  const toastTimeouts: unknown[] = []
  on('ui.toast', (_$, e) => {
    toasts.push(String((e as unknown as { text?: unknown }).text ?? e))
    toastTimeouts.push((e as unknown as { timeoutMs?: unknown }).timeoutMs)
    return ok(undefined)
  })
  on('process.run', (_$, e, next) => {
    const { argv, init } = e as { argv: string[]; init?: { stdin?: string; timeoutMs?: number } }
    // The host starts nothing at MISSING, as for a jev that is not installed.
    // A fake cannot throw the host's "failed to start" (a throwing hook is skipped), so MISSING exits 127 with no output.
    if (argv[0] === MISSING) return ok({ exitCode: 127, stdout: '', stderr: '', isStdoutTruncated: false, isStderrTruncated: false })
    runs.push({ argv, stdin: init?.stdin, timeoutMs: init?.timeoutMs })
    const args = argv.slice(1)
    if (args[0] === world.unstartable) return next({ ...(e as object), argv: [MISSING, ...args] } as never)
    const done = (r: Result) => ok({ exitCode: r.exitCode, stdout: r.stdout, stderr: '', isStdoutTruncated: false, isStderrTruncated: false })
    if (args[0] === '--version') return done({ exitCode: 0, stdout: `jev ${world.version ?? '0.1.0'}\n` })
    return done(world.jev?.(args, init?.stdin) ?? line({ ok: true }))
  })
  return { runs, toasts, toastTimeouts }
}

/** The runs after `jev --version`, by their command. */
const calls = (runs: Run[], verb: string) => runs.filter(run => run.argv[1] === verb)

/** A dependent plugin, inline, that reaches $.jev as a plugin built on jev-daemon does. */
const ACME: Plugin = {
  name: 'acme-review',
  register(on) {
    on('command.run', { command: 'acme' }, async ($, e) => {
      // An inline plugin runs in its own scope, so it holds its own copy.
      const QUESTIONS = { q1: { type: 'noul', instructions: 'Is a.py a test?' } }
      const args = (e as unknown as { args?: string }).args ?? ''
      let out: unknown
      if (args === 'status') out = await $.jev.status()
      else if (args === 'as-other') out = await $.jev.ask({ plugin: 'other', state: 's', questions: QUESTIONS as never })
      else if (args === 'exact') {
        const overhead = JSON.stringify({ state: '', questions: QUESTIONS }).length
        out = await $.jev.ask({ state: 'x'.repeat(1048576 - overhead), questions: QUESTIONS as never })
      } else if (args === 'big') out = await $.jev.ask({ state: 'x'.repeat(1024 * 1024), questions: QUESTIONS as never })
      else if (args === 'choice') out = await $.jev.ask({ state: 's', questions: { mood: { type: 'choice', criteria: { angry: null, calm: null } } } })
      else out = await $.jev.ask({ state: { files: ['a.py'] }, questions: QUESTIONS as never })
      return { text: JSON.stringify(out) }
    })
  },
}

/**
 * Test options with the plugin's `jevPath` set. Claude Code 2.1.300 reads
 * `options`; older API types do not list it, and a value that is not an
 * object literal passes their check.
 */
const withPath = (jevPath: string) => {
  const options = { plugins: [ACME], options: { jevPath } }
  return options
}

type Api = { command: { run: (input: unknown) => Promise<unknown> } }
const acme = async ($: unknown, args = ''): Promise<Record<string, unknown>> => {
  const out = (await ($ as Api).command.run({ command: 'acme', args, origin: { kind: 'composer' } })) as { text?: unknown }
  return JSON.parse(String(out.text)) as Record<string, unknown>
}
const codeOf = (result: Record<string, unknown>) => (result.error as { code?: unknown } | undefined)?.code

test('$.jev.ask sends the request on stdin as the client cc-<plugin>', { plugins: [ACME] }, async ($, on) => {
  resetForTests()
  const seen = engine(on, { jev: args => (args[0] === 'ask' ? line({ ok: true, answers: ANSWER }) : line({ ok: true })) })
  expect(await acme($)).toEqual({ ok: true, answers: ANSWER })
  const run = calls(seen.runs, 'ask')[0]
  expect(run?.argv).toEqual(['jev', 'ask', '--client', 'cc-acme-review'])
  expect(JSON.parse(run?.stdin ?? '')).toEqual({ state: { files: ['a.py'] }, questions: QUESTIONS })
  expect(seen.toasts).toEqual([])
})

test('a choice question and its answer pass through', { plugins: [ACME] }, async ($, on) => {
  resetForTests()
  const answers = { mood: { type: 'choice', choice: 'calm', confidence: 0.7, probabilities: { angry: 0.3, calm: 0.7 } } }
  const seen = engine(on, { jev: args => (args[0] === 'ask' ? line({ ok: true, answers }) : line({ ok: true })) })
  expect(await acme($, 'choice')).toEqual({ ok: true, answers })
  const run = calls(seen.runs, 'ask')[0]
  expect(JSON.parse(run?.stdin ?? '')).toEqual({ state: 's', questions: { mood: { type: 'choice', criteria: { angry: null, calm: null } } } })
})

test('the jevPath option names the binary', withPath(' C:/tools/jev.exe '), async ($, on) => {
  resetForTests()
  const seen = engine(on, { jev: () => line({ ok: true, answers: ANSWER }) })
  await acme($)
  expect(seen.runs.map(run => run.argv[0])).toEqual(['C:/tools/jev.exe', 'C:/tools/jev.exe'])
})

test('jev --version runs once per process', { plugins: [ACME] }, async ($, on) => {
  resetForTests()
  const seen = engine(on, { jev: () => line({ ok: true, answers: ANSWER }) })
  await acme($)
  await acme($)
  expect(calls(seen.runs, '--version').length).toBe(1)
  expect(calls(seen.runs, 'ask').length).toBe(2)
})

test('each jev run has its own time limit', { plugins: [ACME] }, async ($, on) => {
  resetForTests()
  const seen = engine(on, { jev: args => (args[0] === 'ask' ? line({ ok: false, error: { code: 'not-paired', message: 'x' } }, 4) : line({ ok: true })) })
  await acme($)
  await acme($, 'status')
  const limit = (verb: string) => calls(seen.runs, verb)[0]?.timeoutMs
  expect(limit('ask')).toBe(ASK_TIMEOUT_MS)
  expect(limit('pair')).toBe(OTHER_TIMEOUT_MS)
  expect(limit('status')).toBe(OTHER_TIMEOUT_MS)
  expect(limit('clients')).toBe(OTHER_TIMEOUT_MS)
  expect(limit('--version')).toBe(VERSION_TIMEOUT_MS)
  expect([ASK_TIMEOUT_MS, OTHER_TIMEOUT_MS, VERSION_TIMEOUT_MS]).toEqual([150000, 30000, 10000])
})

test('a plugin field that names another plugin is bad-request, and runs nothing', { plugins: [ACME] }, async ($, on) => {
  resetForTests()
  const seen = engine(on)
  expect(codeOf(await acme($, 'as-other'))).toBe('bad-request')
  expect(seen.runs).toEqual([])
})

test('a request over 1 MiB is too-large, and runs nothing', { plugins: [ACME] }, async ($, on) => {
  resetForTests()
  const seen = engine(on)
  expect(codeOf(await acme($, 'big'))).toBe('too-large')
  expect(seen.runs).toEqual([])
})

test('not-paired starts a pairing; the toast shows once per code', { plugins: [ACME] }, async ($, on) => {
  resetForTests()
  const seen = engine(on, {
    jev: args =>
      args[0] === 'ask'
        ? line({ ok: false, error: { code: 'not-paired', message: 'no client' } }, 4)
        : line({ ok: false, error: { code: 'pending', message: 'approve it' }, pair_code: 'ABCD2345', name: 'cc-acme-review' }, 3),
  })
  const first = await acme($)
  expect(first).toEqual({ ok: false, error: { code: 'pending', message: 'approve it', pairCode: 'ABCD2345' } })
  await acme($)
  const pairs = calls(seen.runs, 'pair')
  expect(pairs.length).toBe(2)
  expect(pairs[0]?.argv).toEqual(['jev', 'pair', '--name', 'cc-acme-review', '--no-prompt', '--json'])
  expect(pairs[0]?.stdin).toBe(undefined)
  expect(seen.toasts).toEqual(['jev: the plugin acme-review asks to use Jev. In a terminal, run jev allow and approve code ABCD2345.'])
  expect(seen.toastTimeouts).toEqual([60_000])
})

test('a new pairing code gets its own toast', { plugins: [ACME] }, async ($, on) => {
  resetForTests()
  let code = 'ABCD2345'
  const seen = engine(on, {
    jev: args =>
      args[0] === 'ask'
        ? line({ ok: false, error: { code: 'not-paired', message: '' } }, 4)
        : line({ ok: false, error: { code: 'pending', message: '' }, pair_code: code }, 3),
  })
  await acme($)
  code = 'WXYZ6789'
  await acme($)
  expect(seen.toasts.length).toBe(2)
})

test('key problems toast once per profile', { plugins: [ACME] }, async ($, on) => {
  resetForTests()
  const errors = [
    { code: 'no-key', message: 'm', profile: 'default' },
    { code: 'bad-key', message: 'm', profile: 'default' },
    { code: 'no-key', message: 'm', profile: 'work' },
    { code: 'jev-unauthorized', message: 'm', profile: 'work' },
    { code: 'jev-unauthorized', message: 'm', profile: 'work' },
  ]
  let i = 0
  const seen = engine(on, { jev: () => line({ ok: false, error: errors[i++] }, 6) })
  for (const error of errors) expect(codeOf(await acme($))).toBe(error.code)
  expect(seen.toasts.length).toBe(3)
  expect(seen.toasts[0]?.includes('jev key add --profile default')).toBe(true)
  expect(seen.toasts[1]?.includes('jev key add --profile work')).toBe(true)
  expect(seen.toasts[2]?.includes('refused the key for profile work')).toBe(true)
})

test('other failures pass through with no toast', { plugins: [ACME] }, async ($, on) => {
  resetForTests()
  const seen = engine(on, {
    jev: () => line({ ok: false, error: { code: 'rate-limited', message: 'wait', retry_after: 9 } }, 5),
  })
  expect(await acme($)).toEqual({ ok: false, error: { code: 'rate-limited', message: 'wait', retryAfter: 9 } })
  expect(seen.toasts).toEqual([])
})

test('a missing jev is one toast, and every call returns ok: false', withPath(MISSING), async ($, on) => {
  resetForTests()
  const seen = engine(on)
  expect(codeOf(await acme($))).toBe('jev-missing')
  expect(codeOf(await acme($))).toBe('jev-missing')
  const state = await acme($, 'status')
  expect(state).toEqual({ ok: true, installed: false, version: null, compatible: false, running: false, paired: null })
  expect(seen.toasts.length).toBe(1)
  expect(seen.toasts[0]?.includes('need the jev CLI')).toBe(true)
})

test('an old jev is one toast, and every call returns ok: false', { plugins: [ACME] }, async ($, on) => {
  resetForTests()
  const seen = engine(on, { version: '0.0.9' })
  expect(codeOf(await acme($))).toBe('jev-outdated')
  expect(codeOf(await acme($))).toBe('jev-outdated')
  expect(calls(seen.runs, 'ask')).toEqual([])
  expect(seen.toasts.length).toBe(1)
  expect(seen.toasts[0]?.includes('need jev 0.1.0 or later')).toBe(true)
})

test('a jev that cannot finish is jev-failed, with no throw', { plugins: [ACME] }, async ($, on) => {
  resetForTests()
  // --version answers; the ask then reaches the host, which cannot start MISSING.
  engine(on, { unstartable: 'ask' })
  expect(codeOf(await acme($))).toBe('jev-failed')
})

test('a reply with no answers is bad-answer', { plugins: [ACME] }, async ($, on) => {
  resetForTests()
  engine(on, { jev: () => line({ ok: true, answers: [1] }) })
  expect(codeOf(await acme($))).toBe('bad-answer')
})

test('$.jev.status with jevd stopped runs no jev clients', { plugins: [ACME] }, async ($, on) => {
  resetForTests()
  const seen = engine(on, {
    jev: args => (args[0] === 'status' ? line({ ok: false, error: { code: 'daemon-unavailable', message: 'not running' } }, 8) : line({ ok: true })),
  })
  expect(await acme($, 'status')).toEqual({ ok: true, installed: true, version: '0.1.0', compatible: true, running: false, paired: null })
  expect(calls(seen.runs, 'clients')).toEqual([])
})

test('$.jev.status with jevd running reads the paired clients', { plugins: [ACME] }, async ($, on) => {
  resetForTests()
  let names = ['cc-other']
  engine(on, {
    jev: args =>
      args[0] === 'status'
        ? line({ ok: true, daemon_version: '0.1.0' })
        : line({ ok: true, clients: names.map(name => ({ name, profile: 'default' })) }),
  })
  expect((await acme($, 'status')).paired).toBe(false)
  names = ['cc-other', 'cc-acme-review']
  expect(await acme($, 'status')).toEqual({ ok: true, installed: true, version: '0.1.0', compatible: true, running: true, paired: true })
})

test('jev --version runs again after a failed version check', { plugins: [ACME] }, async ($, on) => {
  resetForTests()
  const world: World = { version: '0.0.9', jev: () => line({ ok: true, answers: ANSWER }) }
  const seen = engine(on, world)
  expect(codeOf(await acme($))).toBe('jev-outdated')
  world.version = '0.1.0'
  expect(await acme($)).toEqual({ ok: true, answers: ANSWER })
  expect(calls(seen.runs, '--version').length).toBe(2)
})

test('a relative jevPath is jev-missing, and runs nothing', withPath('jev.exe'), async ($, on) => {
  resetForTests()
  const seen = engine(on)
  const result = await acme($)
  expect(codeOf(result)).toBe('jev-missing')
  expect((result.error as { message: string }).message).toBe('jevPath must be an absolute path')
  expect(seen.runs).toEqual([])
})

test('$.jev.status with an old jev reports it', { plugins: [ACME] }, async ($, on) => {
  resetForTests()
  engine(on, { version: '0.0.9' })
  expect(await acme($, 'status')).toEqual({ ok: true, installed: true, version: '0.0.9', compatible: false, running: false, paired: null })
})

test('$.jev.status with another jev status failure says running, and runs no jev clients', { plugins: [ACME] }, async ($, on) => {
  resetForTests()
  const seen = engine(on, {
    jev: args => (args[0] === 'status' ? line({ ok: false, error: { code: 'unauthorized', message: 'no' } }, 6) : line({ ok: true })),
  })
  expect(await acme($, 'status')).toEqual({ ok: true, installed: true, version: '0.1.0', compatible: true, running: true, paired: null })
  expect(calls(seen.runs, 'clients')).toEqual([])
})

test('$.jev.status has paired: null when jev clients fails', { plugins: [ACME] }, async ($, on) => {
  resetForTests()
  engine(on, {
    jev: args => (args[0] === 'clients' ? line({ ok: false, error: { code: 'daemon-unavailable', message: 'gone' } }, 8) : line({ ok: true })),
  })
  expect(await acme($, 'status')).toEqual({ ok: true, installed: true, version: '0.1.0', compatible: true, running: true, paired: null })
})

test('a pending with no pairCode shows one toast that names jev allow', { plugins: [ACME] }, async ($, on) => {
  resetForTests()
  const seen = engine(on, {
    jev: args =>
      args[0] === 'ask'
        ? line({ ok: false, error: { code: 'not-paired', message: '' } }, 4)
        : line({ ok: false, error: { code: 'pending', message: 'wait' } }, 3),
  })
  expect(codeOf(await acme($))).toBe('pending')
  await acme($)
  expect(seen.toasts.length).toBe(1)
  expect(seen.toasts[0]?.includes('jev allow')).toBe(true)
})

test('a version check that does not finish is jev-failed, with no toast', { plugins: [ACME] }, async ($, on) => {
  resetForTests()
  const seen = engine(on, { unstartable: '--version' })
  expect(codeOf(await acme($))).toBe('jev-failed')
  expect(await acme($, 'status')).toEqual({ ok: true, installed: true, version: null, compatible: false, running: false, paired: null })
  expect(seen.toasts).toEqual([])
})

test('a request of exactly 1 MiB is not refused', { plugins: [ACME] }, async ($, on) => {
  resetForTests()
  const seen = engine(on, { jev: () => line({ ok: true, answers: ANSWER }) })
  expect(await acme($, 'exact')).toEqual({ ok: true, answers: ANSWER })
  // The body is all ASCII, so its length is its byte count.
  expect((calls(seen.runs, 'ask')[0]?.stdin ?? '').length).toBe(1048576)
})
