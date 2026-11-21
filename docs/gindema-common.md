# gindema-common

## Overview

Shared error types, traits, and small utilities used across the gindema
workspace, by both the protection-core crates and the mirage video-poisoning
crates. This first pass has exactly one thing in it: `checksum`/
`checksums_match` (FNV-1a, 64-bit), moved here from `gindema-guard` once a
second plausible consumer made "every crate rolls its own" the wrong call.
See "Known limits" for why shared error types/traits — named in this
crate's own `description` in `Cargo.toml` — still don't exist here.

## Modules

### `lib.rs`
**What it does:** crate doc, module wiring, flat re-export
(`gindema_common::{checksum, checksums_match}`).

### `checksum.rs`
**What it does:** `checksum(&[u8]) -> u64` (FNV-1a) and
`checksums_match(u64, u64) -> bool` (XOR-based comparison, not a plain
`==`, for whatever small amount of timing-side-channel resistance that's
worth — documented as best-effort, not verified against compiled output).
Identical content to what used to live in `gindema-guard`'s own
`checksum.rs`, which was deleted in the same change — see `docs/gindema-
guard.md`'s "Fixes and Problems" for that side of the move. Explicitly
*not* a cryptographic hash; see the function's own doc comment for what
that does and doesn't mean for a caller relying on it.

**Tests:** inline. Same bytes always checksum the same; different bytes
checksum differently; empty input doesn't panic and equals the FNV offset
basis; `checksums_match` agrees with plain equality.

## Known limits (intentional, for this pass)

- **No shared error types or traits yet**, despite this crate's own
  `Cargo.toml` description naming them. Every other crate in this
  workspace (`gindema-vm`, `gindema-obfuscate`, `gindema-guard`,
  `gindema-cli`) defines its own local error enum, deliberately — see each
  of their own docs — rather than depending on a shared one, mainly
  because a shared top-level error type would need to wrap each of
  *their* errors, which would make `gindema-common` depend on crates
  higher up the stack than it, inverting the dependency direction a
  "common" leaf crate is supposed to have. A shared error type, if one
  ever earns its place, would need to be something those crates'
  `From` impls point *at* without `gindema-common` itself knowing about
  any of them — not attempted in this pass.
- Every crate that's declared a dependency on `gindema-common` so far
  (`gindema-vm`, `gindema-obfuscate`, `gindema-cli`) still doesn't use it
  for anything — `cargo` reports it as an unused dependency for all three.
  Only `gindema-guard` actually uses it, for `checksum`. Left as-is rather
  than removed from the others' `Cargo.toml`s, matching this workspace's
  existing convention of not pruning a scaffolded-but-not-yet-needed
  dependency just because it's currently unused.

## CI and Workflows

See `.github/workflows/ci.yml` (workspace-wide, workflow_dispatch only).
`cargo test -p gindema-common` runs 4 unit tests; `cargo clippy -p
gindema-common --all-targets -- -D warnings` and `cargo fmt -p
gindema-common -- --check` are both clean. Verified in the sandbox on Rust
1.100.

## Fixes and Problems

- None from this pass — the content moved over from `gindema-guard`
  unchanged (same function bodies, same tests), so there was nothing new
  to get wrong.
