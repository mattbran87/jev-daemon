# License notices in every archive (0.1.2)

Each release archive holds `jev`, `jevd`, and `jev-tray`. Those programs contain code from crates under the MIT, BSD, ISC, Apache-2.0, Unicode, and MPL-2.0 licenses. Those licenses ask that their notices ship with the binaries. 0.1.0 and 0.1.1 ship only this project's own `LICENSE`. This spec adds the notices of every dependency. Plan 6 listed this as a task for later.

## 1. The file

`THIRD-PARTY-LICENSES.md` sits at the repository root.

- It groups the dependencies by license.
- Each group gives the full license text once, then lists every crate and version under that license.
- It covers every dependency of every release target: `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-pc-windows-msvc`, and `aarch64-apple-darwin`. A crate that only one target uses still appears. A list longer than one archive needs is allowed. A list that misses a crate is not.
- It leaves out the workspace's own crates, which `LICENSE` covers, and crates used only for tests or builds, which no archive contains.

The file is generated. Nobody edits it by hand.

## 2. How the file is made

`cargo about generate` writes the file from 2 files at the repository root:

| File | Content |
| --- | --- |
| `about.toml` | The configuration: the accepted licenses, the 4 targets, and the rules in section 1. |
| `about.hbs` | The Handlebars template that gives the Markdown layout. |

**Accepted licenses.** `about.toml` accepts the licenses in the `allow` list of `deny.toml`, and MPL-2.0 for `option-ext` only, as `deny.toml`'s exception does. `cargo deny` stays the gate for which licenses may enter the tree. `cargo about` only writes their texts.

**No network.** `cargo about` runs with no lookups to outside services, so its output depends only on `Cargo.lock` and the crate sources.

**The command** is one line, given in the README and in the CI failure message. The plan names the exact `cargo-about` version, pinned, and one that was released at least 2 weeks before the plan.

## 3. NOTICE files

Apache-2.0 asks a redistributor to also pass on the text of a crate's `NOTICE` file, when the crate has one. `cargo about` does not collect `NOTICE` files.

The plan checks the source of every dependency in the tree for a file named `NOTICE` or `NOTICE.*`, once.
- If it finds any, the template gets a closing section, **Notices**, with each file's text and the crate it came from.
- If it finds none, the template has no such section. The CI check in section 5 does not repeat this search. The README tells the person who adds a dependency to look for a `NOTICE` file.

## 4. In the archives

`dist` adds `THIRD-PARTY-LICENSES.md` to each archive through its `include` setting for the `jev` package. The installers unpack the archives, so they get the file too. The plan checks with a local `dist` build that the file is in the archive.

## 5. In CI

A new job, `licenses`, runs on Linux on each push to `main` and on each pull request.

1. It installs the pinned `cargo-about` with `cargo install --locked`.
2. It writes the file again to a temporary path.
3. It compares that file with the committed `THIRD-PARTY-LICENSES.md`. If they differ, the job fails, and its message gives the command that refreshes the file.

A dependency update that changes the notices therefore fails CI until the pull request also updates the file.

## 6. Docs and release

- The README gets a short **Licenses** section: this project is Apache-2.0, `THIRD-PARTY-LICENSES.md` holds the notices of its dependencies, and the command refreshes the file after a dependency change.
- The CHANGELOG gets a 0.1.2 entry.
- The version changes to 0.1.2 in the workspace `Cargo.toml`, in `Cargo.lock` for the workspace crates, and in `plugins/claude-code/.claude-plugin/plugin.json`. The plugin does not change, but its version follows the workspace, as the `versions` test requires.
- The tag `v0.1.2` follows the merge of the pull request, after the person approves it. The private marketplace's pin then moves to `v0.1.2`, in that repository, by its own session.

## 7. Out of scope

- Notices for GTK, AppIndicator, and other system libraries. `jev-tray` links them from the system at run time, and the archives do not contain them.
- A license notice inside each program, such as a `--licenses` flag.
- The Claude Code plugin, which has no third-party dependencies at run time.
