# Duplicates in jevd's pending file

**Date:** 2026-11-12
**Status:** built

This change fixes 2 defects in how jevd loads `pending.json` when 2 entries share a code or a key. Both need a hand-edited or damaged file, because jevd itself never writes 2 entries with the same code or key. No wire format, file format, or exit code changes.

## The deadline

Each entry in `pending.json` has a `deadline`: the Unix time, in seconds, when the waiting request expires. When a client asks to pair, jevd sets it to the current time plus `PENDING_EXPIRY` (600 seconds). Every request gets the same 10 minutes, so a later deadline means a newer request.

One limit: if the clock is set back between 2 requests, the newer request can get the earlier deadline. No saved field records the true arrival order, so the load uses the deadline. If 2 duplicates have the same deadline, the one that comes first in the file stays.

## Defects

`Pairings::load_pending` in `crates/jevd/src/pairing.rs` removes duplicates at 2 points, and both are in the wrong place:

1. **The code check runs before the sort.** The load reads the file in order and keeps the first entry with each code. So of 2 entries with the same code, the first one in the file stays, not the newest one.
2. **The key check runs after the cut.** The load sorts the entries, keeps `MAX_PENDING` (16), and only then skips an entry whose key is already in `self.pending`. A duplicate `id_sha256` takes up 1 of the 16 places and then does not load. So the load can keep fewer than 16 entries, even when more valid entries exist.

## Fix

The load works in 3 steps:

1. Keep every entry that is valid and not expired, with no duplicate check.
2. Sort by the saved deadline, the latest first. The current code does this already.
3. Walk that order. Skip an entry whose code is already taken. Skip a socket entry whose key is already taken. Stop when 16 entries load.

So for each duplicate code or key, the entry with the latest deadline stays, and the 16 places go only to entries that load.

A `cli` entry gets a new random request ID at the load, and its key is the hash of that ID. So only a socket entry needs the key check. The check also covers a key that is already in `self.pending`, as the current code does.

## Tests

- 2 entries with the same code and different deadlines. The one with the later deadline stays, and it comes second in the file.
- 17 valid socket entries, 2 of which share an `id_sha256`. 16 entries load, not 15. Of the 2 that share a key, the one with the later deadline stays.
- The existing test `a_load_keeps_the_16_latest_valid_entries` changes its expected result. Its `copy` entry shares the code and the key of `app17` and has a later deadline, so `copy` now loads in place of `app17`.

## Docs

One more line under **Unreleased** in `CHANGELOG.md`.

## Out of scope

- A new field that records the true arrival time.
- Any other check on `pending.json` fields.
