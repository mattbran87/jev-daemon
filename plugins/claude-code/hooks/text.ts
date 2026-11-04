// Helpers that need no `$`: names, versions, sizes, `jev` replies, and the
// text of each toast.

import type { JevFailure } from '../types'

/** A plugin name that the mod turns into the client name `cc-<plugin>` (spec section 7). */
const PLUGIN_NAME = /^[a-z0-9][a-z0-9-]{0,60}$/
/** Origins that name the host, not a plugin. */
const RESERVED = new Set(['engine', 'client'])
/** The oldest `jev` that has `jev pair --no-prompt`. */
export const MIN_JEV: Version = [0, 1, 0]
/** `jev`'s own cap on a request line (`jev_proto::MAX_LINE`). */
export const MAX_REQUEST_BYTES = 1024 * 1024
/** jevd's pairing codes: 8 characters, no look-alikes. */
const PAIR_CODE = /^[23456789ABCDEFGHJKMNPQRSTUVWXYZ]{8}$/
/** jevd's profile and client name rule (`jev_proto::valid_name`). */
const PROFILE_NAME = /^[a-z0-9][a-z0-9-]{0,63}$/
/** The longest message the mod passes on from `jev`. */
const MAX_MESSAGE = 300

export type Version = readonly [number, number, number]

/** What one `jev` run gave: its JSON result, or one failure. */
export type Parsed = { ok: true; data: Record<string, unknown> } | JevFailure

export function failure(code: string, message: string): JevFailure {
  return { ok: false, error: { code, message } }
}

export function clientName(plugin: string): string {
  return `cc-${plugin}`
}

function isObject(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

/**
 * The calling plugin, as the host names it. An input `plugin` field must name
 * the same plugin, so one mod cannot ask as another.
 */
export function callerName(origin: string, input: unknown): { ok: true; plugin: string } | JevFailure {
  if (!PLUGIN_NAME.test(origin) || RESERVED.has(origin)) {
    return failure('bad-request', `"${origin.slice(0, 64)}" is not a plugin name that jev accepts`)
  }
  const given = isObject(input) ? input.plugin : undefined
  if (given !== undefined && given !== origin) {
    return failure('bad-request', `the plugin field must name the calling plugin, ${origin}`)
  }
  return { ok: true, plugin: origin }
}

/** The version in `jev --version` output, such as `jev 0.1.0`. */
export function parseVersion(stdout: string): Version | null {
  const match = /^jev (\d+)\.(\d+)\.(\d+)(?:[-+]\S*)?(?:\s|$)/.exec(stdout.trim())
  return match === null ? null : [Number(match[1]), Number(match[2]), Number(match[3])]
}

export function olderThan(found: Version, need: Version): boolean {
  for (let i = 0; i < 3; i++) {
    if (found[i] !== need[i]) return (found[i] ?? 0) < (need[i] ?? 0)
  }
  return false
}

export function versionText(version: Version): string {
  return version.join('.')
}

export function requestBytes(text: string): number {
  return new TextEncoder().encode(text).length
}

/** Text from `jev` made safe to show: hidden characters become spaces, and the cut counts code points. */
export function clean(text: string): string {
  const cleaned = text.replace(/[\u0000-\u001F\u007F-\u009F\u061C\u200B-\u200F\u2028-\u202E\u2060-\u2064\u2066-\u206F\uFEFF\uFFF9-\uFFFB\u{E0000}-\u{E007F}]/gu, ' ')
  return Array.from(cleaned).slice(0, MAX_MESSAGE).join('')
}

/** A code from `jev` must look like a code name. */
const ERROR_CODE = /^[a-z0-9][a-z0-9-]{0,63}$/

/**
 * One `jev --json` result from its stdout. The JSON's code wins over the exit
 * code, so a code from a newer `jev` reaches the caller as it is. Only the
 * fields the mod knows pass on, each checked.
 */
export function readReply(stdout: string, exitCode: number): Parsed {
  let value: unknown
  try {
    value = JSON.parse(stdout.trim())
  } catch {
    value = undefined
  }
  if (isObject(value) && value.ok === true) return { ok: true, data: value }
  if (isObject(value) && value.ok === false && isObject(value.error) && typeof value.error.code === 'string' && ERROR_CODE.test(value.error.code)) {
    const error = value.error
    const out: JevFailure = {
      ok: false,
      error: {
        code: error.code as string,
        message: typeof error.message === 'string' ? clean(error.message) : '',
      },
    }
    if (typeof error.retry_after === 'number' && Number.isInteger(error.retry_after) && error.retry_after >= 0) out.error.retryAfter = error.retry_after
    if (typeof error.profile === 'string' && PROFILE_NAME.test(error.profile)) out.error.profile = error.profile
    if (typeof value.pair_code === 'string' && PAIR_CODE.test(value.pair_code)) out.error.pairCode = value.pair_code
    return out
  }
  return failure('jev-failed', `jev printed no result (exit ${exitCode})`)
}

// Only a `pairCode` that `readReply` checked may go in.
export function pendingToast(plugin: string, code: string): string {
  return `jev: the plugin ${plugin} asks to use Jev. Run jev allow ${code} in a terminal to approve it.`
}

export function pendingNoCodeToast(plugin: string): string {
  return `jev: the plugin ${plugin} asks to use Jev. Run jev allow in a terminal to see and approve the request.`
}

export function keyToast(profile: string | undefined): string {
  const name = profile ?? 'default'
  return `jev: profile ${name} has no usable key. Run jev key add --profile ${name}.`
}

export function unauthorizedToast(profile: string | undefined): string {
  const name = profile ?? 'default'
  return `jev: Jev refused the key for profile ${name}. Run jev key add --profile ${name}.`
}

export function missingToast(): string {
  return 'jev: plugins that use Jev need the jev CLI. Install jev and jevd, or set the jevPath option of the jev-daemon plugin.'
}

export function outdatedToast(found: string, need: Version): string {
  return `jev: plugins that use Jev need jev ${versionText(need)} or later, and this one is ${found}. Update jev.`
}
