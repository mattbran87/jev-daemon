import type { Register } from 'claude-code'

import type { JevFailure } from '../types'
import { ask, status } from './jev'
import { failure } from './text'

/** What a `$.jev` method answers when its hook failed, so no call ever rejects. */
const hookFailed = () => ({ value: failure('internal', 'the jev-daemon hook failed') }) as never

export const register: Register = (on, options) => {
  const configured = typeof options.jevPath === 'string' ? options.jevPath.trim() : ''

  // Every plugin that lists jev-daemon under dependencies gets $.jev. The `$`
  // at engine.create carries no nouns yet, so these methods only stand in: a
  // call to $.jev.ask is the event jev.ask, which the hooks below answer.
  on('engine.create', async (_$, e, next) => {
    const built = await next(e)
    const unanswered: JevFailure = failure('internal', 'no jev-daemon hook answered')
    return { ...built, jev: { ask: async () => unanswered, status: async () => unanswered } }
  })

  on('jev.ask', async ($, e, next) => ({ value: await ask($, e, next.origin.plugin, configured) }) as never).catch(
    () => hookFailed(),
  )

  // `$.jev.status()` with no input arrives as an absent e.
  on('jev.status', async ($, e, next) => ({ value: await status($, e, next.origin.plugin, configured) }) as never).catch(
    () => hookFailed(),
  )
}
