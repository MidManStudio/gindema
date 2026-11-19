# gindema-ptr

## Overview

Type-erased raw pointer wrappers vendored from mid-engine's `mid-ptr` crate,
which is itself a close port of Bevy's `bevy_ptr` crate (MIT/Apache-2.0).
`no_std`, zero dependencies. Gives the rest of gindema a small toolkit for
holding a pointer to a value without the compiler knowing its type —
`Ptr`/`PtrMut`/`OwningPtr` mirror `&T`/`&mut T`/`Box<T>` with the type erased,
`MovingPtr` moves a value to a new location (and can be deconstructed
field-by-field) without passing it by value, and `ThinSlicePtr` is a `&[T]`
with the length stripped out for callers that track it separately.

Within gindema, this is the primitive gindema-guard and gindema-obfuscate are
expected to build on: routing a tamper-sensitive read or write through a
type-erased accessor means the access doesn't have an obvious, concrete-typed
shape for a decompiler to pattern-match against. Neither of those crates
consumes it yet — gindema-ptr is a leaf dependency with no consumers in the
workspace so far; this pass vendored and verified the primitive itself.

## Provenance and licensing (carried over from mid-engine, open)

This is a close port, not a from-scratch design — the type layouts, safety
invariants, and most of the doc comments come directly from mid-engine's
`mid-ptr` (`crates/mid-ptr/src/`), which itself transcribes `bevy_ptr` nearly
line-for-line. Everything in mid-ptr's 8 modules was carried over unchanged
in substance: `Aligned`/`Unaligned`/`IsAligned`, `ConstNonNull`,
`DebugEnsureAligned`, `Ptr`/`PtrMut`/`OwningPtr`, `MovingPtr` with its full
field-deconstruction machinery, the `move_as_ptr!`/`deconstruct_moving_ptr!`
macros, `ThinSlicePtr`, and `UnsafeCellDeref`.

**`bevy_ptr` is dual MIT/Apache-2.0. mid-engine's own `docs/mid-ptr.md` flags
this as an unresolved question there — mid-ptr carries no license file or
upstream attribution/notice of its own, since mid-engine's workspace hasn't
made a license decision project-wide.** gindema is in a different position:
its own `LICENSE` is already proprietary/all-rights-reserved, and the compiled
CLI it produces is distributed publicly — so a substantial derivative of
MIT/Apache-2.0 code sitting inside it is a real question, not a deferred one.
**This has not been resolved in this pass; flagging it rather than picking an
answer.** MIT and Apache-2.0 both permit this kind of closed/commercial use,
but both also carry attribution obligations (reproducing the copyright
notice and license text; Apache-2.0 additionally expects a NOTICE file to
travel with binary distributions). Before `gindema-cli` ships publicly, this
crate specifically needs either a `THIRD_PARTY_NOTICES`-style file carrying
`bevy_ptr`'s copyright and license text, or legal review of whether the
`#![no_std]`, non-public (not re-exported past gindema-guard/gindema-obfuscate,
once those exist) nature of this crate changes that calculus. Not a call this
pass makes on its own.

## Modules

### `lib.rs`
**What it does:** Crate root — `#![no_std]`, the `#![allow(unsafe_code)]`
opt-out (per `RUST_AND_CRATE_GUIDELINES.md` §3), module wiring, and the flat
`pub use` re-exports that keep the public API surface identical to upstream's
single-file layout (`gindema_ptr::Ptr`, `gindema_ptr::MovingPtr`, and so on).

### `aligned.rs`
**What it does:** `Aligned`/`Unaligned` marker types and the sealed `IsAligned`
trait that dispatches `read_ptr`/`copy_nonoverlapping`/`drop_in_place` to either
the aligned or byte-wise-unaligned version, so every other type parameterized
over `A: IsAligned` only has to write the aligned/unaligned split once.

**Tests:** inline `#[cfg(test)] mod tests`. Covers aligned and deliberately
misaligned round-trip reads, and an aligned `copy_nonoverlapping` call.

### `const_non_null.rs`
**What it does:** `ConstNonNull<T>` — the `*const T` counterpart to
`NonNull<T>`'s `*mut T`; only allows conversion to read-only borrows.

**Tests:** inline. Covers null rejection, `as_ref` round-tripping, and the
`From<&T>`/`From<&mut T>` impls.

### `debug_align.rs`
**What it does:** `pub(crate)` `DebugEnsureAligned` trait — asserts a `*mut T` is
properly aligned for `T` in debug builds (skipped under miri, which already
checks this) and is a no-op in release. Every erased-pointer deref/read/drop-as
call routes through it first.

### `erased.rs`
**What it does:** `Ptr`, `PtrMut`, `OwningPtr` — the borrow-shaped,
mutable-borrow-shaped, and owning-pointer-shaped type-erased pointers. Shares a
local `impl_ptr!` macro for the common `byte_offset`/`byte_add`/`Debug`/`Pointer`
impls across all three.

**Tests:** inline. Covers `Ptr`/`PtrMut` deref round-trips, `OwningPtr::make` +
`read`, `drop_as` running `Drop` exactly once, and `to_unaligned` +
`read_unaligned`.

### `moving.rs`
**What it does:** `MovingPtr<'a, T, A>` — moves a value to a new location without
passing it by value. Full method surface carried over: `from_value`, `read`,
`write_to`, `assign_to` (via an internal drop-guard to avoid a double memcpy),
`move_field`/`move_maybe_uninit_field` (the primitives
`deconstruct_moving_ptr!` builds on), `partial_move`, `assume_init`,
`to_unaligned`, plus `Deref`/`DerefMut` (aligned only), `Drop`, and the
`From<MovingPtr> for OwningPtr` / `TryFrom<Unaligned> for Aligned` conversions.

**Fix during this port:** upstream's file has a stray duplicate closing brace
after the last test function (the `mod tests { ... }` block closes twice).
Harmless syntactically but not what the module structure intends — removed
here rather than carried over.

**Tests:** inline. Covers `read` skipping the pointee's `Drop`, `assign_to`
dropping the old value exactly once, and `to_unaligned` + `TryFrom` recovering
`Aligned`.

### `moving_macros.rs`
**What it does:** `move_as_ptr!`, `get_pattern!` (macro-internal helper), and
`deconstruct_moving_ptr!` — the three `#[macro_export]` macros that make
`MovingPtr` usable without hand-writing raw pointer arithmetic at every call
site. `deconstruct_moving_ptr!` supports structs, tuples (via the `tuple`
identifier), and projecting through `MaybeUninit::<_>`.

**MSRV note:** the field-projection arms use `&raw mut`/`&raw const` (RFC
2582, stable since Rust 1.82). mid-engine's own docs flag this as a wall
against its 1.75 floor; it's a non-issue here since gindema's workspace floor
is already 1.100 (see the root README's toolchain history).

**Doctest note:** the three worked examples in this file's doc comments are
real doctests (`cargo test` runs them) — updated from upstream's
`mid_ptr::...` paths to `gindema_ptr::...` so they compile against this
crate's own name instead of the one they were copied from.

### `thin_slice.rs`
**What it does:** `ThinSlicePtr<'a, T>` — a `&'a [T]` with the length cut out.
Debug builds still track the length internally and bounds-check
`get_unchecked`; release builds don't. A specialized `impl` block for
`ThinSlicePtr<'a, UnsafeCell<T>>` adds mutable-slice access and a `cast` back to
the plain `T` form.

**Tests:** inline. Covers `get_unchecked` across a full slice, `as_slice_unchecked`
reconstructing the original slice, `slice_unchecked` on a subrange, and the
`UnsafeCell` specialization's mutable-slice + `cast` round-trip. One doctest
in the type's own doc comment, also updated to `gindema_ptr::ThinSlicePtr`.

### `unsafe_cell.rs`
**What it does:** `UnsafeCellDeref` — a sealed extension trait adding
`deref`/`deref_mut`/`read` to `&UnsafeCell<T>`, for call sites that have already
established the aliasing invariants some other way.

**Tests:** inline. Covers `deref`/`deref_mut` reaching the same cell and `read`
returning a copy without disturbing it.

## CI and Workflows

See `.github/workflows/ci.yml` (workspace-wide, workflow_dispatch only).
`cargo test -p gindema-ptr` runs 20 unit tests + 4 doctests; `cargo clippy -p
gindema-ptr --all-targets -- -D warnings` and `cargo fmt -p gindema-ptr --
--check` are both clean. Verified in the sandbox on Rust 1.100.

## Fixes and Problems

- clippy 1.100 flags `needless_lifetimes` on `unsafe_cell.rs`'s
  `impl<'a, T> SealedUnsafeCell for &'a UnsafeCell<T> {}` — not caught by
  mid-engine's own CI (different clippy version at the time, or this lint's
  behavior changed since). Elided here: `impl<T> SealedUnsafeCell for
  &UnsafeCell<T> {}`.
- rustfmt 1.100 wants to wrap one long line in `thin_slice.rs`'s
  `unsafe_cell_slice_mut_and_cast_round_trip` test (an array literal past the
  line-length limit) that upstream's own copy left unwrapped. Reformatted.
- `moving.rs` had a stray duplicate closing brace at end-of-file (see that
  module's note above) — removed.
