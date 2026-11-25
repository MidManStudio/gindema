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
into this mechanism rather than reimplementing it.

**State-dependent decoding is the same split, but this time the mechanism
itself needed real work here, not just a pass-through type.**
`interleave_fluff` (`fluff.rs`) interleaves pseudo-random junk bits before
every instruction, in an amount derived purely from the seed and that
instruction's own bit position — never stored explicitly in the stream.
This is the one piece of that pass that couldn't live entirely in
`gindema-obfuscate` the way honeypot injection does: moving instructions
later in the stream invalidates every absolute jump target after the
first fluff region, so something with byte-and-bit-level knowledge of the
format has to recompute them, the same way `remap_program` already needed
that knowledge for a simpler reason. `gindema-obfuscate`'s policy wrapper
over this is correspondingly thin — there's no permutation to choose, just
a seed to pass through.

Control-flow flattening's *mechanism* is `decode_program` (`decode.rs`) —
reading an already-built program back into a structured instruction list,
rather than the raw bit stream — but the actual flattening logic (finding
basic blocks, building the dispatcher loop) is still entirely
`gindema-obfuscate`'s; `decode_program` is useful on its own regardless of
flattening (see its own section below). Honeypot/dead-end injection needed
no corresponding mechanism here at all — it operates entirely through
`ProgramBuilder`'s existing public API, before a program is even built.

**A textual assembly language now exists** (`asm.rs`, `assemble`) — see
that module's own doc comment for the syntax. This closes what was,
through the previous several sessions, the single biggest gap flagged in
both this doc and `docs/gindema-cli.md`: `ProgramBuilder`'s Rust API was
the only way to construct a `Program` at all, which is why `gindema-cli`'s
`demo` subcommand had to hardcode one fixed program. `gindema-cli`'s new
`assemble` subcommand is a thin wrapper around this.

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

### `asm.rs`
**What it does:** `assemble(&str) -> Result<Program, AsmError>` and
`AsmError`. A minimal textual assembly language: one instruction per line,
`;` line comments, an optional `.locals N` directive (defaults to `0`),
`name:` labels, and the same 17 mnemonics as `ProgramBuilder`'s emitter
methods (`halt`, `pop`, `dup`, `swap`, `add`, `sub`, `mul`, `div`, `neg`,
`not`, `eq`, `lt`, `load_const <literal>`, `load_local <slot>`,
`store_local <slot>`, `jump <label>`, `jump_if_false <label>`) — see the
module's own doc comment (reproduced in the doctest below) for the full
syntax.

**Two-pass label resolution, text-level:** every `name:` definition is
collected into a set before any instruction is parsed into a builder call,
and every `jump`/`jump_if_false` target is checked against that set before
`ProgramBuilder` is ever touched. A label used before its definition
(`jump end` on line 1, `end:` on line 20) therefore resolves exactly the
same way a backward reference does — there's no "forward declaration"
concept to learn. This also means `ProgramBuilder::build`'s own
`UnresolvedLabel` error can never actually surface from assembled source
(every label this assembler creates via `new_label()` is always placed),
which is why `AsmError::Build` is documented as being, in practice, only
`TooManyConstants`/`ProgramTooLarge`.

**No constant-pool deduplication.** Each `load_const <literal>` calls
`add_const` fresh, even if the exact same value already has a pool entry
from an earlier line in the same source. Simpler, and the 16-bit pool
(65,536 entries) is nowhere near a practical concern for anything written
by hand; a real compiler sitting on top of this (there is none yet) would
be the natural place to add deduplication if it ever matters.

**One label per line.** `foo: bar: halt` is not supported — the line
splits on the *first* `:` only, so a second label on the same line
becomes part of the instruction text and fails as an unknown mnemonic.
Documented rather than special-cased; writing each label on its own line
is one keystroke, not a real limitation in practice.

**Tests:** inline. The lib-doc `(2+3)*4` expression and the loop-sum-1..10
program (both also exercised as doctests — the latter is the same logic
as `gindema-cli`'s `build_demo_program`, written as text instead of Rust);
a forward label reference; a label and instruction sharing one line;
comments and blank lines being ignored; `load_const` parsing int/float/
bool literals; the `.locals` default and an explicit value; every error
variant (unknown directive, duplicate `.locals`, unknown mnemonic,
undefined label, duplicate label, missing/too-many operands including the
zero-operand case, an invalid literal and an out-of-range slot index); a
`BuildError::TooManyConstants` propagating through as `AsmError::Build`;
and a full `to_bytes`/`from_bytes` round trip of an assembled program.

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
one, copy the operand bits through unchanged.

**Walks exactly `Program::code_bit_len` bits — not until the first `Halt`.**
The original version of this function stopped decoding at the first `Halt`
opcode it saw, on the reasoning that every program ends in one. That broke
the first time it met a program containing a `gindema-obfuscate` honeypot:
a honeypot's dead-end block has its own `Halt`, positioned in the raw byte
stream *before* the program's real one (even though it's never actually
reached at runtime — `Vm::run` only gets to a `Halt` by really executing
there, which this function never does; it just walks bytes front-to-back).
The old version silently truncated the remap at that point, dropping
everything genuinely after it. See `program.rs`'s note on
`code_bit_len` for the fix and why "decode until no bits remain" isn't
safe either (trailing byte-alignment padding can itself look like a short
valid instruction).

**Tests:** inline. A real program (with a loop, so jump targets are
actually exercised) remapped under a deliberately scattered, reversed
permutation: same byte length, different bytes, and — run through `Vm`
both ways — identical result. Remapping with the same map the program
already uses is a byte-for-byte no-op. Remapping to a scrambled map and
then back to identity recovers the exact original bytes. A dedicated
regression test reproduces the honeypot-shaped bug directly with
`ProgramBuilder` (this crate can't depend on `gindema-obfuscate` to use
its real `inject_honeypot`): an always-taken branch skips over dead code
containing its own `Halt`, followed by real, reachable code — remapping
and then running it must still reach that real code, not stop at the dead
`Halt`.

### `decode.rs`
**What it does:** `decode_program(&Program) -> Result<Vec<DecodedInstr>,
VmError>` — walks a program's stream (exactly `code_bit_len` bits, same
exact-length discipline as `remap_program`, for the same honeypot-shaped
reason) and returns every instruction's opcode, interpreted `Operand`
(`None`/`ConstIndex(u16)`/`LocalSlot(u8)`/`JumpTarget(u64)`), and the bit
range it occupies. `resolve_jump_target(&[DecodedInstr], end_bit, target)
-> Result<Option<usize>, VmError>` resolves a `Jump`/`JumpIfFalse` target
to the index of the instruction it points at, or `None` for the "jump past
the very last instruction" case (a label placed at a program's end with
nothing after it — see `program.rs`'s own note on why that's a valid
target with no instruction actually there).

**Why this exists as its own thing, separate from `vm.rs`'s and
`remap.rs`'s own decode loops:** those two decode-and-immediately-*do*-
something (execute; re-encode) one instruction at a time, and don't need a
structured result. `gindema-obfuscate`'s control-flow flattening pass
needs the *opposite*: the whole instruction list up front, with real bit
offsets, before it can find basic-block boundaries at all. Rather than
have flattening grow its own third copy of the opcode/operand decode
logic, it's factored out here once. Also closes part of the former "no
disassembler" known limit — not a human-readable disassembler (nothing
here prints anything), but the structural half of one.

**`bit_offset` is captured before fluff is skipped, deliberately:** a
`DecodedInstr`'s `bit_offset` always equals the value a `Jump`/
`JumpIfFalse` targeting it actually encodes, whether or not the program
carries fluff — see `fluff.rs`'s section below for why that specific
convention is what keeps fluff invisible to every caller of this function,
`gindema-obfuscate`'s flattening pass included.

**Tests:** inline. Straight-line arithmetic decodes in order with the
right operands; a loop program's two jump targets both resolve to a real
instruction's `bit_offset`; `resolve_jump_target` recognizes the implicit
end and rejects a target matching no instruction at all (line 3 of a
single-`Halt` program, Debug-printed by the assertion that actually proves
it rather than the lucky-coincidence padding path that would otherwise
decode it as a bogus instruction); decoding a fluffed program gives back
the identical opcodes/operands (modulo jump targets' absolute values,
which necessarily shift) as decoding the same program unfluffed.

### `fluff.rs`
**What it does:** `interleave_fluff(&Program, seed: u64) -> Result<Program,
VmError>` — the bytecode-format mechanism behind state-dependent decoding
(see the Overview section above for the mechanism/policy split).
`fluff_len_at(seed, bit_pos) -> u32` (private) is the single function both
the encoder and `Vm::run`'s decode loop call to agree on how much junk
precedes the instruction at `bit_pos`, under `seed` — a pure function of
those two numbers, nothing else, capped at `MAX_FLUFF_BITS = 7`.
`skip_fluff`/`copy_fluff` (private, `pub(crate)`) are the shared "do the
right thing at this position" helpers `vm.rs`, `decode.rs`, and `remap.rs`
all call rather than each reimplementing the same skip logic slightly
differently.

**Why bit *position*, not instruction *index*, is the key:** the first
design that comes to mind — draw a fluff length per sequential instruction
index from one continuously-advancing RNG stream — breaks the moment a
`Jump` lands somewhere other than "the next instruction in program order."
The VM would need to know *which* instruction index it just landed on to
resume the stream correctly, which means either threading an index through
every jump target (a second format change on top of fluff itself) or
re-deriving it some other way. Keying fluff length off the reader's
*current bit position* instead sidesteps this entirely: wherever the
reader ends up — by falling through sequentially, or by
`BitReader::seek_to_bit` from a jump — recomputing `fluff_len_at(seed,
reader.bit_pos())` always recovers the exact same length the encoder used
there, because the encoder computed it from that exact same position when
it originally wrote the fluff. No index, counter, or table, anywhere.
Jump targets keep meaning "an absolute bit offset" exactly like they
always have — they just now point at where an instruction's *fluff*
starts, not its opcode.

**Composability with the other three passes:** `interleave_fluff` is
built on `decode_program`, so it transparently re-fluffs an
already-fluffed program under a new seed (discarding the old fluff, not
stacking two layers) and tolerates a non-identity opcode map already
being in place. `remap_program` was updated (via the shared `copy_fluff`
helper) to read-and-copy-through any existing fluff unchanged rather than
misreading it as a bogus opcode, carrying the same `fluff_seed` into its
output — so `fluff` then `remap` composes in either order. Flattening
does *not* compose transparently the same way: `flatten_control_flow`
always rebuilds via `ProgramBuilder`, which has no fluff concept, so
flattening an already-fluffed program silently produces a *clean* (never
fluffed) result — see `docs/gindema-obfuscate.md`'s own notes on why
fluff (and remap) belong *after* flattening in a pipeline, not before.

**Tests:** inline. A fluffed program's bit length grows (or, for a program
too short to get any nonzero-length draws, stays equal — never shrinks)
and carries its seed; running a fluffed loop program gives the identical
answer running the original does; the same seed is byte-for-byte
deterministic, different seeds usually differ; re-fluffing an already-
fluffed program replaces (doesn't stack) the old fluff; a program with a
dead honeypot-shaped branch still decodes its real tail correctly once
fluffed (mirroring `remap.rs`'s own regression test, for the same
honeypot-shaped reason, applied to this pass instead); remapping a fluffed
program preserves both its fluff and its behavior; a fluffed program
round-trips through `to_bytes`/`from_bytes` correctly; `fluff_len_at` never
exceeds `MAX_FLUFF_BITS`; `fluff_bits_at` never produces a value wider than
its requested width (including the zero-width case).

### `program.rs`
**What it does:** `Instr` (the builder's intermediate instruction
representation — jump operands are `LabelId`s here, not yet bit offsets),
`LabelId`, `BuildError`, `Program` (the assembled result: bit-packed `code`,
`constants`, `local_count`, the `OpcodeMap` the stream was encoded with,
and — added alongside the fix described in `remap.rs`'s section above —
`code_bit_len`: the exact real bit length of `code`, as opposed to
`code.len() * 8`, which also counts trailing byte-alignment padding.
`code_bytes()`/`opcode_map()`/`code_bit_len()` are accessors for passes
like `remap_program` that transform an already-built program), and
`ProgramBuilder`.

**Why `code_bit_len` has to be tracked explicitly, not inferred:** a pass
that walks every instruction without following control flow (unlike
`Vm::run`, which always does) has no reliable way to tell where real
content ends otherwise. Stopping at the first `Halt` is wrong once dead
code can contain one earlier in the stream (see `remap.rs`). Stopping only
when no bits remain is *also* wrong: `BitWriter::into_bytes`'s trailing
zero-padding, up to 7 bits, can itself be long enough to misdecode as a
short, spurious instruction. `ProgramBuilder::build` already computes the
real bit length in its first pass (to resolve labels) — it was simply
being discarded afterward; now it's kept.

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

**`fluff_seed: Option<u64>`:** `Some(seed)` once a program has been through
`interleave_fluff`, `None` otherwise — `ProgramBuilder::build` always
produces `None`, the same way it always produces `OpcodeMap::identity()`.
See `fluff.rs`'s section above.

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

### `serialize.rs`
**What it does:** `Program::to_bytes(&self) -> Vec<u8>` /
`Program::from_bytes(&[u8]) -> Result<Program, SerializeError>` — a
self-contained binary file format: magic `b"GVMP"`, a `u16` version
(currently `2`), the `local_count` byte, `code_bit_len` as a little-endian
`u64`, a `fluff_seed` presence byte (`0` absent, `1` present — followed by
the 8-byte little-endian seed only when present, so the common unfluffed
case costs one extra byte over the pre-fluff format, not a fixed nine),
the opcode table as `OPCODE_COUNT` raw bytes (one per `OpCode::ALL` entry
— enough on its own to rebuild the full `OpcodeMap` via
`from_permutation`), the constant pool (a `u32` count, then each value as
a 1-byte type tag plus an 8-byte little-endian payload, or 1 byte for
`Bool`), then the bytecode itself (a `u32` length, then that many bytes).
Little-endian and byte-aligned throughout — deliberately not bit-packed
the way `code` itself is; there's no reason for the *container* around an
already bit-packed stream to also be bit-level, and byte alignment makes
the format easier to read by eye in a hex dump.

**Version bumped 1 → 2 for `fluff_seed`, a breaking change, deliberately:**
a version-1 file's bytes have no `fluff_seed` field at all, and there's no
way to tell "this is an old file with no fluff" apart from "this is a
corrupted new file" from the bytes alone — so version 1 is now rejected
with `SerializeError::UnsupportedVersion(1)` rather than silently assumed
to mean `fluff_seed: None`. See "Fixes and Problems" below.

This is what makes a `Program` persistable at all — without it, a program
only ever exists for the lifetime of one process. `gindema-cli`'s `demo`/
`info`/`run`/`remap` subcommands all go through this.

A small private `ByteReader` (this file only — not a general-purpose
binary-parsing library, since this format has exactly one reader and one
writer and both live here and change together) reads length-prefixed
fields and reports `SerializeError::UnexpectedEof` rather than panicking
on a truncated buffer.

**Tests:** inline. Round-trips a program containing all three `Value`
kinds (`to_bytes` then `from_bytes` gives back an equal `Program`, and
running both gives the same result); round-trips a *remapped* program's
non-identity opcode table specifically, not just the identity case;
round-trips a *fluffed* program's seed specifically, confirming it still
runs correctly after the round trip; wrong magic, an unsupported version,
an invalid constant type tag, an invalid fluff-flag byte, and a corrupted
(duplicate-raw-value) opcode table are all rejected with the right
`SerializeError` variant; version `1` specifically (the pre-fluff format,
not just an arbitrary unsupported value) is confirmed rejected; truncating
the byte buffer at every possible length is rejected with `UnexpectedEof`
rather than panicking (not just one hand-picked truncation point).

## Known limits (intentional, for this pass)

- No functions or calls — no call stack, no `Call`/`Return` opcodes. Every
  program is one flat instruction stream.
- No strings, arrays, or heap objects — `Value` is `Int`/`Float`/`Bool` only.
- No disassembler — `assemble` goes one way (text to `Program`); `decode_program`
  gives the structural half of a disassembler back (a `Program` to a
  decoded instruction list), but nothing here prints readable assembly
  text from one yet.
- `assemble`'s own known limits (no constant-pool deduplication, one label
  per line) are documented on that module above, not repeated here.
- `ProgramBuilder::build` always produces the identity `OpcodeMap` and
  `fluff_seed: None` — this crate never randomizes a program's own
  numbering or adds fluff on its own. The mechanisms exist
  (`remap_program`, `interleave_fluff`), but choosing or generating a
  permutation or a seed is entirely `gindema-obfuscate`'s job.
  Control-flow flattening and honeypot/dead-end injection have no
  supporting mechanism here at all (flattening needed one, `decode_program`
  — honeypot injection didn't, since it works entirely through
  `ProgramBuilder`'s existing public API).
- `MAX_FLUFF_BITS = 7` is a fixed constant, not configurable per call —
  reasonable for a first pass; a program with unusually tight size
  constraints has no way to ask for less overhead than that.
- `gindema-ptr` is not yet used anywhere in this crate (see "Overview").
- The interpreter's bit-by-bit decode (inherited from `gindema-bits`, see
  that crate's own "Known limits") is not optimized; nothing here has shown
  it needs to be yet.

## CI and Workflows

See `.github/workflows/ci.yml` (workspace-wide, workflow_dispatch only).
`cargo test -p gindema-vm` runs 75 unit tests + 2 doctests (up from 58 + 2:
5 new in `decode.rs`, 9 new in `fluff.rs`, 3 new in `serialize.rs`'s own
tests for the format change); `cargo clippy -p gindema-vm --all-targets --
-D warnings` and `cargo fmt -p gindema-vm -- --check` are both clean.
Verified in the sandbox on Rust 1.100. This crate was part of the
workspace-wide `cargo test`/`clippy`/`fmt` that `ci.yml` ran successfully
on the real GitHub Actions runner at commit `f8e0115` (189 tests) — that
confirmation predates this session's `decode.rs`/`fluff.rs` work, which is
sandbox-verified only so far; see the root `README.md`/`CHANGELOG.md`.

## Fixes and Problems

- **Deliberate breaking format change, not a bug fix:** adding
  `fluff_seed` to `Program` meant `serialize.rs`'s file format needed a new
  field, and there's no way to add it such that an *old* (version-1) file's
  bytes could still be told apart from "this is secretly fluffed but the
  reader doesn't know to look." Rather than guess or default silently,
  `VERSION` was bumped to `2` and version `1` now fails with
  `UnsupportedVersion(1)` — any `.gvm` file written before this change
  needs regenerating (via `gindema-cli demo`/`assemble`) to load again. A
  dedicated test (`version_1_the_pre_fluff_format_is_now_unsupported`)
  pins this down explicitly, separate from the pre-existing "some arbitrary
  unsupported version is rejected" test, so the breaking change itself has
  its own regression coverage.
- **Design decision worth recording:** state-dependent decoding's fluff
  length is keyed on the reader's current *bit position*, not a
  sequentially-advancing instruction *index* — see `fluff.rs`'s section
  above for why the index-based version breaks the moment a `Jump` lands
  anywhere other than the next instruction in program order, and how
  keying off position instead avoids needing any new per-jump bookkeeping
  at all.
- **Real bug, caught by `gindema-cli`'s own tests, not this crate's own:**
  `remap_program` originally stopped decoding at the first `Halt` opcode.
  The first time a program containing a `gindema-obfuscate` honeypot was
  remapped (via `gindema-cli demo --honeypots 1` followed by `remap`), this
  silently truncated the output at the honeypot's own dead-end `Halt` —
  which sits earlier in the raw byte stream than the program's real one,
  even though it's never actually reached at runtime — dropping every real
  instruction after it. Fixed by adding `Program::code_bit_len` (the exact
  real bit length, already computed by `ProgramBuilder::build`'s label-
  resolution pass but previously discarded) and having `remap_program`
  decode exactly that many bits instead of watching for `Halt`. A
  regression test reproducing the exact shape (branch always skips over
  dead code containing its own `Halt`, followed by real code) was added
  directly in `remap.rs`'s own tests, using plain `ProgramBuilder` (this
  crate can't depend on `gindema-obfuscate` for its real `inject_honeypot`
  without a dependency cycle). `Program::to_bytes`/`from_bytes` were
  updated in the same change to persist `code_bit_len`, since it's now part
  of what a program *is*, not an optional extra.
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
