import { expect, test } from 'claude-code/testing'

import {
  MAX_REQUEST_BYTES,
  callerName,
  clean,
  clientName,
  keyToast,
  olderThan,
  parseVersion,
  pendingNoCodeToast,
  pendingToast,
  readReply,
  requestBytes,
} from './text'

function failureOf(stdout: string, exit: number) {
  const parsed = readReply(stdout, exit)
  if (parsed.ok) throw new Error('expected a failure')
  return parsed.error
}

test('the caller is the host name, and an input plugin must match it', () => {
  expect(callerName('acme-review', {})).toEqual({ ok: true, plugin: 'acme-review' })
  expect(callerName('acme-review', { plugin: 'acme-review' })).toEqual({ ok: true, plugin: 'acme-review' })
  expect(callerName('acme-review', { plugin: 'other' }).ok).toBe(false)
  for (const bad of ['engine', 'client', 'Acme', '-acme', 'a'.repeat(62), 'acme_review', '']) {
    expect(callerName(bad, {}).ok).toBe(false)
  }
  expect(callerName('a'.repeat(61), undefined).ok).toBe(true)
  expect(clientName('a'.repeat(61)).length).toBe(64)
})

test('the version comes from jev --version, and older versions are refused', () => {
  expect(parseVersion('jev 0.1.0\n')).toEqual([0, 1, 0])
  expect(parseVersion('jev 12.3.40')).toEqual([12, 3, 40])
  expect(parseVersion('jevd 0.1.0')).toBe(null)
  expect(parseVersion('jev 0.1')).toBe(null)
  expect(parseVersion('')).toBe(null)
  expect(olderThan([0, 0, 9], [0, 1, 0])).toBe(true)
  expect(olderThan([0, 1, 0], [0, 1, 0])).toBe(false)
  expect(olderThan([1, 0, 0], [0, 9, 9])).toBe(false)
  expect(olderThan([0, 10, 0], [0, 9, 0])).toBe(false)
})

test('the size counts UTF-8 bytes', () => {
  expect(requestBytes('ab')).toBe(2)
  expect(requestBytes('é')).toBe(2)
  expect(requestBytes('\u{1F600}')).toBe(4)
  expect(MAX_REQUEST_BYTES).toBe(1048576)
})

test('a success reply passes its data', () => {
  expect(readReply('{"ok":true,"answers":{"q":{"type":"noul","noul":0.5}}}\n', 0)).toEqual({
    ok: true,
    data: { ok: true, answers: { q: { type: 'noul', noul: 0.5 } } },
  })
})

test('a failure keeps its code, even one this mod does not know', () => {
  expect(readReply('{"ok":false,"error":{"code":"future-code","message":"new"}}', 1)).toEqual({
    ok: false,
    error: { code: 'future-code', message: 'new' },
  })
})

test('a failure passes on retry_after, a valid profile, and a valid pair code only', () => {
  const line = JSON.stringify({
    ok: false,
    error: { code: 'pending', message: 'm', retry_after: 7, profile: 'work' },
    pair_code: 'ABCD2345',
  })
  expect(readReply(line, 3)).toEqual({
    ok: false,
    error: { code: 'pending', message: 'm', retryAfter: 7, profile: 'work', pairCode: 'ABCD2345' },
  })
  const bad = JSON.stringify({ ok: false, error: { code: 'pending', message: 'm', profile: 'Bad Name' }, pair_code: 'abcd0000' })
  expect(readReply(bad, 3)).toEqual({ ok: false, error: { code: 'pending', message: 'm' } })
  const long = JSON.stringify({ ok: false, error: { code: 'internal', message: 'x'.repeat(1000) } })
  const parsed = readReply(long, 1)
  expect(parsed.ok === false && parsed.error.message.length).toBe(300)
})

test('with no JSON, every exit code is jev-failed and the message names it', () => {
  for (const exit of [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 42]) {
    const error = failureOf('not json', exit)
    expect(error.code).toBe('jev-failed')
    expect(error.message).toBe(`jev printed no result (exit ${exit})`)
  }
  expect(failureOf('{"ok":"yes"}', 0).code).toBe('jev-failed')
  expect(failureOf('', 0).code).toBe('jev-failed')
  expect(failureOf('{"ok":false,"error":{"code":"internal","message":"a"}}\n{"ok":true}', 1).code).toBe('jev-failed')
  expect(failureOf('{"ok":false,"error":"boom"}', 1).code).toBe('jev-failed')
})

test('a success with no data still passes', () => {
  expect(readReply('{"ok":true}', 0)).toEqual({ ok: true, data: { ok: true } })
})

test('a bad input plugin field is bad-request', () => {
  expect(callerName('acme', { plugin: null }).ok).toBe(false)
  expect(callerName('acme', { plugin: 42 }).ok).toBe(false)
  const bad = callerName('Acme', {})
  expect(bad.ok === false && bad.error.message.startsWith('"Acme" is not')).toBe(true)
})

test('an error code must have the name shape', () => {
  for (const code of ['Bad Code', 'a'.repeat(65), '']) {
    expect(failureOf(JSON.stringify({ ok: false, error: { code, message: 'm' } }), 1).code).toBe('jev-failed')
  }
  expect(failureOf(JSON.stringify({ ok: false, error: { code: 'a'.repeat(64), message: 'm' } }), 1).code).toBe('a'.repeat(64))
})

test('the message is cleaned and cut by code points', () => {
  const dirty = JSON.stringify({ ok: false, error: { code: 'internal', message: 'a\u001bb\u202ec\u0085d' } })
  expect(failureOf(dirty, 1).message).toBe('a b c d')
  expect(clean('x\u200by\ufeffz\u{E0041}')).toBe('x y z ')
  const emoji = JSON.stringify({ ok: false, error: { code: 'internal', message: 'a'.repeat(299) + '\u{1F600}' + 'zzz' } })
  const message = failureOf(emoji, 1).message
  expect(Array.from(message).length).toBe(300)
  expect(message.endsWith('\u{1F600}')).toBe(true)
  expect(failureOf(JSON.stringify({ ok: false, error: { code: 'internal', message: 5 } }), 1).message).toBe('')
})

test('retry_after must be a whole number of seconds, 0 or more', () => {
  for (const bad of [-1, 1.5]) {
    const line = JSON.stringify({ ok: false, error: { code: 'rate-limited', message: 'm', retry_after: bad } })
    expect(failureOf(line, 5).retryAfter).toBe(undefined)
  }
  const ok = JSON.stringify({ ok: false, error: { code: 'rate-limited', message: 'm', retry_after: 0 } })
  expect(failureOf(ok, 5).retryAfter).toBe(0)
})

test('a pre-release version parses', () => {
  expect(parseVersion('jev 0.2.0-rc.1')).toEqual([0, 2, 0])
  expect(parseVersion('jev 0.2.0+build5\n')).toEqual([0, 2, 0])
})

test('each toast holds its argument', () => {
  expect(pendingToast('acme', 'ABCD2345')).toContain('jev allow ABCD2345')
  expect(pendingNoCodeToast('acme')).toContain('jev allow')
  expect(keyToast('work')).toContain('--profile work')
  expect(keyToast(undefined)).toContain('--profile default')
})
