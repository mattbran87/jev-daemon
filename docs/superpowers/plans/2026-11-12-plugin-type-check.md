# Plugin type check Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** CI fails when the plugin's public types in `plugins/claude-code/types/index.d.ts` break their contract.

**Architecture:** A folder `ci/plugin-types/` holds a pinned TypeScript, a stub for the module `claude-code`, a strict `tsconfig.json`, and `contract.ts` with type assertions. A new CI job runs `tsc -p` there on Ubuntu.

**Tech Stack:** TypeScript 7.0.2 (npm, exact version, lockfile), GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-11-12-plugin-type-check-design.md`

**Branch:** `ci/plugin-type-check` (the spec commit is on it).

**Commit identity for every commit:**

```bash
git -c user.name="Matthew Brandenburg" -c user.email="mattbran87@users.noreply.github.com" commit -m "<subject>" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

**Checked before this plan:** in a scratch folder, `typescript@7.0.2` installs a `tsc` command (`tsc --version` prints `Version 7.0.2`). Its lockfile lists the binary packages for Linux and Windows. A check like the one below passes on the real `index.d.ts`, and fails when one assertion breaks.

**Files:** write new files with an editor tool, never with PowerShell `Set-Content` or `Out-File` (they can add a byte order mark).

---

### Task 1: The check folder

**Files:**
- Create: `ci/plugin-types/package.json`
- Create: `ci/plugin-types/package-lock.json` (by `npm install`)
- Create: `ci/plugin-types/tsconfig.json`
- Create: `ci/plugin-types/claude-code.d.ts`
- Create: `ci/plugin-types/contract.ts`

- [ ] **Step 1: Write `package.json`**

```json
{
  "private": true,
  "description": "A type check of the jev-daemon plugin's public types. CI runs it; nothing here ships.",
  "devDependencies": {
    "typescript": "7.0.2"
  }
}
```

- [ ] **Step 2: Create the lockfile**

Run in `ci/plugin-types`: `npm install --no-audit --no-fund`
Expected: `added 2 packages`. Then check the lockfile:

Run: `Select-String -Path ci/plugin-types/package-lock.json -Pattern '"node_modules/@typescript/typescript-linux-x64"'`
Expected: one line. CI runs on Linux, so the lockfile must list the Linux binary.

- [ ] **Step 3: Write `tsconfig.json`**

```json
{
  "compilerOptions": {
    "target": "es2023",
    "lib": ["es2023"],
    "module": "esnext",
    "moduleResolution": "bundler",
    "strict": true,
    "noUncheckedIndexedAccess": true,
    "noEmit": true,
    "types": []
  },
  "files": [
    "claude-code.d.ts",
    "../../plugins/claude-code/types/index.d.ts",
    "contract.ts"
  ]
}
```

- [ ] **Step 4: Write `claude-code.d.ts`**

```ts
// Stands in for the module that Claude Code writes beside a mod it loads
// (.claude-plugin/types/claude-code/index.d.ts). The plugin's index.d.ts adds
// `jev` to EngineInterface, so the check needs the interface to exist.
declare module 'claude-code' {
  interface EngineInterface {}
}
```

- [ ] **Step 5: Write `contract.ts`**

```ts
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
```

- [ ] **Step 6: Run the check**

Run in `ci/plugin-types`: `npx --no-install tsc -p .`
Expected: no output, exit 0.

If an assertion fails or a `@ts-expect-error` is unused, read the message. If `index.d.ts` behaves as the spec says and the assertion is wrong, fix the assertion and report it. If `index.d.ts` is wrong, stop and report: this task does not change the plugin.

- [ ] **Step 7: Check that the check catches a broken type**

In `plugins/claude-code/types/index.d.ts`, change `choice: string;` in `JevAnswer` to `choice: number;`.
Run in `ci/plugin-types`: `npx --no-install tsc -p .`
Expected: errors, exit code not 0.

Undo the change: `git checkout -- plugins/claude-code/types/index.d.ts`.
Run: `git status --short plugins/claude-code`
Expected: no output.

Run the check again. Expected: no output, exit 0.

- [ ] **Step 8: Commit**

Run: `git status --short`
Expected: only `ci/plugin-types/` files. `node_modules/` must not show, because `.gitignore` ignores it.

```bash
git add ci/plugin-types/package.json ci/plugin-types/package-lock.json ci/plugin-types/tsconfig.json ci/plugin-types/claude-code.d.ts ci/plugin-types/contract.ts
git -c user.name="Matthew Brandenburg" -c user.email="mattbran87@users.noreply.github.com" commit -m "ci: a type check of the plugin's public types" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: The CI job

**Files:**
- Modify: `.github/workflows/ci.yml` (a new job after `mod`, before `deny`)

- [ ] **Step 1: Add the job**

In `.github/workflows/ci.yml`, insert this job after the last step of the `mod` job (`run: claude plugin test plugins/claude-code`) and before the line `  deny:`, with one blank line on each side:

```yaml
  plugin-types:
    name: plugin types
    runs-on: ubuntu-latest
    timeout-minutes: 10
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1  # v7.0.1
        with:
          persist-credentials: false
      - uses: actions/setup-node@949feb2413d6458794dcd2491c4babbbce0c15c1  # v7.1.0
        with:
          node-version: "22"
      - name: Install TypeScript
        working-directory: ci/plugin-types
        run: npm ci --no-audit --no-fund
      - name: Check the plugin's public types
        working-directory: ci/plugin-types
        run: npx --no-install tsc -p .
```

The two `uses:` lines must match the `mod` job's lines exactly. If they differ in the file, copy the `mod` job's lines and report it.

- [ ] **Step 2: Check the workflow file**

Run: `git diff --stat .github/workflows/ci.yml`
Expected: about 18 lines added, none removed.

Run: `claude plugin validate --strict .`
Expected: `Validation passed`. The marketplace check must not see the new folder as part of a plugin.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git -c user.name="Matthew Brandenburg" -c user.email="mattbran87@users.noreply.github.com" commit -m "ci: run the plugin type check" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: CHANGELOG

**Files:**
- Modify: `CHANGELOG.md`

- [ ] **Step 1: Add the line**

In `CHANGELOG.md`, add this line at the end of the list under `## Unreleased`:

```markdown
- CI checks the plugin's public types (`types/index.d.ts`) with TypeScript. The check needs no Claude Code session, so it does not cover `hooks/*.ts`. The programs and the plugin do not change.
```

Keep the file's line ends: check `git ls-files --eol CHANGELOG.md` before and after.

- [ ] **Step 2: Commit**

```bash
git add CHANGELOG.md
git -c user.name="Matthew Brandenburg" -c user.email="mattbran87@users.noreply.github.com" commit -m "docs: the plugin type check in the changelog" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: Final check

- [ ] **Step 1: Run the checks**

```bash
cd ci/plugin-types
npm ci --no-audit --no-fund
npx --no-install tsc -p .
cd ../..
claude plugin validate --strict .
claude plugin validate --strict plugins/claude-code
claude plugin test plugins/claude-code
```

Expected: `npm ci` adds 2 packages, `tsc` prints nothing, both validations pass, and every plugin test passes.

- [ ] **Step 2: Check the branch**

Run: `git log --oneline main..HEAD` and `git status --short`
Expected: 5 commits (the spec, the plan, then Tasks 1 to 3) and a clean tree.

Push, the pull request, and the merge wait for the person's approval. The new CI job runs for the first time on the pull request.
