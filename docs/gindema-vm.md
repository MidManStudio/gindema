# gindema-vm

## Overview

Custom stack-based bytecode format and interpreter — the core virtual
machine `gindema-obfuscate` is meant to compile into and `gindema-guard` is
meant to watch over running. This pass implements a real, tested baseline:
a fixed instruction set, a two-pass label-resolving program builder, and a
safe interpreter loop. It deliberately does **not** yet implement any of the
obfuscation (per-build opcode remap, control-flow flattening, state-dependent
decoding, honeypots) — that is `gindema-obfuscate`'s job, layered on top of
this fixed baseline, not this crate's.

Depends on `gindema-bits` for the bit-packed instruction stream (a 6-bit
opcode followed by a variable-width operand, byte-unaligned between
instructions — denser and less immediately recognizable to a generic
disassembler than a byte-aligned format). Does not yet depend on
`gindema-ptr`: nothing here needs a type-erased accessor yet, since there's
no anti-tamper guard consuming the interpreter's memory access patterns yet.
That integration is future work for `gindema-guard`.

**Per-build opcode remap, mechanism vs. policy:** this crate now owns the
*mechanism* — `OpcodeMap` and `remap_program` (see those modules below) can
losslessly re-encode an already-built `Program` under a different raw
opcode numbering, with every bit offset in the stream staying valid. It
still has no opinion on the *policy* — generating the actual random
permutation for a given build is `gindema-obfuscate`'s job, which calls
into this mechanism rather than reimplementing it. Control-flow
flattening, state-dependent decoding, and honeypot/dead-end injection
remain entirely `gindema-obfuscate`'s, with no corresponding support here
yet.

## Instruction set

17 opcodes, 0-16 (see `opcode.rs` for the exact encoding and bit widths):
`Halt`, `Pop`, `Dup`, `Swap`, `Add`, `Sub`, `Mul`, `Div`, `Neg`, `Not`, `Eq`,
`Lt`, `LoadConst`, `LoadLocal`, `StoreLocal`, `Jump`, `JumpIfFalse`. Invented
for this project specifically — not modeled on any real ISA's numbering or
mnemonics, so there's no shared alphabet with JVM bytecode, CPython bytecode,
WASM, x86, or anything else a decompiler's heuristics might be trained on.

Values are `Int(i64)`, `Float(f64)`, or `Bool(bool)` — deliberately small for
this pass (see "Known limits" below).

## Modules

### `lib.rs`
**What it does:** crate doc (with a runnable `(2 + 3) * 4` example), module
wiring, flat re-exports.

### `error.rs`
**What it does:** `VmError` — `StackUnderflow`, `TypeMismatch`,
`DivisionByZero`, `ArithmeticOverflow`, `InvalidOpcode(u8)`,
`InvalidConstIndex(u16)`, `InvalidLocalSlot(u8)`, `StepLimitExceeded`, and
`Bits(BitsError)` for a malformed/truncated bytecode stream (as opposed to a
well-formed stream with a semantically invalid operand, which gets its own
specific variant). `From<BitsError> for VmError` so the interpreter loop can
use `?` directly on `BitReader` calls.

### `value.rs`
**What it does:** `Value` and its fallible operations: `try_add`, `try_sub`,
`try_mul`, `try_div`, `try_neg`, `try_not`, `try_lt` (all `Result<Value,
VmError>`), and `eq_value`/`as_bool` (infallible). Arithmetic requires both
operands to be the same numeric kind (`Int`+`Int` or `Float`+`Float`) —
`TypeMismatch` otherwise, no implicit coercion. `Int` overflow is
`ArithmeticOverflow` (via `checked_*`), not silent wraparound. `Int` division
by zero is `DivisionByZero`; `Float` division by zero follows IEEE 754
(`inf`/`NaN`) like `f64`'s own `/`. `eq_value` treats cross-kind comparison
as simply not equal rather than an error (so a program can safely compare
values of statically-unknown kind); `try_lt` has no such default and errors
on `Bool` or mixed `Int`/`Float`.

**Naming:** originally named `add`/`sub`/`mul`/`div`/`neg`/`not`/`lt` to
mirror the opcodes; clippy's `should_implement_trait` flagged all seven as
easily confused with `std::ops::{Add,Sub,Mul,Div,Neg,Not}`/`PartialOrd::lt`
even though the return type (`Result<Value, VmError>`, not an infallible
`Self`/`bool`) means they aren't real trait-method collisions. Renamed to
`try_*` — idiomatic for a fallible operation and removes the ambiguity
outright rather than suppressing the lint.

**Tests:** inline. Covers same-kind arithmetic, type-mismatch rejection,
int-vs-float division-by-zero behavior, overflow reporting, cross-kind
equality's "false, not an error" rule, `try_lt`'s stricter rejection, and
`try_not`.

### `opcode.rs`
**What it does:** `OpCode` (the 17 variants above, `#[repr(u8)]`), the bit
width constants (`OPCODE_WIDTH = 6`, `CONST_INDEX_WIDTH = 16`,
`LOCAL_SLOT_WIDTH = 8`, `JUMP_TARGET_WIDTH = 32`), `to_u8`/`from_u8` for the
numeric encoding, and `operand_width` (0 for opcodes with none).

**Tests:** inline. Every defined byte 0-16 round-trips through
`from_u8`/`to_u8`; undefined bytes (17, 63, 255) don't decode; every opcode's
numeric value fits `OPCODE_WIDTH` bits; `operand_width` matches the declared
constants per opcode.

### `opcode_map.rs`
**What it does:** `OpcodeMap` — maps the raw 6-bit value on the wire to and
from the logical `OpCode` a given `Program` was encoded with.
`OpcodeMap::identity()` (what `ProgramBuilder::build` always produces) maps
every opcode to its own canonical `to_u8()` value.
`OpcodeMap::from_permutation([u8; OPCODE_COUNT])` builds a non-identity map
from a caller-supplied assignment — validated (`RawValueOutOfRange`,
`DuplicateRawValue`) rather than trusted. `OpCode::ALL` (added to
`opcode.rs` alongside this) is what both builders iterate over, so adding a
future opcode only means updating that one array.

This crate only validates and *applies* a permutation — it has no opinion
on how one is generated. That's deliberately left to whoever wants a
non-identity map (`gindema-obfuscate`'s per-build remap is the first real
caller); keeping randomness-generation out of `gindema-vm` means this
crate's own tests stay fully deterministic.

**Tests:** inline. Identity round-trips every opcode through its own
value; a scrambled (reversed, spread-out) permutation round-trips the same
way; out-of-range and duplicate raw values are both rejected; an
unassigned raw value decodes to `None` rather than panicking.

### `remap.rs`
**What it does:** `remap_program(&Program, &OpcodeMap) -> Result<Program,
VmError>` — re-encodes a program's instruction stream under a different
`OpcodeMap`, changing nothing about its logic, constants, or local count.

**Why this is a lossless, pure transcoding and not a decode-and-rebuild:**
an opcode's operand *width* is fixed by the opcode itself
(`OpCode::operand_width`) and does not depend on which `OpcodeMap` is in
use — only which raw 6-bit value means that opcode does. So every
instruction occupies exactly the same number of bits, at exactly the same
position in the stream, before and after remapping. That means every
`Jump`/`JumpIfFalse` target (an absolute bit offset) stays valid completely
unchanged — there is nothing to recompute, unlike `ProgramBuilder::build`'s
own two-pass label resolution. The function just walks the stream once:
decode each opcode field under the *old* map, re-encode under the *new*
one, copy the operand bits through unchanged, stop at `Halt`.

**Tests:** inline. A real program (with a loop, so jump targets are
actually exercised) remapped under a deliberately scattered, reversed
permutation: same byte length, different bytes, and — run through `Vm`
both ways — identical result. Remapping with the same map the program
already uses is a byte-for-byte no-op. Remapping to a scrambled map and
then back to identity recovers the exact original bytes.

### `program.rs`
**What it does:** `Instr` (the builder's intermediate instruction
representation — jump operands are `LabelId`s here, not yet bit offsets),
`LabelId`, `BuildError`, `Program` (the assembled result: bit-packed `code`,
`constants`, `local_count`, and — added alongside `opcode_map.rs` — the
`OpcodeMap` the stream was encoded with, plus `code_bytes()`/`opcode_map()`
accessors for passes like `remap_program` that transform an already-built
program), and `ProgramBuilder`.

**Design — two-pass label resolution instead of back-patching:** a jump
target isn't always known when the jump instruction is emitted (a forward
`if`/`else` branch, a loop's exit). The obvious fix is back-patching: write a
placeholder operand, remember its bit position, and overwrite it once the
real target is known. `gindema-bits::BitWriter` doesn't support overwriting
already-written bits (append-only, by design — see `gindema-bits.md`), so
this crate does something simpler instead: **every instruction's encoded
width is fixed by its opcode alone** (`OpCode::operand_width`), independent
of what the operand's *value* turns out to be. That means a single forward
pass over the instruction list — before any bits are written — already
determines every instruction's final bit offset. `ProgramBuilder::build`
uses that pass to resolve every `LabelId` to a concrete bit offset, then
encodes the whole stream in a second pass with every jump target already
known. No back-patching, no mutable-bit-buffer requirement on
`gindema-bits` at all.

`place_label` records the *instruction index* the label points to (not yet a
bit offset — that's only computed in `build`), so a label can be placed
before its target instruction is known to be the last one (e.g. a label
placed right before `build()` is called, with nothing emitted after it,
correctly resolves to "the end of the program").

**`build` takes `&mut self`, not `self`:** the natural call pattern is a
chain ending in `.build()`, e.g. `ProgramBuilder::new(0).halt().build()`. The
emitter methods (`halt`, `add`, `load_const`, ...) return `&mut Self` for
chaining, so by the time `.build()` is reached, only a `&mut Self` is in
hand — not an owned `Self`. `build` takes the instruction list, label table,
and constant pool out of `self` via `std::mem::take` rather than cloning
them, so the composability costs nothing extra; calling `build` a second
time on the same (now-emptied) builder just produces an empty program
instead of erroring, which is harmless but not a documented, relied-on
behavior.

**No assembly syntax:** a program is built by calling builder methods
directly (`b.load_const(idx).add().halt()`), not by parsing a textual
instruction listing. Enough to drive and test the interpreter; a real
assembler (or a higher-level language gindema-obfuscate compiles from) is
future work.

**Tests:** inline. An empty (`halt`-only) program's byte length; an
unplaced label failing `build` with `UnresolvedLabel`; a label placed at the
very end still resolving; the 16-bit constant-pool index limit being
enforced (`TooManyConstants`).

### `vm.rs`
**What it does:** `Vm` (holds just the value stack between runs — a fresh,
zeroed local-slot array is allocated per `run` call, sized from the
`Program`), `DEFAULT_STEP_LIMIT` (10,000,000), `run`/`run_with_step_limit`.
The interpreter loop decodes one instruction at a time from a `BitReader`
over the program's `code`, dispatches on `OpCode`, and for `Jump`/
`JumpIfFalse` calls `BitReader::seek_to_bit` directly to the (already fully
resolved, by `build`) absolute target — no re-decoding of skipped
instructions.

**Step limit:** a safety net against a malformed or adversarial program's
unbounded loop hanging whatever runs it (a test suite, CI, eventually a real
caller) — not an architectural cap on legitimate programs. `run` uses
`DEFAULT_STEP_LIMIT`; `run_with_step_limit` takes an explicit bound (the test
suite uses a small one to prove the mechanism works without waiting out 10
million iterations).

**Tests:** inline. `(2 + 3) * 4` end-to-end; a local variable stored then
loaded twice and added (`5 + 5 = 10`); a forward conditional jump that skips
a dead branch (`if (3 == 4) { 1 } else { 2 }` → `2`); a backward jump
implementing a real counting loop (`while local[0] < 5 { local[0] += 1 }` →
`5`) — this is the test that actually exercises a loop, not just a single
forward branch; stack underflow reported as an error rather than panicking;
an intentionally infinite loop (`jump` to its own label) hitting
`StepLimitExceeded` rather than hanging the test; invalid constant-pool and
local-slot indices both reported with the offending index in the error.

## Known limits (intentional, for this pass)

- No functions or calls — no call stack, no `Call`/`Return` opcodes. Every
  program is one flat instruction stream.
- No strings, arrays, or heap objects — `Value` is `Int`/`Float`/`Bool` only.
- No assembly syntax or disassembler — programs are built and inspected only
  through `ProgramBuilder`'s own API.
- `ProgramBuilder::build` always produces the identity `OpcodeMap` — this
  crate never randomizes a program's own numbering on its own. The
  mechanism to re-encode under a different map exists (`remap_program`),
  but choosing or generating that map is entirely `gindema-obfuscate`'s
  job. Control-flow flattening, state-dependent decoding, and honeypot/
  dead-end injection have no support here at all yet.
- `gindema-ptr` is not yet used anywhere in this crate (see "Overview").
- The interpreter's bit-by-bit decode (inherited from `gindema-bits`, see
  that crate's own "Known limits") is not optimized; nothing here has shown
  it needs to be yet.

## CI and Workflows

See `.github/workflows/ci.yml` (workspace-wide, workflow_dispatch only).
`cargo test -p gindema-vm` runs 31 unit tests + 1 doctest; `cargo clippy -p
gindema-vm --all-targets -- -D warnings` and `cargo fmt -p gindema-vm --
--check` are both clean. Verified in the sandbox on Rust 1.100.

## Fixes and Problems

- `value.rs`'s arithmetic methods were originally named to mirror the
  opcodes (`add`, `sub`, `mul`, `div`, `neg`, `not`, `lt`) and clippy's
  `should_implement_trait` flagged all seven — renamed to `try_*`; see that
  module's note above for why renaming rather than suppressing the lint.
- `ProgramBuilder::build` was originally `fn build(self) -> ...`
  (consuming), which doesn't compose with the chained `&mut Self`-returning
  emitter methods (`ProgramBuilder::new(0).halt().build()` failed to compile
  — only a `&mut Self` is available by the time `build` is reached). Changed
  to `fn build(&mut self) -> ...`, taking its data out via `std::mem::take`
  instead of consuming `self` by value.
- `Program` needed `#[derive(PartialEq)]` added (originally just `Debug,
  Clone`) so `assert_eq!(builder.build(), Err(...))` compiles in tests —
  caught by the compiler (E0369), not something that would have silently
  passed.
- The `#![allow(unsafe_code)]` the original scaffold stub carried has been
  removed: this pass uses no unsafe code at all (plain `Vec`-backed stack and
  locals), so the escape hatch would have been unused and, per
  `RUST_AND_CRATE_GUIDELINES.md` §3 ("a crate that genuinely needs unsafe
  opts out explicitly"), claiming it without using it is the wrong default.
  It'll come back if a future fast-interpreter pass (e.g. unchecked stack
  access) genuinely needs it — not before.
