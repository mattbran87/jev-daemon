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
  if (answer.type === 'noul') return answer.noul
  return answer.score
}

// Error codes: the named codes, and any string from a newer jev.
export const codes: JevErrorCode[] = ['not-paired', 'jev-failed', 'a-code-from-a-newer-jev']
// @ts-expect-error an error code is a string
export const notACode: JevErrorCode = 7

// The engine: $.jev.ask and $.jev.status.
export function ask($: EngineInterface, input: JevAskInput): Promise<JevAskResult> {
  return $.jev.ask(input)
}
export const status = ($: EngineInterface) => $.jev.status()
// @ts-expect-error an ask needs questions
export const noQuestions = ($: EngineInterface) => $.jev.ask({ state: 's' })
