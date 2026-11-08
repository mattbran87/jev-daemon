# License Notices in Every Archive (0.1.2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Each release archive holds `THIRD-PARTY-LICENSES.md`, with the license and notice text of every crate that `jev`, `jevd`, and `jev-tray` contain. CI fails when the file is out of date. The release is 0.1.2.

**Architecture:** `cargo about generate` writes the file from `about.toml` (accepted licenses, the 4 release targets, no build or dev crates) and `about.hbs` (the Markdown layout). `dist` adds the file to each archive through `include` in the `[package.metadata.dist]` table of the `jev` package. A new CI job, `licenses`, writes the file again on Linux and compares it with the committed file.

**Tech Stack:** `cargo-about` 0.9.2 (pinned), `dist` 0.33.1, GitHub Actions, CI toolchain Rust 1.99.1.

**Spec:** `docs/superpowers/specs/2026-11-07-license-notices-design.md`.

---

## Before you start

- Work on the branch `license-notices`. It holds the spec commit, 2a2b1b2.
- Run every command from the repo root, in Git Bash on Windows or in bash on Linux or macOS. The plan was tested in Git Bash on Windows. The CI step was also tested on Linux, with the same toolchain as CI.
- The repo has `core.autocrlf=true`. Write files with LF line ends. Git stores them with LF.
- Commit as the user, and end each commit message with the trailer:

```bash
git -c user.name="Matthew Brandenburg" -c user.email="mattbran87@users.noreply.github.com" commit -m "<subject>" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

## Design notes

Read these before Task 1. They explain choices that the spec leaves open, and the facts the plan found when it built each part.

- **The `cargo-about` version.** The plan pins 0.9.2, released 2026-08-18. The newer 0.9.3 came out on 2026-10-26, less than 2 weeks before this plan. 0.9.2 needs Rust 1.88 to build. It is a tool, not a dependency, so the 1.85 MSRV does not apply. CI builds it with Rust 1.99.1.
- **The `cli` feature.** Since 0.9.0, the `cargo-about` binary needs the `cli` feature. Without it, `cargo install` builds the library and fails with `none of the package's binaries are available for install using the selected features`. Each install command in this plan adds `--features cli`.
- **No network.** `cargo-about` removed its ClearlyDefined lookups in 0.8.0. The plan runs it with `--frozen` (`--locked` and `--offline`), so `cargo-about` makes no web client at all. `about.toml` has no `clarify` entries and no `workarounds`, so nothing needs a git fetch. The output depends only on `Cargo.lock` and the crate sources. `--frozen` needs the crate sources on disk. CI runs `cargo fetch --locked` first, which downloads the crates of every target.
- **The workspace crates.** `cargo-about` lists the 4 workspace crates as dependencies. Its `private.ignore` setting skips only crates with `publish = false`, and `publish = false` would also make `dist` skip the `jev` package. So the template leaves out each crate with no `source`, which is a crate of this workspace. All 4 share one Apache-2.0 text block with 35 crates.io crates, so no text block stays without a crate.
- **One block per distinct text.** `cargo-about` keeps each distinct license text once. Most MIT, BSD, and ISC texts carry a copyright line, and those licenses ask that the line ship with the binaries. So a license group can hold more than one text block. Each block gives the text, then lists the crates that use exactly that text. The file has 6 license groups and 70 text blocks.
- **The licenses in the file.** `accepted` in `about.toml` is the `allow` list of `deny.toml`, in the same order. For a crate under `A OR B`, `cargo-about` takes the first of the two in that list. The file then holds 6 licenses: Apache-2.0, BSD-3-Clause, ISC, MIT, MPL-2.0 (only `option-ext`), and Unicode-3.0. These match the list in the first paragraph of the spec. The other licenses in the `allow` list (for example `Unlicense`, `CC0-1.0`, and `MIT-0`) appear in the tree only as an `OR` choice.
- **Build and dev crates.** `ignore-build-dependencies` and `ignore-dev-dependencies` leave them out. `wiremock`, `cc`, `pkg-config`, and `system-deps` do not appear. `tempfile` appears: it is a normal dependency of `jevd`. Proc-macro crates such as `syn` and `serde_derive` are normal dependencies, so they appear too. The spec allows a list longer than the archives need.
- **The same bytes on each run and each platform.** 12 runs on Windows gave the same bytes. A run on Linux, with Rust 1.99.1, gave the same bytes as Windows.
- **CR bytes.** 3 crates ship a license file with CRLF line ends: `proc-macro-error` 1.0.4, `async-recursion` 1.2.0, and `generic-array` 0.14.7. `cargo-about` copies their text as it is, so the file that it writes has 421 lines that end in CRLF, on every platform. `.gitattributes` gives the file `text eol=lf`, so git stores LF only, and each checkout, Windows too, has LF only. The CI job compares with `diff --strip-trailing-cr`, so those CRs do not count. `about.hbs` also gets `text eol=lf`, so a refresh on Windows does not put a CR into each template line.
- **NOTICE files.** The plan searched the source of each of the 310 crates in the file for a file named `NOTICE` or `NOTICE.*`, at any depth. It found none. So `about.hbs` has no Notices section (spec section 3). Task 1, Step 8 repeats the search.
- **`dist`.** The `include` path is relative to `crates/jev/Cargo.toml`. A local `dist build` for `x86_64-pc-windows-msvc` put the file in the zip. `dist plan` lists it in all 4 archives. `dist generate --check` passes, so `release.yml` does not change.
- **The version.** The workspace crates are not in the file, so the change to 0.1.2 does not change it.

## File map

| File | Change |
| --- | --- |
| `about.toml` | New: the `cargo-about` configuration. |
| `about.hbs` | New: the Markdown template. |
| `THIRD-PARTY-LICENSES.md` | New: generated. |
| `.gitattributes` | LF for `about.hbs` and `THIRD-PARTY-LICENSES.md`. |
| `crates/jev/Cargo.toml` | `[package.metadata.dist]` with `include`. |
| `.github/workflows/ci.yml` | New job `licenses`. |
| `README.md`, `CHANGELOG.md` | Spec section 6. |
| `Cargo.toml`, `Cargo.lock`, `plugins/claude-code/.claude-plugin/plugin.json` | Version 0.1.2. |

---

### Task 1: Generate THIRD-PARTY-LICENSES.md

**Files:**
- Create: `about.toml`
- Create: `about.hbs`
- Create: `THIRD-PARTY-LICENSES.md` (generated)
- Modify: `.gitattributes`

- [ ] **Step 1: Install `cargo-about`**

Run:

```bash
cargo install --locked --version 0.9.2 --features cli cargo-about
cargo about --version
```

Expected: the last line is `cargo-about 0.9.2`. The build takes about 4 minutes.

- [ ] **Step 2: LF for the template and the file**

Replace the whole of `.gitattributes` with:

```gitattributes
*.png binary
*.sh  text eol=lf
*.svg text eol=lf
# cargo about writes THIRD-PARTY-LICENSES.md from about.hbs. Both are stored
# and checked out with LF only, on Windows too. A few crates ship a license
# file with CRLF line ends; git turns those CRs into LF when it stores the file.
about.hbs               text eol=lf
THIRD-PARTY-LICENSES.md text eol=lf
```

- [ ] **Step 3: Write the template**

Create `about.hbs` with exactly this content. The file ends with one newline after `{{/each}}`.

````handlebars
# Third-party licenses

`jev`, `jevd`, and `jev-tray` contain code from the crates below. This file gives the license of each crate and its notice text. The license of this project is in [LICENSE](LICENSE).

This file is generated. Do not edit it by hand. To refresh it, see the Licenses section of [README.md](README.md).

## Licenses

{{#each licenses}}
{{#if first_of_kind}}
- {{{name}}} (`{{id}}`)
{{/if}}
{{/each}}
{{#each licenses}}
{{#if first_of_kind}}

## {{{name}}} (`{{id}}`)
{{/if}}

```text
{{{text}}}
```

Used by:

{{! A crate of this workspace has no source. LICENSE covers it, so it is left out. }}
{{#each used_by}}
{{#if crate.source}}
- {{crate.name}} {{crate.version}}
{{/if}}
{{/each}}
{{/each}}
````

Notes on the template:

- `{{{name}}}` and `{{{text}}}` use 3 braces, so Handlebars does not turn `"` into `&quot;`.
- The template never prints `source_path` or `path`. They hold local folder paths.
- The license list at the top comes from `licenses` with `first_of_kind`, not from `overview`. `overview` sorts by count, and its count includes the workspace crates.

- [ ] **Step 4: Run the generator with no configuration, and see it fail**

Run: `cargo about generate --frozen --workspace -o THIRD-PARTY-LICENSES.md about.hbs`
Expected: FAIL, exit code 1. The last line is `encountered 412 errors resolving licenses, unable to generate output`. With no `about.toml`, no license is accepted.

- [ ] **Step 5: Write the configuration**

Create `about.toml`:

```toml
# cargo-about: writes THIRD-PARTY-LICENSES.md (see the README, Licenses).
# cargo deny decides which licenses may enter the tree. This file only
# writes their texts, so `accepted` is the `allow` list of deny.toml.

accepted = [
    "Apache-2.0",
    "Apache-2.0 WITH LLVM-exception",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "CC0-1.0",
    "ISC",
    "MIT",
    "MIT-0",
    "Unicode-3.0",
    "Unlicense",
]

# The release targets, as in deny.toml and dist-workspace.toml.
targets = [
    "x86_64-pc-windows-msvc",
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "aarch64-apple-darwin",
]

# No archive holds a crate that only tests or build scripts use.
ignore-build-dependencies = true
ignore-dev-dependencies = true

# option-ext: MPL-2.0, as the exception in deny.toml allows. Crate settings
# must come last in this file.
[option-ext]
accepted = ["MPL-2.0"]
```

- [ ] **Step 6: Generate the file**

Run: `cargo about generate --frozen --workspace -o THIRD-PARTY-LICENSES.md about.hbs`
Expected: PASS, exit code 0, no output.

If it says that it cannot fetch in offline mode, run `cargo fetch --locked`, then run it again.

- [ ] **Step 7: Check the file**

Run each command:

```bash
wc -lc THIRD-PARTY-LICENSES.md
grep -c '^Used by:$' THIRD-PARTY-LICENSES.md
awk '/^Used by:$/{list=1; next} /^```text$/{list=0} list && /^- /' THIRD-PARTY-LICENSES.md | sort -u | wc -l
grep -n '^## ' THIRD-PARTY-LICENSES.md
grep -n -x -e '- option-ext 0.2.0' -e '- tempfile 3.27.0' THIRD-PARTY-LICENSES.md
grep -E '^- (jev|jevd|jev-proto|jev-tray|wiremock|cc|pkg-config|system-deps) ' THIRD-PARTY-LICENSES.md
tr -cd '\r' < THIRD-PARTY-LICENSES.md | wc -c
```

Expected, in order:

1. `  8011 417345 THIRD-PARTY-LICENSES.md`. With the `Cargo.lock` of commit 2a2b1b2, the numbers are exact. After another dependency change, the size stays near 417 KB, from 400 000 to 440 000 bytes.
2. `70`: the text blocks.
3. `310`: the crates. A crate under 2 licenses appears in 2 blocks, so `sort -u` counts it once.
4. The license groups, in this order:

```text
7:## Licenses
16:## Apache License 2.0 (`Apache-2.0`)
6495:## BSD 3-Clause "New" or "Revised" License (`BSD-3-Clause`)
6553:## ISC License (`ISC`)
6640:## MIT License (`MIT`)
7510:## Mozilla Public License 2.0 (`MPL-2.0`)
7893:## Unicode License v3 (`Unicode-3.0`)
```

5. Two lines:

```text
4551:- tempfile 3.27.0
7891:- option-ext 0.2.0
```

6. No output, exit code 1: no workspace crate, no dev crate, and no build crate.
7. `421`: the CR bytes from 3 crates (see Design notes). Git removes them in Step 9.

- [ ] **Step 8: Check that each run gives the same bytes, and search for NOTICE files**

Run:

```bash
cargo about generate --frozen --workspace -o target/THIRD-PARTY-LICENSES.again.md about.hbs
cmp THIRD-PARTY-LICENSES.md target/THIRD-PARTY-LICENSES.again.md && echo same
```

Expected: `same`.

Then search the source of each crate for a NOTICE file. A one-time template lists each crate as `<name>-<version>`, which is the name of its folder in the cargo registry:

```bash
printf '{{#each crates}}{{package.name}}-{{package.version}}\n{{/each}}' > target/crates.hbs
cargo about generate --frozen --workspace -o target/crates.txt target/crates.hbs
rm -f target/crate-dirs.txt
for crate in $(cat target/crates.txt); do
  for dir in "${CARGO_HOME:-$HOME/.cargo}"/registry/src/*/"$crate"; do
    if [ -d "$dir" ]; then echo "$dir" >> target/crate-dirs.txt; fi
  done
done
wc -l < target/crate-dirs.txt
find $(cat target/crate-dirs.txt) -type f \( -iname NOTICE -o -iname 'NOTICE.*' \)
echo "search done"
find $(cat target/crate-dirs.txt) -maxdepth 1 -type f -iname 'LICENSE*' | wc -l
```

Expected:

```text
310
search done
529
```

`310` is the number of crate folders found. The first `find` prints nothing: no crate has a NOTICE file. The last line is a control: the same search finds the `LICENSE*` files, so the search works. The 4 workspace crates are not in the registry, so the loop skips them.

If the first `find` prints a file, stop. Spec section 3 then asks for a Notices section in `about.hbs`, and this plan does not cover it.

- [ ] **Step 9: Commit**

```bash
git add .gitattributes about.toml about.hbs THIRD-PARTY-LICENSES.md
git -c user.name="Matthew Brandenburg" -c user.email="mattbran87@users.noreply.github.com" commit -m "feat: THIRD-PARTY-LICENSES.md, generated by cargo about" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

`git add` can warn that CRLF will be replaced by LF in `THIRD-PARTY-LICENSES.md`. That is the expected result of `text eol=lf`.

Check that git stores LF only:

```bash
git show HEAD:THIRD-PARTY-LICENSES.md | tr -cd '\r' | wc -c
```

Expected: `0`.

---

### Task 2: The file in each archive

**Files:**
- Modify: `crates/jev/Cargo.toml`

- [ ] **Step 1: See that the archives lack the file**

Run: `dist plan 2>/dev/null | grep misc`
Expected: 4 lines, one for each archive, with no `THIRD-PARTY-LICENSES.md`:

```text
      [misc] CHANGELOG.md, LICENSE, README.md
      [misc] CHANGELOG.md, LICENSE, README.md
      [misc] CHANGELOG.md, LICENSE, README.md
      [misc] CHANGELOG.md, LICENSE, README.md
```

- [ ] **Step 2: Add `include`**

In `crates/jev/Cargo.toml`, replace:

```toml
# jev-tray needs GTK 3 to build on Linux, and it loads the AppIndicator
# library at run time.
[package.metadata.dist.dependencies.apt]
```

with:

```toml
# Each archive holds the notices of the dependencies (see the README, Licenses).
[package.metadata.dist]
include = ["../../THIRD-PARTY-LICENSES.md"]

# jev-tray needs GTK 3 to build on Linux, and it loads the AppIndicator
# library at run time.
[package.metadata.dist.dependencies.apt]
```

The path is relative to `crates/jev/Cargo.toml`.

- [ ] **Step 3: See the file in each archive**

Run: `dist plan 2>/dev/null | grep misc`
Expected:

```text
      [misc] CHANGELOG.md, LICENSE, README.md, THIRD-PARTY-LICENSES.md
      [misc] CHANGELOG.md, LICENSE, README.md, THIRD-PARTY-LICENSES.md
      [misc] CHANGELOG.md, LICENSE, README.md, THIRD-PARTY-LICENSES.md
      [misc] CHANGELOG.md, LICENSE, README.md, THIRD-PARTY-LICENSES.md
```

Run: `dist generate --check`
Expected: no output, exit code 0. `release.yml` does not change.

- [ ] **Step 4: Build one archive and look inside**

On Windows, run:

```bash
dist build --artifacts=local --target x86_64-pc-windows-msvc
unzip -l target/distrib/jev-x86_64-pc-windows-msvc.zip
```

Expected: the build takes about 5 minutes. Its last lines name the zip, then:

```text
      [bin] jev.exe
      [bin] jev-tray.exe
      [bin] jevd.exe
      [misc] CHANGELOG.md, LICENSE, README.md, THIRD-PARTY-LICENSES.md
```

`unzip -l` lists 7 files: `CHANGELOG.md`, `jev-tray.exe`, `jev.exe`, `jevd.exe`, `LICENSE`, `README.md`, and `THIRD-PARTY-LICENSES.md`, about 417 KB.

On Linux or macOS, build the archive of the host target instead, and list it with `tar -tf`. The pull request's `release` workflow run also builds all 4 archives (`pr-run-mode = "upload"`). Task 4, Step 8 checks one of them.

- [ ] **Step 5: Commit**

```bash
git add crates/jev/Cargo.toml
git -c user.name="Matthew Brandenburg" -c user.email="mattbran87@users.noreply.github.com" commit -m "feat(dist): each archive holds THIRD-PARTY-LICENSES.md" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: The `licenses` job in CI

**Files:**
- Modify: `.github/workflows/ci.yml`

- [ ] **Step 1: Add the job**

At the end of `.github/workflows/ci.yml`, after the `deny` job, add one blank line and then:

```yaml
  licenses:
    # THIRD-PARTY-LICENSES.md must be what cargo about writes from Cargo.lock now.
    name: licenses
    runs-on: ubuntu-latest
    timeout-minutes: 20
    env:
      # Pinned: a new cargo-about can change the file with no change in this repo.
      CARGO_ABOUT_VERSION: "0.9.2"
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1  # v7.0.1
        with:
          persist-credentials: false
      - name: Install Rust
        run: |
          rustup toolchain install "$RUST_VERSION" --profile minimal
          rustup default "$RUST_VERSION"
      - uses: Swatinem/rust-cache@d1527c8f67ef9af357290475d4af4b0fab8f12e0  # v2.9.3
      - name: Install cargo-about
        # The cargo-about binary needs the cli feature.
        run: cargo install --locked --version "$CARGO_ABOUT_VERSION" --features cli cargo-about
      - name: THIRD-PARTY-LICENSES.md is up to date
        # cargo fetch downloads the crates of every target. --frozen then keeps
        # cargo about off the network, so the file depends only on Cargo.lock.
        # A few crates ship a license file with CRLF line ends. Git stores this
        # file with LF only (.gitattributes), so the comparison ignores a CR.
        run: |
          cargo fetch --locked
          cargo about generate --frozen --workspace -o "$RUNNER_TEMP/THIRD-PARTY-LICENSES.md" about.hbs
          if ! diff -q --strip-trailing-cr THIRD-PARTY-LICENSES.md "$RUNNER_TEMP/THIRD-PARTY-LICENSES.md" > /dev/null; then
            diff -u --strip-trailing-cr THIRD-PARTY-LICENSES.md "$RUNNER_TEMP/THIRD-PARTY-LICENSES.md" | head -n 100 || true
            echo "::error file=THIRD-PARTY-LICENSES.md::THIRD-PARTY-LICENSES.md is out of date. Refresh it with: cargo about generate --frozen --workspace -o THIRD-PARTY-LICENSES.md about.hbs"
            exit 1
          fi
```

The job uses the workflow's `on`, so it runs on each push to `main` and on each pull request. `RUST_VERSION` comes from the workflow's `env`. `rust-cache` also keeps `~/.cargo/bin`, so a later run skips most of the `cargo-about` build.

- [ ] **Step 2: Run the step's lines on the committed file**

GitHub runs a `run:` block with `bash --noprofile --norc -eo pipefail`. Run the same lines that way:

```bash
export RUNNER_TEMP="$(mktemp -d)"
bash --noprofile --norc -eo pipefail -c '
cargo fetch --locked
cargo about generate --frozen --workspace -o "$RUNNER_TEMP/THIRD-PARTY-LICENSES.md" about.hbs
if ! diff -q --strip-trailing-cr THIRD-PARTY-LICENSES.md "$RUNNER_TEMP/THIRD-PARTY-LICENSES.md" > /dev/null; then
  diff -u --strip-trailing-cr THIRD-PARTY-LICENSES.md "$RUNNER_TEMP/THIRD-PARTY-LICENSES.md" | head -n 100 || true
  echo "::error file=THIRD-PARTY-LICENSES.md::THIRD-PARTY-LICENSES.md is out of date. Refresh it with: cargo about generate --frozen --workspace -o THIRD-PARTY-LICENSES.md about.hbs"
  exit 1
fi
'; echo "exit $?"
```

Expected: `exit 0`. To test the LF copy that CI checks out, first run `rm THIRD-PARTY-LICENSES.md` and `git checkout -- THIRD-PARTY-LICENSES.md`. The result is the same.

- [ ] **Step 3: Change the file, and see the step fail**

Run:

```bash
sed -i 's/^- option-ext 0.2.0$/- option-ext 0.2.1/' THIRD-PARTY-LICENSES.md
```

Then run the block of Step 2 again.
Expected: FAIL. The output is this diff (the dates and the temporary folder differ), then the error, then `exit 1`:

```text
--- THIRD-PARTY-LICENSES.md	<date>
+++ <temporary folder>/THIRD-PARTY-LICENSES.md	<date>
@@ -7888,7 +7888,7 @@
 
 Used by:
 
-- option-ext 0.2.1
+- option-ext 0.2.0
 
 ## Unicode License v3 (`Unicode-3.0`)
 
::error file=THIRD-PARTY-LICENSES.md::THIRD-PARTY-LICENSES.md is out of date. Refresh it with: cargo about generate --frozen --workspace -o THIRD-PARTY-LICENSES.md about.hbs
exit 1
```

- [ ] **Step 4: Put the file back**

Run:

```bash
git checkout -- THIRD-PARTY-LICENSES.md
git status --short
```

Expected: `git status --short` prints nothing.

- [ ] **Step 5: Commit**

```bash
git add .github/workflows/ci.yml
git -c user.name="Matthew Brandenburg" -c user.email="mattbran87@users.noreply.github.com" commit -m "ci: the licenses job checks THIRD-PARTY-LICENSES.md" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: Documents, version 0.1.2, gates, and the pull request

**Files:**
- Modify: `README.md`, `CHANGELOG.md`
- Modify: `Cargo.toml`, `Cargo.lock`, `plugins/claude-code/.claude-plugin/plugin.json`

- [ ] **Step 1: The README**

In `README.md`, replace:

```markdown
**Status:** 0.1.1. See [CHANGELOG.md](CHANGELOG.md).
```

with:

```markdown
**Status:** 0.1.2. See [CHANGELOG.md](CHANGELOG.md).
```

Then replace the last section:

```markdown
## License

Apache-2.0. See [LICENSE](LICENSE).
```

with:

````markdown
## Licenses

This project is under Apache-2.0. See [LICENSE](LICENSE).

`jev`, `jevd`, and `jev-tray` contain code from other crates. [THIRD-PARTY-LICENSES.md](THIRD-PARTY-LICENSES.md) holds the license and notice text of each one. Each release archive holds this file too.

The file is generated. After a change to the dependencies, refresh it with `cargo-about` 0.9.2:

```bash
cargo install --locked --version 0.9.2 --features cli cargo-about
cargo about generate --frozen --workspace -o THIRD-PARTY-LICENSES.md about.hbs
```

If `cargo about` says that it cannot fetch in offline mode, run `cargo fetch --locked` first. CI fails when the file is out of date.

When you add a dependency, look in its source for a file named `NOTICE` or `NOTICE.*`. Apache-2.0 asks us to pass that text on, and `cargo about` does not collect it. If you find one, add a Notices section to `about.hbs`.
````

The heading is `## Licenses`, because `about.hbs` points to "the Licenses section of README.md".

- [ ] **Step 2: The CHANGELOG**

In `CHANGELOG.md`, replace:

```markdown
# Changelog

## 0.1.1
```

with:

```markdown
# Changelog

## 0.1.2

- Each release archive holds `THIRD-PARTY-LICENSES.md`. It gives the license and notice text of each crate that `jev`, `jevd`, and `jev-tray` contain. The archives of 0.1.0 and 0.1.1 hold only this project's `LICENSE`.
- CI fails when `THIRD-PARTY-LICENSES.md` is out of date.
- The programs and the Claude Code plugin do not change.

Known limits:

- Nobody has checked `jev-tray` by hand on macOS or on a Linux desktop yet. CI builds and tests both systems, but it has no real tray.
- The macOS binaries are not signed or notarized.
- **Stop jevd** does not last: a plugin's next request starts `jevd` again.
- `jev ask --score` has no way to give `criteria`. The plugin's types list `criteria` as needed on a score question. To ask a score question, give the whole request as JSON on stdin or with `--request`.

## 0.1.1
```

The 0.1.1 and 0.1.0 entries stay as they are.

- [ ] **Step 3: The version**

In `Cargo.toml`, under `[workspace.package]`, replace:

```toml
version = "0.1.1"
```

with:

```toml
version = "0.1.2"
```

In `Cargo.lock`, change the version of the 4 workspace crates, and nothing else. For each `name` below, the next line `version = "0.1.1"` becomes `version = "0.1.2"`:

```toml
name = "jev"
version = "0.1.2"
```

```toml
name = "jev-proto"
version = "0.1.2"
```

```toml
name = "jev-tray"
version = "0.1.2"
```

```toml
name = "jevd"
version = "0.1.2"
```

`ndk-context` also has `version = "0.1.1"` in `Cargo.lock`. It stays.

In `plugins/claude-code/.claude-plugin/plugin.json`, replace:

```json
  "version": "0.1.1",
```

with:

```json
  "version": "0.1.2",
```

The path dependencies in the crate manifests say `version = "0.1.0"`. That requirement accepts 0.1.2, so they stay. `MIN_JEV` in `plugins/claude-code/hooks/text.ts` stays `[0, 1, 0]`.

Run: `cargo test -p jev --locked --test versions`
Expected: PASS, `test result: ok. 3 passed`. A wrong `Cargo.lock` fails here with `the lock file ... needs to be updated but --locked was passed`.

- [ ] **Step 4: The version does not change the notices**

The workspace crates are not in the file, so the new version must not change it. Run:

```bash
cargo about generate --frozen --workspace -o target/THIRD-PARTY-LICENSES.again.md about.hbs
diff -q --strip-trailing-cr THIRD-PARTY-LICENSES.md target/THIRD-PARTY-LICENSES.again.md && echo same
```

Expected: `same`.

- [ ] **Step 5: All gates**

From the repo root:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo deny check
```

Expected: all pass. On Windows, `cargo test --workspace --locked` reports 450 tests: 449 passed and 1 ignored. `cargo deny check` ends with `advisories ok, bans ok, licenses ok, sources ok`. A new advisory can appear on any day with no change in this repo. If one does, report it to the person, and do not change `deny.toml` in this plan.

This plan changes no Rust code and no plugin code. CI's `msrv` and `mod` jobs check those on the pull request.

- [ ] **Step 6: Commit**

```bash
git add README.md CHANGELOG.md Cargo.toml Cargo.lock plugins/claude-code/.claude-plugin/plugin.json
git -c user.name="Matthew Brandenburg" -c user.email="mattbran87@users.noreply.github.com" commit -m "docs: 0.1.2, license notices in every archive" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

- [ ] **Step 7: Push and open the pull request, after the person approves**

Do not push before the person approves. Then push the branch `license-notices` and open a pull request into `main`. The description says that each archive now holds `THIRD-PARTY-LICENSES.md`, names the new CI job `licenses`, and ends with:

```markdown
🤖 Generated with [Claude Code](https://claude.com/claude-code)
```

- [ ] **Step 8: Check the pull request's runs**

1. In the `ci` run, the `licenses` job passes.
2. The `release` run builds every archive and uploads it to the run (`pr-run-mode = "upload"`). Download `jev-x86_64-unknown-linux-gnu.tar.xz` from the run's artifacts. `tar -tf jev-x86_64-unknown-linux-gnu.tar.xz` lists `THIRD-PARTY-LICENSES.md`.

The tag `v0.1.2` is not a step of this plan. It follows the merge, and only after the person approves the release. The private marketplace then moves its pin to `v0.1.2`, in that repository, by its own session.

---

## Self-review against the spec

| Spec | Where |
| --- | --- |
| 1. The file at the root, grouped by license, text then crates, every target, no workspace, test, or build crate | Task 1, Steps 3, 5, and 7 |
| 1. The file is generated | Task 1, Step 3 (the header text); Task 3 |
| 2. `about.toml`, `about.hbs`, `cargo about generate` | Task 1, Steps 3, 5, and 6 |
| 2. Accepted licenses from `deny.toml`, MPL-2.0 for `option-ext` only | Task 1, Step 5 |
| 2. No network | Design notes; `--frozen` in every command |
| 2. One command, a pinned version at least 2 weeks old | Design notes; Task 4, Step 1; Task 3, Step 1 |
| 3. NOTICE search, once; no Notices section when none | Design notes; Task 1, Step 8 |
| 4. `include` for the `jev` package; a local `dist` build | Task 2 |
| 5. CI job `licenses` on Linux: install, write, compare, the refresh command | Task 3 |
| 6. README Licenses section, CHANGELOG, version 0.1.2 | Task 4, Steps 1 to 3 |
| 6. Tag after the merge and the person's approval | Task 4, Step 8 note |
| 7. Out of scope: system libraries, a `--licenses` flag, the plugin | No task |
