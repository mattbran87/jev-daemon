// The `$.jev` calls: find `jev`, run it, and show each toast once.

import type { EngineInterface } from 'claude-code'

import type { JevAskResult, JevFailure, JevStatus } from '../types'
import {
  MAX_REQUEST_BYTES,
  MIN_JEV,
  type Parsed,
  callerName,
  clean,
  clientName,
  failure,
  keyToast,
  missingToast,
  olderThan,
  outdatedToast,
  parseVersion,
  pendingNoCodeToast,
  pendingToast,
  readReply,
  requestBytes,
  unauthorizedToast,
  versionText,
} from './text'

/** `jev --version` answers at once. */
export const VERSION_TIMEOUT_MS = 10_000
/**
 * One `jev ask`: jev's own 120 s reply timeout, plus its 10 s wait for a
 * jevd that it starts, plus room.
 */
export const ASK_TIMEOUT_MS = 150_000
/** `jev pair`, `jev status`, and `jev clients`. */
export const OTHER_TIMEOUT_MS = 30_000
/** The host's toast has only a timeout, so it cannot stay until someone closes it. */
const TOAST_MS = 60_000

type Found = { ok: true; path: string; version: string }
type NotFound = { ok: false; code: 'jev-missing' | 'jev-outdated' | 'jev-failed'; message: string; version: string | null }

/** The `jev` that answered `--version`, kept for the rest of the process. A failure is not kept, so a `jev` installed later counts. */
let found: Found | null = null
/** The toasts shown in this process, by key. */
const shown = new Set<string>()

export function resetForTests(): void {
  found = null
  shown.clear()
}

/** The toast for a `jev` that is missing or too old. */
function notFoundToast(jev: NotFound): string {
  return jev.code === 'jev-missing' ? missingToast() : outdatedToast(jev.version ?? '?', MIN_JEV)
}

export function toastOnce($: EngineInterface, key: string, text: string): void {
  if (shown.has(key)) return
  shown.add(key)
  $.ui.toast(text, { timeoutMs: TOAST_MS })
}

/** Runs `jev --version` once, to find `jev` and check that it is new enough. */
export async function findJev($: EngineInterface, configured: string): Promise<Found | NotFound> {
  // The bare `jev` is looked up by the host. The host refuses a binary that it
  // finds only in the session's folder (a probe on 2026-11-03 got "not found or
  // is in an unsafe location (current directory)"), so a `jev` planted in a
  // repo does not run.
  const path = configured === '' ? 'jev' : configured
  if (found !== null && found.path === path) return found
  if (configured !== '' && !/^([A-Za-z]:)?[\\/]/.test(configured)) {
    return { ok: false, code: 'jev-missing', message: 'jevPath must be an absolute path', version: null }
  }
  let stdout: string
  let exitCode: number
  try {
    const r = await $.process.run([path, '--version'], { timeoutMs: VERSION_TIMEOUT_MS })
    stdout = r.stdout
    exitCode = r.exitCode
  } catch (err) {
    if (String(err).includes('failed to start')) {
      return { ok: false, code: 'jev-missing', message: `cannot run ${path}`, version: null }
    }
    return { ok: false, code: 'jev-failed', message: `${path} --version did not finish: ${clean(String(err)).slice(0, 200)}`, version: null }
  }
  const version = exitCode === 0 ? parseVersion(stdout) : null
  if (version === null) {
    return { ok: false, code: 'jev-missing', message: `${path} --version printed no jev version`, version: null }
  }
  if (olderThan(version, MIN_JEV)) {
    const text = versionText(version)
    return { ok: false, code: 'jev-outdated', message: `jev ${text} is older than ${versionText(MIN_JEV)}`, version: text }
  }
  found = { ok: true, path, version: versionText(version) }
  return found
}

/** `jev` found and new enough, or a failure after one toast per process. */
export async function ready($: EngineInterface, configured: string): Promise<Found | JevFailure> {
  const jev = await findJev($, configured)
  if (jev.ok) return jev
  if (jev.code !== 'jev-failed') {
    toastOnce($, jev.code, notFoundToast(jev))
  }
  return failure(jev.code, jev.message)
}

/** One `jev` run. It never rejects: a run that cannot start or finish is `jev-failed`. */
export async function runJev(
  $: EngineInterface,
  path: string,
  args: readonly string[],
  stdin: string | undefined,
  timeoutMs: number,
): Promise<Parsed> {
  try {
    const r = await $.process.run([path, ...args], { ...(stdin === undefined ? {} : { stdin }), timeoutMs })
    return readReply(r.stdout, r.exitCode)
  } catch (err) {
    return failure('jev-failed', `jev did not finish: ${clean(String(err)).slice(0, 200)}`)
  }
}

/** One toast for a key problem, per profile, per process. */
export function notice($: EngineInterface, error: JevFailure['error']): void {
  if (error.code === 'no-key' || error.code === 'bad-key') {
    toastOnce($, `key:${error.profile ?? 'default'}`, keyToast(error.profile))
  } else if (error.code === 'jev-unauthorized') {
    toastOnce($, `unauthorized:${error.profile ?? 'default'}`, unauthorizedToast(error.profile))
  }
}

/**
 * `jev pair --no-prompt`: jevd lists the request, and `jev` prints its code.
 * A repeat while the request waits gets the same code, so one toast per code.
 */
export async function pair($: EngineInterface, path: string, plugin: string): Promise<JevFailure> {
  const reply = await runJev($, path, ['pair', '--name', clientName(plugin), '--no-prompt', '--json'], undefined, OTHER_TIMEOUT_MS)
  if (reply.ok) return failure('not-paired', `jev pair approved ${clientName(plugin)} with no person; ask again`)
  const code = reply.error.pairCode
  if (reply.error.code === 'pending') {
    if (code !== undefined) toastOnce($, `pending:${code}`, pendingToast(plugin, code))
    else toastOnce($, 'pending:', pendingNoCodeToast(plugin))
  }
  return reply
}

/** `$.jev.ask`, spec section 7, "Flow of one ask". */
export async function ask($: EngineInterface, input: unknown, origin: string, configured: string): Promise<JevAskResult> {
  const who = callerName(origin, input)
  if (!who.ok) return who
  const given = (input ?? {}) as { state?: unknown; questions?: unknown }
  let body: string
  try {
    body = JSON.stringify({ state: given.state, questions: given.questions })
  } catch {
    return failure('bad-request', 'state and questions must be JSON values')
  }
  if (requestBytes(body) > MAX_REQUEST_BYTES) {
    return failure('too-large', `the request is over ${MAX_REQUEST_BYTES} bytes`)
  }
  const jev = await ready($, configured)
  if (!jev.ok) return jev
  const reply = await runJev($, jev.path, ['ask', '--client', clientName(who.plugin)], body, ASK_TIMEOUT_MS)
  if (reply.ok) {
    const answers = reply.data.answers
    return answers !== null && typeof answers === 'object' && !Array.isArray(answers)
      ? { ok: true, answers: answers as Extract<JevAskResult, { ok: true }>['answers'] }
      : failure('bad-answer', 'jev printed no answers')
  }
  if (reply.error.code === 'not-paired') return await pair($, jev.path, who.plugin)
  notice($, reply.error)
  return reply
}

/** `$.jev.status`. It never starts jevd: `jev status` does not, and `jev clients` runs only when jevd runs. */
export async function status($: EngineInterface, input: unknown, origin: string, configured: string): Promise<JevStatus> {
  const who = callerName(origin, input)
  if (!who.ok) return who
  const jev = await findJev($, configured)
  if (!jev.ok) {
    if (jev.code === 'jev-failed') {
      return { ok: true, installed: true, version: null, compatible: false, running: false, paired: null }
    }
    toastOnce($, jev.code, notFoundToast(jev))
    return { ok: true, installed: jev.code === 'jev-outdated', version: jev.version, compatible: false, running: false, paired: null }
  }
  const daemon = await runJev($, jev.path, ['status', '--json'], undefined, OTHER_TIMEOUT_MS)
  if (!daemon.ok) {
    if (daemon.error.code === 'daemon-unavailable') {
      return { ok: true, installed: true, version: jev.version, compatible: true, running: false, paired: null }
    }
    return { ok: true, installed: true, version: jev.version, compatible: true, running: true, paired: null }
  }
  const list = await runJev($, jev.path, ['clients', '--json'], undefined, OTHER_TIMEOUT_MS)
  const clients = list.ok ? list.data.clients : undefined
  const paired = Array.isArray(clients)
    ? clients.some(c => c !== null && typeof c === 'object' && (c as { name?: unknown }).name === clientName(who.plugin))
    : null
  return { ok: true, installed: true, version: jev.version, compatible: true, running: true, paired }
}
