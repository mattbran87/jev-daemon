// Type assertions on the plugin's public contract (types/index.d.ts). CI
// checks this file with tsc. Each line after `@ts-expect-error` must fail to
// compile; if it compiles, tsc reports the unused directive and fails.
import type { EngineInterface } from 'claude-code'
import type {
  JevAnswer,
  JevAskInput,
  JevAskResult,
  JevErrorCode,
  JevQuestion,
  JevStatus,
} from '../../plugins/claude-code/types/index'

// Questions: each type with its fields.
export const noul: JevQuestion = { type: 'noul', instructions: 'Is this a test?' }
export const score: JevQuestion = { type: 'score', instructions: 'How urgent?', criteria: ['Can wait', 'Now'] }
export const choice: JevQuestion = { type: 'choice', criteria: { angry: null, calm: 'A polite message' } }
// @ts-expect-error a choice question needs criteria
export const choiceWithoutCriteria: JevQuestion = { type: 'choice', instructions: 'What mood?' }
// @ts-expect-error a score question needs criteria
export const scoreWithoutCriteria: JevQuestion = { type: 'score', instructions: 'How urgent?' }

// Answers: the type field narrows each answer to its own fields.
export function valueOf(answer: JevAnswer): string | number {
  if (answer.type === 'choice') {
    const name: string = answer.choice
    const odds: Record<string, number> = answer.probabilities
    void odds
    // @ts-expect-error the choice is an option name, not a number
    const wrong: number = answer.choice
    void wrong
    return name
  }
  if (answer.type === 'noul') {
    const p: number = answer.noul
    return p
  }
  const s: number = answer.score
  const c: number = answer.confidence
  void c
  return s
}

// Error codes: the named codes, and any string from a newer jev.
export const named: Extract<JevErrorCode, 'not-paired' | 'jev-failed'>[] = ['not-paired', 'jev-failed']
export const codes: JevErrorCode[] = ['not-paired', 'jev-failed', 'a-code-from-a-newer-jev']
// @ts-expect-error an error code is a string
export const notACode: JevErrorCode = 7

// The engine: $.jev.ask and $.jev.status. The two positive calls below also
// prove that `$.jev` exists, so keep them: the `@ts-expect-error` at the end
// would pass on a missing `$.jev` too.
export async function ask($: EngineInterface, input: JevAskInput): Promise<JevAskResult> {
  const result = await $.jev.ask(input)
  // @ts-expect-error a result is an object, not any
  const bad: number = result
  void bad
  if (result.ok) {
    const answers: Record<string, JevAnswer> = result.answers
    void answers
  } else {
    const code: JevErrorCode = result.error.code
    void code
  }
  return result
}
export async function status($: EngineInterface): Promise<JevStatus> {
  const result = await $.jev.status()
  // @ts-expect-error a status is an object, not any
  const bad: number = result
  void bad
  return result
}
// @ts-expect-error an ask needs questions
export const noQuestions = ($: EngineInterface) => $.jev.ask({ state: 's' })
