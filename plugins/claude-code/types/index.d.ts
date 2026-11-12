// The public contract of the jev-daemon plugin: the `$.jev` noun that other
// mods call. A plugin that lists jev-daemon under `dependencies` gets these
// types in its own .claude-plugin/types/. A breaking change here needs a new
// major version.

/**
 * Why a call returned no answer. Fall back to what your plugin does without
 * Jev. The codes from `jev` are its JSON error codes; a code this list does
 * not name comes from a newer `jev`, so treat it as an unknown failure. A
 * reply with no readable JSON is `jev-failed`.
 *
 * Did Jev get the request? (Spec section 6, "Did Jev get the request?")
 * - No: `bad-request`, `too-large`, `forbidden`, `pending`, `not-paired`,
 *   `revoked`, `no-key`, `bad-key`, `rate-limited`, `protocol-unsupported`,
 *   `internal`, `jev-missing`, `jev-outdated`.
 * - Yes: `jev-unauthorized`, `jev-http-error` (maybe twice: jevd retries a
 *   429 or 529 once), `bad-answer`.
 * - Maybe: `jev-timeout`, `jev-unreachable`, `daemon-unavailable`,
 *   `jev-failed`, and any code this list does not name.
 */
export type JevErrorCode =
  | 'pending'
  | 'not-paired'
  | 'revoked'
  | 'rate-limited'
  | 'no-key'
  | 'bad-key'
  | 'jev-unauthorized'
  | 'jev-http-error'
  | 'jev-timeout'
  | 'jev-unreachable'
  | 'bad-answer'
  | 'daemon-unavailable'
  | 'protocol-unsupported'
  | 'forbidden'
  | 'bad-request'
  | 'too-large'
  | 'internal'
  /** No `jev` on the PATH or at the `jevPath` option. */
  | 'jev-missing'
  /** The `jev` found is older than this plugin needs. */
  | 'jev-outdated'
  /** `jev` did not finish in time, or could not run. */
  | 'jev-failed'
  | (string & {})

export type JevFailure = {
  ok: false
  error: {
    code: JevErrorCode
    message: string
    /** Seconds until a `rate-limited` client can try again. */
    retryAfter?: number
    /** The profile whose key failed, for `no-key`, `bad-key`, and `jev-unauthorized`. */
    profile?: string
    /** For `pending`: the code that the person types at `jev allow`. */
    pairCode?: string
  }
}

/**
 * One question. `noul` asks for a probability from 0 to 1 that the answer is
 * yes. `score` asks for a score and a confidence, and takes `criteria`: one
 * description for each level, from the lowest to the highest.
 */
export type JevQuestion =
  | { type: 'noul'; instructions: string; [field: string]: unknown }
  | { type: 'score'; instructions: string; criteria: string[]; [field: string]: unknown }

export type JevAnswer =
  | { type: 'noul'; noul: number; [field: string]: unknown }
  | { type: 'score'; score: number; confidence: number; [field: string]: unknown }

export type JevAskInput = {
  /** What Jev reads. jevd redacts secrets with known shapes before the send. */
  state: unknown
  /** 1 to 20 questions by id: letters, digits, `_` and `-`. */
  questions: Record<string, JevQuestion>
  /** Your plugin's name. The host names the caller; a different name here is `bad-request`. */
  plugin?: string
}

export type JevAskResult = { ok: true; answers: Record<string, JevAnswer> } | JevFailure

export type JevStatusInput = { plugin?: string }

/**
 * `$.jev.status` reports a missing or old `jev` in its fields (`installed`,
 * `compatible`), never as `jev-missing` or `jev-outdated`. Its only failures
 * are `bad-request` (a caller name it refuses) and `internal` (the hook failed).
 */
export type JevStatus =
  | {
      ok: true
      /** Whether this plugin found a `jev` binary. */
      installed: boolean
      /** The version of the `jev` found, or null. */
      version: string | null
      /** Whether that `jev` is new enough for this plugin. `installed: true` with `version: null` means the version check did not finish, so `compatible` is unknown, not "update jev". */
      compatible: boolean
      /** Whether `jevd` runs. `$.jev.status` never starts it. If jevd stops between the two runs, `jev clients` can start it again. */
      running: boolean
      /** Whether jevd lists your plugin's client `cc-<plugin>`; null when jevd does not run, or when the mod could not read the client list. */
      paired: boolean | null
    }
  | JevFailure

/**
 * jev for Claude Code plugins. The host names your plugin from where the call
 * came from, so your plugin cannot speak for another. Your plugin pairs once
 * as the client `cc-<plugin>`; the person approves it with `jev allow`. Each
 * method takes one input object. No method throws: every failure is a
 * `JevFailure`.
 *
 * `$.jev` is missing when jev-daemon is off or not installed, and a call then
 * throws. Put each call in a `try`. Write each call as `$.jev.ask(...)`: the
 * hooks loader refuses `$.jev` in a variable and `$.jev?.ask`.
 */
export type Jev = {
  ask: (input: JevAskInput) => Promise<JevAskResult>
  /** See `JevStatus` for what it reports and how it can fail. */
  status: (input?: JevStatusInput) => Promise<JevStatus>
}

declare module 'claude-code' {
  interface EngineInterface {
    jev: Jev
  }
}
