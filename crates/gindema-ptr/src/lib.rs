// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-ptr.md, section "lib.rs"
// ============================================================================

//! Type-erased raw pointer wrappers, vendored from mid-engine's `mid-ptr`
//! crate, which is itself ported from Bevy's `bevy_ptr` crate (MIT /
//! Apache-2.0). `no_std`, zero dependencies. See `docs/gindema-ptr.md` for the
//! provenance note and the open licensing question this vendoring carries.
//!
//! The crate gives the rest of gindema a small toolkit for holding onto a
//! pointer to *some* value without the compiler knowing what type that value
//! actually is, while still tracking the invariants a raw pointer on its own
//! throws away: whether it's still properly aligned, how long it stays valid
//! for, and who is allowed to read, write, or drop through it. That's useful
//! here specifically for weaving integrity checks into memory access without
//! an obvious branch: a type-erased accessor can route every read through the
//! same few call sites gindema-guard watches, rather than through whatever
//! shape the compiler would otherwise generate per concrete type.
//!
//! Three families of type live here:
//!
//! - [`Ptr`], [`PtrMut`], and [`OwningPtr`] are the borrow-shaped, mutable-
//!   borrow-shaped, and owning-pointer-shaped erased pointers, mirroring
//!   `&T`, `&mut T`, and `Box<T>` respectively, minus the compile-time type
//!   information.
//! - [`MovingPtr`] moves a value to a new location without ever passing it by
//!   value, and can be deconstructed into per-field `MovingPtr`s via
//!   [`deconstruct_moving_ptr`].
//! - [`ThinSlicePtr`] is a `&[T]` with the length stripped out, for callers
//!   that already know (or separately track) how long the slice is.
//!
//! [`Aligned`] and [`Unaligned`] are marker types threaded through all of the
//! above as a type parameter, so the alignment guarantee (or lack of one) a
//! given pointer carries is visible in its type rather than only in a doc
//! comment.
//!
//! Every public item here is `unsafe fn` or backed by one internally — this
//! crate exists to make the unsafety explicit and centrally reviewed, not to
//! remove it.

#![no_std]
// This crate's own `[lints] workspace = true` (Cargo.toml) pulls in the root
// workspace's `unsafe_code = "deny"` — opted back out of here, explicitly, per
// docs/RUST_AND_CRATE_GUIDELINES.md §3: this crate's entire purpose is
// wrapping raw pointers, so the exception is this one visible line rather
// than the crate silently failing to compile. Every `unsafe` block below
// carries its own `// SAFETY:` comment or `# Safety` doc section.
#![allow(unsafe_code)]

mod aligned;
mod const_non_null;
mod debug_align;
mod erased;
mod moving;
mod moving_macros;
mod thin_slice;
mod unsafe_cell;

pub use aligned::{Aligned, IsAligned, Unaligned};
pub use const_non_null::ConstNonNull;
pub use erased::{OwningPtr, Ptr, PtrMut};
pub use moving::MovingPtr;
pub use thin_slice::ThinSlicePtr;
pub use unsafe_cell::UnsafeCellDeref;
