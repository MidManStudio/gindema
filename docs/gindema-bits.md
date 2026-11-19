# gindema-bits

## Overview

Bit-level stream I/O for `gindema-vm`'s bytecode format: a `BitWriter`/
`BitReader` pair that packs and unpacks arbitrary-width (1..=64 bit) unsigned
fields into a plain byte buffer, most-significant-bit first, with no
byte-alignment required between fields. This is what lets gindema-vm pack a
6-bit opcode next to a differently-sized operand with no wasted padding
between instructions, and lets `BitReader::seek_to_bit` jump straight to an
absolute bit offset for jump/branch instructions instead of re-decoding
everything in between.

## Plan change from the original scaffold note (important — read before touching this crate)

The scaffold note for this crate said it would vendor "bit-level stream I/O
from mbfa's `bitreader.rs`/`bitwriter.rs`." Having now actually opened both
files in `Mid-D-Man/mbfa`: **they are not generic bit I/O.** They're mbfa's
own LZ-style token codec — encoding/decoding a `Token` enum (`Lit`/`Backref`/
`RepRef`/`End`) with mbfa-specific opcode constants (`OPCODE_BACKREF_VAL`,
`BYTE_BITS`, `REPREF_SLOT_BITS`, ...) — built on top of the **published**
`bitstream_io` crate (`bitstream_io = "2.4"` in mbfa's own `Cargo.toml`), using
`std::io::Result` and `std::io::Cursor`, not `no_std`. There is nothing
generic in those two files to vendor; vendoring them verbatim would mean
pulling in mbfa's own compression-token semantics for no reason, since
gindema-vm has a completely different instruction set.

Two real options existed once this was found: depend on `bitstream_io`
directly (it's on crates.io, so "not published" never applied to the actual
bit-I/O layer, only to mbfa the crate), or write gindema-vm's own bit
reader/writer from scratch. This crate does the latter: a small,
zero-dependency implementation, read/write shaped the same way mbfa's
`BitReader`/`BitWriter` calls are (`read_bits(width)`, `write_bits(value,
width)`, `byte_align()`) and keeping the same most-significant-bit-first
convention mbfa also uses, but with its own `BitsError` type instead of
`std::io::Result`, 1..=64-bit widths instead of a fixed `u32`, and
`seek_to_bit` added (mbfa's own reader never needs to seek — it's a pure
forward decode of a token stream — but gindema-vm's jump instructions do).
Not a vendoring in the literal sense anymore; the module names and shape
still echo where the idea came from.

## Modules

### `lib.rs`
**What it does:** module wiring and the flat `pub use` re-exports
(`gindema_bits::{BitReader, BitWriter, BitsError}`).

### `error.rs`
**What it does:** `BitsError` — `InvalidWidth(u32)` (width is 0 or > 64),
`ValueTooWide { width }` (a value to write has bits set above the requested
width — rejected rather than silently masked), `EndOfStream` (read past the
end), `SeekOutOfRange { pos, len }`. Manual `Display` + `std::error::Error`,
no `thiserror`, matching `mirage-core::PatternError`'s style.

### `writer.rs`
**What it does:** `BitWriter` — an internal `Vec<u8>` plus a partially-filled
next byte (`cur`, `cur_bits`). `write_bit`/`write_bits` (MSB-first, one bit at
a time internally — clear and easy to verify correct; a batched/table-driven
version is a legitimate future optimization, not done here since nothing yet
shows this needs to be fast), `byte_align`, `bit_len` (so a compiler can
record a jump target's bit offset before the destination is written),
`into_bytes` (byte-aligns, then returns the buffer).

**Tests:** inline. Covers width validation (0, 65, and a value that overflows
its width all rejected), `byte_align` padding and its no-op case, `bit_len`
tracking position before alignment, and `into_bytes` aligning on consumption.

### `reader.rs`
**What it does:** `BitReader<'a>` — wraps a `&'a [u8]` plus an absolute
`bit_pos`, rather than a generic `Read`, specifically so it can
`seek_to_bit`. `read_bit`/`read_bits` (the exact inverse of the writer's,
same width in, same value out), `byte_align`, `bit_pos`/`bits_remaining`,
`seek_to_bit`.

**Design note:** on an `EndOfStream` error mid-field, the reader's position is
left wherever it stopped rather than rewound to before the field started —
documented on `read_bits` rather than silently assumed, since a caller that
wants to retry from a known-good point needs to save `bit_pos()` first.

**Tests:** inline (uses `BitWriter` from the sibling module to build fixtures
rather than hand-writing raw bytes). Covers round-tripping a mix of widths
(1, 3, 8, 16, 32, 64, including `u64::MAX`), width validation, end-of-stream
on both `read_bit` and `read_bits`, `byte_align` skipping to the next
boundary, `seek_to_bit` jumping to a recorded target and reading correctly
from there, seeking past the end being rejected (seeking exactly to the end
is allowed; a subsequent read from there is what reports `EndOfStream`), and
`bits_remaining` tracking position.

## CI and Workflows

See `.github/workflows/ci.yml` (workspace-wide, workflow_dispatch only).
`cargo test -p gindema-bits` runs 13 unit tests; `cargo clippy -p
gindema-bits --all-targets -- -D warnings` and `cargo fmt -p gindema-bits --
--check` are both clean. Verified in the sandbox on Rust 1.100.

## Fixes and Problems

- The scaffold note describing this crate as a vendoring target was wrong
  about what mbfa's `bitreader.rs`/`bitwriter.rs` actually contain — see the
  "Plan change" section above. Not a bug exactly, but worth flagging loudly
  since the original plan is still what's written in the root README's
  workspace-layout list ("vendored bit-level I/O") and in `gindema-vm`'s own
  scaffold doc; both should be read with this correction in mind until they're
  updated.
- No known issues with the implementation itself at this point — bit-by-bit
  read/write is simple enough that correctness was straightforward to get
  right the first time; see "Modules" above for the one deliberate
  performance trade-off (not batched) and the one deliberate behavior choice
  (position not rewound on a mid-field `EndOfStream`).
