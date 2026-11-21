# gindema-guard

## Overview

Runtime anti-tamper watchdog: cheap timing and checksum checks that flip a
shared, one-way state to "polluted" on detection, so dependent values read
back a plausible decoy instead of the program crashing or the detection
itself being an observable event. Applied selectively — to specific values
worth protecting — never to a whole game loop.

This first pass is a real, working core, not a stub: `TimingWindow`
(elapsed-time checks), `Watch` (byte-region checksum checks, via
`gindema_common::checksum` — moved there after this crate's own copy
turned out to be exactly the integrity primitive other crates would
plausibly also want), `GuardState` (the shared, sticky pollution flag),
`Guarded<T>` (the decoy-on-pollution read). No unsafe code, and no use of
`gindema-ptr` yet — see "Known limits" for why that second one is a
deliberate choice, not an oversight.

## Modules

### `lib.rs`
**What it does:** crate doc (with a runnable example wiring all four pieces
together: a `Watch` on some data, a `Guarded` value, a `TimingWindow`, all
sharing one `GuardState`), module wiring, flat re-exports.

### `timing.rs`
**What it does:** `TimingWindow::start()` captures `Instant::now()`;
`elapsed()`/`elapsed_at_least(threshold)` report how long it's been. The
signal this is for: a debugger breakpoint or single-stepping through code
that should run in a few microseconds will blow past a threshold tuned for
normal execution. See "Known limits" for what this does *not* catch.

**Tests:** inline, with generous margins to avoid CI flakiness (a 2-second
threshold checked against an unslept window; a 50ms sleep checked against
only a 10ms threshold; `elapsed()` checked to only grow, never shrink, after
a short sleep).

### `state.rs`
**What it does:** `GuardState` — one `AtomicBool`, `is_polluted()`/
`pollute()`. Deliberately one-directional: there is no "clean" method.
Once anything sharing a `GuardState` trips it, it stays polluted for that
state's whole lifetime — a false positive staying polluted is a cheaper
cost than giving an attacker who found a way to trip and un-trip detection
a way to make a tampered run look clean again.

**Tests:** inline. Starts clean; `pollute()` is visible immediately and a
second call changes nothing observable (idempotent).

### `watch.rs`
**What it does:** `Watch::new(data)` records
`gindema_common::checksum(data)`; `Watch::check(data, &GuardState)`
recomputes and compares (via `gindema_common::checksums_match`), pollutes
the state on a mismatch, and — this is the part worth calling out — never
*un*-pollutes on a match, including a state some other watcher already
polluted.

**Tests:** inline. Unchanged data keeps the state clean; changed data
pollutes it; a passing check against already-polluted data does not clean
the state back up (the specific behavior the type's own docs call out).

### `guarded.rs`
**What it does:** `Guarded<T: Copy>` holds a `real` and a `decoy` value set
once at construction; `read(&GuardState)` returns `real` while clean,
`decoy` once polluted. Choosing a `decoy` that's actually plausible in
context (a believable score, not an obviously-wrong sentinel like `-1` or
`i64::MAX`) is left to the caller — this type only implements the switch.

**Tests:** inline. Reads real while clean; switches to decoy once polluted;
two `Guarded` values sharing one `GuardState` both switch together, with
one test deliberately using a decoy equal to its real value (to show the
switch happening even when it's unobservable for that particular value)
alongside one with a visibly different decoy.

## Known limits (intentional, for this pass)

- **Timing checks are coarse and single-sample.** `TimingWindow` catches a
  single long pause between two points, nothing more: it does not account
  for normal scheduling jitter under load (the tests use wide margins
  specifically to avoid being fooled by this), does not aggregate multiple
  windows, and does not distinguish "a debugger paused this" from "the OS
  scheduler paused this" — those look identical from inside the process. A
  patient attacker single-stepping *between* the two measurement points
  rather than inside the measured code entirely evades it.
- **FNV-1a is not tamper-resistant against an informed attacker**, only
  against accidental/casual changes — see `gindema-common`'s own note on
  `checksum`. An attacker who can read this crate's source (it's vendored,
  not secret) and
  who controls what they're corrupting a watched region into can construct
  a replacement with a matching checksum.
- **`GuardState`'s pollution is itself inspectable and, with enough access,
  resettable.** It's a plain `AtomicBool` behind a safe API in an
  unencrypted, unobfuscated binary at this stage (`gindema-obfuscate`
  doesn't touch this crate at all yet, and `gindema-cli` doesn't exist as a
  packaged binary yet either) — nothing here stops an attacker with a
  debugger and enough patience from locating the flag and forcing it back
  to clean. "Sticky, no clean() method" raises the bar for a casual
  reset-via-API; it is not a claim that the underlying memory can't be
  found and poked directly.
- **`gindema-ptr` is not used anywhere in this crate**, despite being the
  crate that motivated building it (see `docs/gindema-ptr.md`'s own
  "Overview": "useful here specifically for weaving integrity checks into
  memory access without an obvious branch"). The reason: whether routing a
  read through a type-erased pointer actually changes anything about the
  *compiled* output's resistance to pattern-matching is a claim about
  generated machine code, not about Rust source — and nothing in this
  sandbox can compile a release binary and inspect its disassembly to
  check that claim. Adding `unsafe` code on the strength of an unverified
  benefit would be unsafe code for its own sake, which
  `RUST_AND_CRATE_GUIDELINES.md` §3 argues against. `Guarded<T>::read`'s
  `if`/`else` is, in its current safe-Rust form, plausibly exactly the
  "obvious branch" `gindema-ptr`'s docs say this crate should avoid — this
  is flagged honestly rather than left implicit. Revisiting this needs
  either a way to actually inspect compiled output here, or accepting the
  integration on theoretical grounds alone as a conscious choice (not one
  this pass makes unilaterally).
- **Not wired into anything.** No call site in `gindema-vm`,
  `gindema-obfuscate`, or anywhere else actually constructs a `Watch` or a
  `Guarded` on real game state yet. This crate is a usable library with no
  consumer so far.

## CI and Workflows

See `.github/workflows/ci.yml` (workspace-wide, workflow_dispatch only).
`cargo test -p gindema-guard` runs 11 unit tests + 1 doctest (down from 15
— the 4 checksum tests moved to `gindema-common` along with the function
itself, not lost); `cargo clippy -p gindema-guard --all-targets -- -D
warnings` and `cargo fmt -p gindema-guard -- --check` are both clean.
Verified in the sandbox on Rust 1.100.

## Fixes and Problems

- The scaffold stub's `#![allow(unsafe_code)]` has been removed: this pass
  uses no unsafe code (an `AtomicBool` plus plain `Copy` values cover
  everything needed). Same reasoning as `gindema-vm`'s equivalent removal —
  see that crate's "Fixes and Problems". It would come back if
  `gindema-ptr` integration is later judged worth doing on the grounds
  described above.
- `checksum.rs` and its 4 tests were deleted from this crate's *tested*
  tree and moved, unchanged, into `gindema-common`; `watch.rs` now calls
  `gindema_common::checksum`/`checksums_match` instead. This finally gives
  `gindema-common`'s long-unused dependency (declared in this crate's
  `Cargo.toml` since the original scaffold, unused until now) something
  real to do, and avoids the same checksum implementation existing twice
  in the workspace now that a second real consumer exists. **One manual
  step the delivery zip can't do for you:** the zip mechanism only adds or
  overwrites files, it never removes one from the live repo — after
  applying this delta, `crates/gindema-guard/src/checksum.rs` will still
  physically exist there, just unreferenced (`lib.rs` no longer has `mod
  checksum;`, so it isn't compiled and has zero effect on the build).
  Delete that one file by hand when convenient.
