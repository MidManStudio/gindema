# gindema-cli

## Overview

The distributable protection-core CLI: assembles, builds, remaps, runs,
and inspects gindema-vm bytecode program files, and packs/unpacks
gindema-container asset archives. Previous sessions' load-bearing
limitation — **no assembly language or general-purpose compiler front end
anywhere in the workspace**, so `demo`'s one fixed, hardcoded program was
the only way to get a program file at all — is now closed:
`gindema-vm::assemble` exists, and this crate's new `assemble` subcommand
is a thin wrapper around it. `demo` still exists, unchanged, since it's
still useful as a zero-setup smoke test.

Ties together `gindema-vm` (`Program`, `ProgramBuilder`, `Vm`, `assemble`,
`Program::to_bytes`/`from_bytes`), `gindema-obfuscate` (`inject_honeypot`,
`remap_opcodes`), and now `gindema-container` (`ContainerBuilder`,
`ContainerKey`, `ContainerReader`) via `pack`/`unpack`. Still does
**not** use `gindema-guard` — see "Known limits" for why it doesn't fit
this crate's actual job, rather than it simply being forgotten.

## Subcommands

```
gindema-cli demo <output> [--honeypots N] [--seed S]
gindema-cli assemble <source> <output>
gindema-cli info <input>
gindema-cli run <input>
gindema-cli remap <input> <output> --seed <S>
gindema-cli pack <output> <input>... [--honeypots N] [--seed S]
gindema-cli unpack <input> --key <BASE64> <output-dir>
gindema-cli help
```

- **`demo`** builds the fixed demo program (sums `1..=10` via a real loop —
  always `55` — see `commands.rs` below), optionally injecting up to 3
  honeypots (`--honeypots`, seeded from `--seed`), and writes it to
  `output` via `Program::to_bytes`.
- **`assemble`** reads `source` as gindema-vm assembly text (see
  `gindema_vm::assemble` / `docs/gindema-vm.md`'s `asm.rs` section for the
  syntax), compiles it, and writes the result to `output` the same way
  `demo` does. The first way to express an arbitrary program from the
  command line.
- **`info`** loads a program file and prints its bytecode length,
  constant count, local count, and whether its opcode table is the
  identity mapping or a remap — without running it.
- **`run`** loads and runs a program file through a fresh `Vm`, printing
  whatever's left on the stack.
- **`remap`** loads a program file, calls `gindema_obfuscate::remap_opcodes`
  with the given seed, and writes the result to a new file. The output is
  the same length and behaves identically — only its opcode numbering
  differs; see `docs/gindema-vm.md` and `docs/gindema-obfuscate.md` for why
  that's guaranteed rather than merely expected.
- **`pack`** reads every `<input>` file, packs them into a new
  gindema-container archive (`--honeypots`/`--seed` as for `demo`'s
  honeypots, but these are decoy *asset slots*, not bytecode — see
  `docs/gindema-container.md`), writes the archive to `output`, and prints
  a freshly generated base64 key. The key is **not** saved anywhere else —
  losing the printed key means losing the archive.
- **`unpack`** opens an archive with `--key`, decrypts the index, and
  writes every real asset into `output-dir` as `asset-NNNN.bin` (4-digit
  zero-padded 0-based order). The container stores no filenames by design
  (see `docs/gindema-container.md`'s "Known limits"), so original names
  are never recovered — this is expected, not a bug.

## Modules

### `main.rs`
**What it does:** crate doc, parses arguments via `cli::parse_args`,
dispatches to `commands::run_*`, prints `cli::USAGE` for `help` or a parse
error, exits `2` on a bad invocation and `1` on a command that ran but
failed (file not found, a malformed program file, a runtime error) — the
usual Unix convention of distinguishing "you used me wrong" from "something
you asked for didn't work."

### `cli.rs`
**What it does:** `Command` (one variant per subcommand, including
`Help`), `parse_args(&[String]) -> Result<Command, ArgsError>`, and
`USAGE`. Deliberately hand-rolled rather than built on a crate like `clap`:
seven subcommands with a handful of positional paths, a variable-length
input list for `pack`, and a few optional `--seed`/`--honeypots`/`--key`
flags between them still doesn't need a general-purpose argument parser,
and keeping this crate's own dependency footprint small felt like the
right trade for that parsing logic. `parse_pack` is the one subcommand
whose positional count isn't fixed — it splits on a leading `output` plus
one-or-more trailing `inputs` via a slice pattern (`[output, inputs @ ..]`)
the same way `parse_remap`'s fixed-arity `[input, output]` pattern already
worked, just with a variable-length tail. Kept entirely separate from
actually running a command (`commands.rs`) specifically so parsing itself
is unit-testable without touching the filesystem — every test in this file
calls `parse_args` directly and checks the resulting `Command` or that it's
an `Err`, never spawning the actual binary.

**Tests:** inline, 15 (up from 9). No args or `help`/`--help` all give
`Command::Help`; `demo` with no flags defaults `honeypots`/`seed` to `0`;
`demo` with both flags in either order; `demo` missing its output path
errors; `info`/`run` each require exactly one path (zero or two both
error); `remap` needs both paths and `--seed`, and errors without the
seed; an unknown subcommand and an unknown flag both error; a non-numeric
`--seed` value errors instead of panicking on the `.parse::<u64>()` call;
`assemble` needs exactly a source and an output path; `pack` with one
input and no flags, with multiple inputs and both flags, and missing its
output or any input all error correctly; `unpack` with `--key` and two
paths parses, and errors without `--key`.

### `commands.rs`
**What it does:** `build_demo_program(honeypot_count, seed) -> Program` —
the one fixed demo program (2 locals: a running sum and a counter; sums
`1..=10` via `Lt`/`JumpIfFalse`/`Jump`, so there's a real loop and real
jump targets for the rest of the pipeline to exercise, not just straight-
line arithmetic) — plus `run_demo`/`run_info`/`run_run`/`run_remap`/
`run_assemble`/`run_pack`/`run_unpack`, each doing real file I/O
(`std::fs::read`/`write`) and returning `Result<(), Box<dyn Error>>`.
`Box<dyn Error>` rather than a crate-specific enum: this is the actual end
of the pipeline (nothing calls these functions except `main`, which only
ever needs to print the error and set an exit code), so there's no caller
that would benefit from matching on a specific variant the way
`VmError`/`BuildError`/`SerializeError`/`ObfuscateError`/`ContainerError`
all need to be matchable deeper in the stack. `pack_assets`/`unpack_all`
are `run_pack`/`run_unpack`'s filesystem-free cores — `pack_assets` takes
already-read asset bytes and returns `(ContainerKey, Vec<u8>)`;
`unpack_all` takes archive bytes and a key and returns every real asset's
bytes, in order — kept separate from the `std::fs` calls around them for
the same reason `build_demo_program` is separate from `run_demo`: so the
logic itself is unit-testable without touching disk.

**Tests:** inline, 10 (up from 6), all pure (no filesystem access —
`run_*`'s file I/O isn't itself unit tested, only `build_demo_program`/
`pack_assets`/`unpack_all` and the library calls they feed into). The demo
program with zero honeypots sums to `55`; with all 3 honeypot slots
filled, still `55`; more honeypots means a strictly larger program (not a
no-op); the same `(honeypot_count, seed)` pair gives byte-identical
output; the demo program round-trips through `to_bytes`/`from_bytes` and
still runs correctly; remapping the demo program (the one with a honeypot
included) preserves its behavior — this last test is the one that
actually caught the real `gindema-vm` bug described below, the first time
it ran; assembling a short text program produces a `Program` that runs
correctly; packing then unpacking round-trips the original asset bytes
exactly; honeypots don't change which assets come back or their order;
unpacking with the wrong key fails cleanly (an `Err`, not a panic).

## Known limits (intentional, for this pass)

- **No disassembler.** `assemble` goes one way (text to a program file);
  there's no way to recover readable assembly from an existing `.gvm`
  file, only `info`'s coarse byte/constant/local-count summary. See
  `docs/gindema-vm.md`'s `asm.rs` section.
- **`gindema-guard` isn't used here, on purpose.** Its primitives
  (`TimingWindow`, `Watch`, `GuardState`, `Guarded<T>`) are meant to run
  *inside a shipped game* watching the game's own runtime state — there
  is no game in this repository for a dev-time CLI tool to protect, and
  forcing an artificial integration (having the CLI watch its own output
  file, say) wouldn't demonstrate anything the crate's own tests don't
  already cover more directly.
- **`pack`/`unpack` don't preserve filenames**, because the container
  format doesn't store them at all, by design (see
  `docs/gindema-container.md`'s "Known limits") — extracted assets are
  named only by their 0-based order. A CLI-level manifest mapping order
  back to original filenames would leak exactly the information the
  format deliberately omits, so this wasn't added as a workaround.
- **`pack`/`unpack` have no password-based key option** — `gindema-cli`
  only ever calls `ContainerKey::generate()` (dixscript's own CSPRNG path)
  and prints the result; `dixscript`'s `Argon2KDF`/`set_password` path
  isn't exposed here, matching `gindema-container`'s own current scope.
- **`gindema-ptr`/`gindema-bits` are transitively present but not used
  directly** — `gindema-vm` and `gindema-obfuscate` already use
  `gindema-bits`; nothing in this crate's own code needs either directly.
- **No build-pipeline integration beyond this CLI itself.** Running
  `remap`/`pack` by hand with a manually-chosen seed is the whole story
  right now; nothing automatically picks a fresh seed per build or calls
  this as part of a packaging step.

## CI and Workflows

See `.github/workflows/ci.yml` (workspace-wide, workflow_dispatch only).
`cargo test -p gindema-cli` runs 25 unit tests (up from 15: 15 in `cli.rs`,
10 in `commands.rs`); `cargo clippy -p gindema-cli --all-targets -- -D
warnings` and `cargo fmt -p gindema-cli -- --check` are both clean. Beyond
the unit tests, the actual built binary was run end-to-end by hand in the
sandbox twice this session: the original `demo --honeypots 3` → `info` →
`run` (`Int(55)`) → `remap --seed 777` → `info`/`run` on the remapped file
(`opcode map: remapped (non-identity)`, `Int(55)` again) → `run` on a
nonexistent file (exit code 1); and, for the new subcommands, a
hand-written `.gasm` loop-sum-to-10 program through `assemble` → `info`/
`run` (`Int(55)`) → `remap` → `info`/`run` again, `assemble` on a file
with a deliberately bad mnemonic (exit code 1, clear "unknown instruction"
error, not a panic), and `pack` on two files → `unpack` with the printed
key (both files recovered byte-for-byte) → `unpack` with a malformed key
and separately with a well-formed-but-wrong key (both fail cleanly with
exit code 1, the latter via dixscript's own decryption-failure error, not
a panic) — not just `cargo test`. Verified in the sandbox on Rust 1.100.

This crate is also now part of the workspace-wide `cargo test`/`clippy`/
`fmt` that `ci.yml` has actually run successfully on the real GitHub
Actions runner for the first time (commit `f8e0115`, 189 tests passed) —
see the root `README.md`/`CHANGELOG.md`.

## Fixes and Problems

- The first real run of `commands::tests::remapping_the_demo_program_preserves_its_behavior`
  (a demo program built with one honeypot, then remapped) failed with
  `InvalidOpcode(0)` — not a bug in this crate, but the first thing in the
  whole workspace to actually exercise "remap a program that contains a
  honeypot," which surfaced a real bug in `gindema-vm`'s `remap_program`
  (it stopped decoding at the honeypot's own dead-end `Halt` instead of the
  program's real one). Fixed in `gindema-vm` — see that crate's own "Fixes
  and Problems" for the full story — after which this test, and the rest
  of this crate's suite, passed without further changes needed here. Left
  as a visible example of integration testing doing its job: a bug that
  three different crates' own unit tests, each passing in isolation,
  didn't catch on its own.
